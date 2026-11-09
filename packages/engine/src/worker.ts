// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Dedicated Engine Worker 侧胶水：把 `postMessage` 消息转交 `rynvek-wasm` 导出的 `EngineWorker`，
 * 并以 `requestAnimationFrame`（Worker 内可用时）或定时器驱动帧回调。
 *
 * 用法（Worker 入口脚本）：
 *
 * ```ts
 * import init, { EngineWorker } from "rynvek-wasm";
 * await init();
 * runEngineWorker(self, new EngineWorker());
 * ```
 */

import { parseEngineEvents } from "./protocol.js";

/** `rynvek-wasm` 的 `EngineWorker` 导出（wasm-bindgen 生成的 d.ts 与之结构一致） */
export interface WasmEngineWorkerLike {
  init(canvas: unknown, messageJson: string): Promise<unknown>;
  post(messageJson: string): string;
  frame(nowMs: number): string;
  phase(): string;
}

/** Dedicated Worker 全局作用域的最小接口 */
export interface WorkerScopeLike {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (ev: { data?: unknown }) => void): void;
  requestAnimationFrame?(cb: (now: number) => void): number;
  setTimeout?(cb: () => void, ms: number): unknown;
  performance?: { now(): number };
}

export interface RunEngineWorkerOptions {
  /** 无 `requestAnimationFrame` 时的帧间隔（毫秒，缺省 16） */
  fallbackFrameMs?: number;
}

/** 运行中的 Worker 循环句柄 */
export interface EngineWorkerLoop {
  /** 当前是否已停止（收到 SHUTDOWN 或初始化失败） */
  readonly stopped: boolean;
  /** 手动驱动一帧（测试或自定义调度） */
  frame(nowMs: number): void;
}

function emit(scope: WorkerScopeLike, json: unknown): void {
  let data: unknown = json;
  if (typeof json === "string") {
    try {
      data = JSON.parse(json);
    } catch {
      return;
    }
  }
  const { events } = parseEngineEvents(data);
  // 批量事件按产生顺序逐条转发，主线程宿主可同时接受单条与数组
  if (events.length > 0) scope.postMessage(events);
}

/** 启动 Worker 消息循环 */
export function runEngineWorker(
  scope: WorkerScopeLike,
  engine: WasmEngineWorkerLike,
  options: RunEngineWorkerOptions = {},
): EngineWorkerLoop {
  const frameMs = options.fallbackFrameMs ?? 16;
  let stopped = false;
  let looping = false;
  // 初始化完成前到达的消息必须排在 ENGINE_INIT 之后，按序串行处理
  let queue: Promise<void> = Promise.resolve();

  const now = (): number => scope.performance?.now() ?? Date.now();
  const checkStopped = (): void => {
    if (engine.phase() === "shut-down") stopped = true;
  };

  const frame = (t: number): void => {
    if (stopped) return;
    emit(scope, engine.frame(t));
    checkStopped();
  };

  const schedule = (): void => {
    if (stopped) return;
    if (scope.requestAnimationFrame) {
      scope.requestAnimationFrame((t) => {
        frame(t);
        schedule();
      });
    } else if (scope.setTimeout) {
      scope.setTimeout(() => {
        frame(now());
        schedule();
      }, frameMs);
    }
  };

  scope.addEventListener("message", (ev) => {
    const data = ev.data as { type?: unknown; canvas?: unknown } | null | undefined;
    queue = queue.then(async () => {
      if (stopped) return;
      if (data && data.type === "ENGINE_INIT") {
        const { canvas, ...rest } = data;
        emit(scope, await engine.init(canvas, JSON.stringify(rest)));
        checkStopped();
        if (!stopped && !looping && engine.phase() === "ready") {
          looping = true;
          schedule();
        }
      } else {
        emit(scope, engine.post(JSON.stringify(data)));
        checkStopped();
      }
    });
  });

  return {
    get stopped() {
      return stopped;
    },
    frame,
  };
}
