// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import type { EngineEvent, TimerApi, TransferableCanvasLike, WorkerLike } from "../src/index.js";

/** 模拟 OffscreenCanvas（仅作为可转移对象的标识） */
export class FakeOffscreen {
  constructor(readonly id: number) {}
}

/** 模拟 HTMLCanvasElement：第二次 transferControlToOffscreen 与浏览器一样抛 InvalidStateError */
export class FakeCanvas implements TransferableCanvasLike<FakeOffscreen> {
  static nextId = 1;
  transfers = 0;
  transferControlToOffscreen(): FakeOffscreen {
    this.transfers += 1;
    if (this.transfers > 1) throw new Error("InvalidStateError: 画布控制权已移交");
    return new FakeOffscreen(FakeCanvas.nextId++);
  }
}

type L = (ev: { data?: unknown }) => void;

/** 模拟 Worker：记录 postMessage，可向主线程回送事件 */
export class FakeWorker implements WorkerLike {
  readonly posted: { message: unknown; transfer: unknown[] | undefined }[] = [];
  readonly listeners = new Map<string, Set<L>>();
  terminated = false;

  postMessage(message: unknown, transfer?: unknown[]): void {
    this.posted.push({ message, transfer });
  }
  addEventListener(type: string, l: L): void {
    let s = this.listeners.get(type);
    if (!s) this.listeners.set(type, (s = new Set()));
    s.add(l);
  }
  removeEventListener(type: string, l: L): void {
    this.listeners.get(type)?.delete(l);
  }
  terminate(): void {
    this.terminated = true;
  }
  /** Worker → 主线程 */
  deliver(data: EngineEvent | EngineEvent[] | unknown): void {
    for (const l of this.listeners.get("message") ?? []) l({ data });
  }
  raiseError(): void {
    for (const l of this.listeners.get("error") ?? []) l({});
  }
  messages(): { type: string }[] {
    return this.posted.map((p) => p.message as { type: string });
  }
}

/** 手动推进的计时器 */
export class FakeTimers implements TimerApi {
  now = 0;
  private seq = 0;
  private readonly tasks = new Map<number, { at: number; fn: () => void }>();
  setTimeout(fn: () => void, ms: number): unknown {
    const id = ++this.seq;
    this.tasks.set(id, { at: this.now + ms, fn });
    return id;
  }
  clearTimeout(handle: unknown): void {
    this.tasks.delete(handle as number);
  }
  advance(ms: number): void {
    this.now += ms;
    for (const [id, t] of [...this.tasks]) {
      if (t.at <= this.now) {
        this.tasks.delete(id);
        t.fn();
      }
    }
  }
  get size(): number {
    return this.tasks.size;
  }
}

export const READY_GPU: EngineEvent = {
  type: "ENGINE_READY",
  backend: "webgpu",
  profile: "TierC",
  adapter: "llvmpipe (Gl)",
  generation: 0,
};

export const ROOT = "ab".repeat(32);
