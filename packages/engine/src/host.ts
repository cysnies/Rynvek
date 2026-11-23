// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 主线程引擎宿主：把 `<canvas>` 的控制权经 `transferControlToOffscreen()` 永久移交给 Dedicated Engine Worker，
 * 维护宿主状态机，并把 `PICK_REQUEST` / `PICK_RESULT` 按 `requestId` 关联为 Promise。
 *
 * ```text
 * idle ──attach──► initializing ──ENGINE_READY──► ready ◄──RECOVERED── recovering
 *                      │                            │  └──DEVICE_LOST──►─┘
 *                      │ ENGINE_ERROR(0x6001)       └──BACKEND_DEGRADED──► degraded（CPU Reference 继续运行）
 *                      ▼
 *                    failed            任意状态 ──shutdown()──► shutdown
 * ```
 *
 * 只依赖结构化类型（`WorkerLike` / `TransferableCanvasLike`），不引入 DOM 类型库，便于在 Node 中以假对象测试。
 */

import {
  type ComputeBackendKind,
  type EngineCommand,
  type EngineEvent,
  type EngineEventOf,
  type EngineEventType,
  type EngineInitMessage,
  type EngineInitOptions,
  parseEngineEvents,
} from "./protocol.js";

/** 最小 Worker 接口（浏览器 `Worker` 天然满足） */
export interface WorkerLike {
  postMessage(message: unknown, transfer?: unknown[]): void;
  addEventListener(type: "message" | "error" | "messageerror", listener: (ev: { data?: unknown }) => void): void;
  removeEventListener(type: "message" | "error" | "messageerror", listener: (ev: { data?: unknown }) => void): void;
  terminate?(): void;
}

/** 可移交控制权的画布（浏览器 `HTMLCanvasElement` 天然满足） */
export interface TransferableCanvasLike<O = unknown> {
  transferControlToOffscreen(): O;
}

/** 宿主阶段 */
export type EngineHostState = "idle" | "initializing" | "ready" | "recovering" | "degraded" | "failed" | "shutdown";

export type EngineHostErrorCode =
  | "CANVAS_ALREADY_TRANSFERRED"
  | "NOT_ATTACHED"
  | "HOST_CLOSED"
  | "PICK_TIMEOUT"
  | "INVALID_ARGUMENT";

/** 宿主侧错误（与 Worker 回报的 `ENGINE_ERROR` 区分） */
export class EngineHostError extends Error {
  readonly code: EngineHostErrorCode;
  constructor(code: EngineHostErrorCode, message: string) {
    super(message);
    this.name = "EngineHostError";
    this.code = code;
  }
}

export interface TimerApi {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export interface EngineHostOptions {
  /** 拾取请求超时（毫秒，缺省 2000） */
  pickTimeoutMs?: number;
  /** 可注入的计时器（测试用；缺省 globalThis） */
  timers?: TimerApi;
  /** `shutdown()` 后是否同时 `terminate()` Worker（缺省 true） */
  terminateOnShutdown?: boolean;
}

/** 拾取结果：被拾取实体的持久单调序号（不是稠密槽位），未命中为 null */
export type PickHit = number | null;

interface PendingPick {
  resolve(hit: PickHit): void;
  reject(err: Error): void;
  timer: unknown;
}

type Listener<T extends EngineEventType> = (event: EngineEventOf<T>) => void;
type AnyListener = (event: EngineEvent) => void;

/** 全局记录已移交的画布：同一画布无论经由哪个宿主都不能二次移交 */
const transferredCanvases = new WeakSet<object>();

/** 浏览器与 Node 都提供的全局计时器（不依赖 DOM / Node 类型库） */
const globalTimers = globalThis as unknown as {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
};

const defaultTimers: TimerApi = {
  setTimeout: (fn, ms) => globalTimers.setTimeout(fn, ms),
  clearTimeout: (h) => globalTimers.clearTimeout(h),
};

const MAX_REQUEST_ID = 0xffff_ffff;

export class EngineHost {
  readonly #worker: WorkerLike;
  readonly #timers: TimerApi;
  readonly #pickTimeoutMs: number;
  readonly #terminateOnShutdown: boolean;
  #state: EngineHostState = "idle";
  #backend: ComputeBackendKind | null = null;
  #generation = 0;
  #lastTick = 0;
  #lastStateRoot: string | null = null;
  #nextRequestId = 1;
  readonly #pending = new Map<number, PendingPick>();
  readonly #listeners = new Map<EngineEventType | "*", Set<AnyListener>>();
  #rejectedMessages = 0;
  #lastError: EngineEventOf<"ENGINE_ERROR"> | null = null;

  readonly #onMessage = (ev: { data?: unknown }): void => this.#receive(ev.data);
  readonly #onError = (): void => this.#fail("Worker 脚本错误");

  constructor(worker: WorkerLike, options: EngineHostOptions = {}) {
    this.#worker = worker;
    this.#timers = options.timers ?? defaultTimers;
    this.#pickTimeoutMs = options.pickTimeoutMs ?? 2000;
    this.#terminateOnShutdown = options.terminateOnShutdown ?? true;
    worker.addEventListener("message", this.#onMessage);
    worker.addEventListener("error", this.#onError);
    worker.addEventListener("messageerror", this.#onError);
  }

  get state(): EngineHostState {
    return this.#state;
  }
  get backend(): ComputeBackendKind | null {
    return this.#backend;
  }
  get generation(): number {
    return this.#generation;
  }
  get lastTick(): number {
    return this.#lastTick;
  }
  get lastStateRoot(): string | null {
    return this.#lastStateRoot;
  }
  get pendingPicks(): number {
    return this.#pending.size;
  }
  /** 未通过协议校验而被丢弃的 Worker 消息数 */
  get rejectedMessages(): number {
    return this.#rejectedMessages;
  }
  get lastError(): EngineEventOf<"ENGINE_ERROR"> | null {
    return this.#lastError;
  }

  /**
   * 移交画布并发送 `ENGINE_INIT`。画布控制权一经移交不可收回：同一宿主或同一画布的第二次调用直接抛错，
   * 不会再次调用 `transferControlToOffscreen()`。
   */
  attach<O>(canvas: TransferableCanvasLike<O>, init: EngineInitOptions): void {
    if (this.#state === "shutdown" || this.#state === "failed") {
      throw new EngineHostError("HOST_CLOSED", "宿主已关闭");
    }
    if (this.#state !== "idle") {
      throw new EngineHostError("CANVAS_ALREADY_TRANSFERRED", "宿主已移交过画布，ENGINE_INIT 只允许一次");
    }
    if (transferredCanvases.has(canvas)) {
      throw new EngineHostError("CANVAS_ALREADY_TRANSFERRED", "该画布的控制权已移交给其他 Worker");
    }
    const width = Math.floor(init.width);
    const height = Math.floor(init.height);
    if (!(width > 0 && height > 0)) {
      throw new EngineHostError("INVALID_ARGUMENT", `画布尺寸非法：${init.width}×${init.height}`);
    }
    const offscreen = canvas.transferControlToOffscreen();
    transferredCanvases.add(canvas);
    const message: EngineInitMessage<O> = {
      type: "ENGINE_INIT",
      canvas: offscreen,
      width,
      height,
      devicePixelRatio: init.devicePixelRatio,
      worldId: init.worldId ?? null,
      gpuMode: init.gpuMode,
      ...(init.arenaFaction !== undefined ? { arenaFaction: init.arenaFaction } : {}),
      ...(init.generalRemote !== undefined ? { generalRemote:init.generalRemote } : {}),
    };
    this.#state = "initializing";
    this.#worker.postMessage(message, [offscreen]);
  }

  /** 发送一条命令（初始化期间由 Worker 缓存并在就绪后按序回放） */
  send(command: EngineCommand): void {
    this.#ensureOpen();
    this.#worker.postMessage(command);
    if (command.type === "SHUTDOWN") this.#close();
  }

  /** 明确载入非 RTS 的有界原 RYWL；执行与本地/创世权限在 Worker 再验。 */
  loadGeneralWorld(bytes: Uint8Array): void {
    if (!(bytes.length > 0 && bytes.length <= 128 * 1024)) throw new EngineHostError("INVALID_ARGUMENT", "通用 World 包须为 1–131072 字节");
    this.send({ type: "GENERAL_WORLD", packageBytes: Array.from(bytes) });
  }

  step(ticks = 1): void {
    this.send({ type: "STEP", ticks });
  }
  start(tickRate = 30): void {
    this.send({ type: "START", tickRate });
  }
  pause(): void {
    this.send({ type: "PAUSE" });
  }
  resize(width: number, height: number, devicePixelRatio: number): void {
    this.send({ type: "RESIZE", width: Math.max(1, Math.floor(width)), height: Math.max(1, Math.floor(height)), devicePixelRatio });
  }

  /** 画布设备像素坐标拾取；解析为持久单调序号或 null（未命中） */
  pick(x: number, y: number): Promise<PickHit> {
    try {
      this.#ensureOpen();
    } catch (e) {
      return Promise.reject(e as Error);
    }
    if (!(Number.isFinite(x) && Number.isFinite(y) && x >= 0 && y >= 0)) {
      return Promise.reject(new EngineHostError("INVALID_ARGUMENT", `拾取坐标非法：(${x}, ${y})`));
    }
    const requestId = this.#allocRequestId();
    return new Promise<PickHit>((resolve, reject) => {
      const timer = this.#timers.setTimeout(() => {
        if (this.#pending.delete(requestId)) {
          reject(new EngineHostError("PICK_TIMEOUT", `拾取请求 ${requestId} 超时`));
        }
      }, this.#pickTimeoutMs);
      this.#pending.set(requestId, { resolve, reject, timer });
      this.#worker.postMessage({ type: "PICK_REQUEST", requestId, x: Math.floor(x), y: Math.floor(y) } satisfies EngineCommand);
    });
  }

  /** 订阅某类事件（`"*"` 订阅全部）；返回取消函数 */
  on<T extends EngineEventType>(type: T, listener: Listener<T>): () => void;
  on(type: "*", listener: AnyListener): () => void;
  on(type: EngineEventType | "*", listener: AnyListener): () => void {
    let set = this.#listeners.get(type);
    if (!set) {
      set = new Set();
      this.#listeners.set(type, set);
    }
    set.add(listener);
    return () => set.delete(listener);
  }

  /** 关闭引擎：发送 `SHUTDOWN`，拒绝全部未完成拾取，并（缺省）终止 Worker */
  shutdown(): void {
    if (this.#state === "shutdown") return;
    if (this.#state !== "idle" && this.#state !== "failed") {
      this.#worker.postMessage({ type: "SHUTDOWN" } satisfies EngineCommand);
    }
    this.#close();
  }

  #ensureOpen(): void {
    if (this.#state === "idle") throw new EngineHostError("NOT_ATTACHED", "尚未移交画布（先调用 attach）");
    if (this.#state === "shutdown" || this.#state === "failed") throw new EngineHostError("HOST_CLOSED", "宿主已关闭");
  }

  #allocRequestId(): number {
    // u32 回绕且跳过 0 与仍在等待中的编号
    for (;;) {
      const id = this.#nextRequestId;
      this.#nextRequestId = id >= MAX_REQUEST_ID ? 1 : id + 1;
      if (!this.#pending.has(id)) return id;
    }
  }

  #close(): void {
    this.#state = "shutdown";
    this.#rejectAll(new EngineHostError("HOST_CLOSED", "宿主已关闭"));
    this.#worker.removeEventListener("message", this.#onMessage);
    this.#worker.removeEventListener("error", this.#onError);
    this.#worker.removeEventListener("messageerror", this.#onError);
    if (this.#terminateOnShutdown) this.#worker.terminate?.();
  }

  #fail(reason: string): void {
    if (this.#state === "shutdown" || this.#state === "failed") return;
    this.#state = "failed";
    this.#rejectAll(new EngineHostError("HOST_CLOSED", reason));
  }

  #rejectAll(err: Error): void {
    for (const [, p] of this.#pending) {
      this.#timers.clearTimeout(p.timer);
      p.reject(err);
    }
    this.#pending.clear();
  }

  #receive(data: unknown): void {
    const { events, rejected } = parseEngineEvents(data);
    this.#rejectedMessages += rejected.length;
    for (const ev of events) this.#apply(ev);
  }

  #apply(ev: EngineEvent): void {
    if (this.#state === "shutdown") return;
    switch (ev.type) {
      case "ENGINE_READY":
        this.#backend = ev.backend;
        this.#generation = ev.generation;
        if (this.#state === "initializing") this.#state = "ready";
        break;
      case "TICK_COMMITTED":
        this.#lastTick = ev.tick;
        this.#lastStateRoot = ev.stateRoot;
        break;
      case "PICK_RESULT": {
        const p = this.#pending.get(ev.requestId);
        if (p) {
          this.#pending.delete(ev.requestId);
          this.#timers.clearTimeout(p.timer);
          p.resolve(ev.entitySerial);
        }
        break;
      }
      case "DEVICE_LOST":
        if (this.#state === "ready") this.#state = "recovering";
        break;
      case "RECOVERED":
        this.#generation = ev.generation;
        if (this.#state === "recovering") this.#state = "ready";
        break;
      case "BACKEND_DEGRADED":
        this.#backend = "cpu-reference";
        if (this.#state === "ready" || this.#state === "recovering") this.#state = "degraded";
        break;
      case "ENGINE_ERROR":
        this.#lastError = ev;
        // ENGINE_READY 之前的错误只可能来自初始化本身（Worker 在就绪后才回放缓存命令），
        // 如 gpuMode=required 无适配器（0x6001）或 worldId 非法（0x1004）：Worker 宿主已终止
        if (this.#state === "initializing") this.#fail(`初始化失败（0x${ev.code.toString(16)}）：${ev.message}`);
        break;
      case "FRAME_RENDERED":
      case "RECOVERY_PROGRESS":
      case "GPU_DIVERGENCE":
        break;
    }
    this.#emit(ev);
  }

  #emit(ev: EngineEvent): void {
    for (const key of [ev.type, "*"] as const) {
      const set = this.#listeners.get(key);
      if (!set) continue;
      for (const l of [...set]) l(ev);
    }
  }
}
