// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import { type WasmEngineWorkerLike, type WorkerScopeLike, runEngineWorker } from "../src/index.js";
import { FakeOffscreen, READY_GPU, ROOT } from "./fakes.js";

/** 模拟 rynvek-wasm 的 EngineWorker：记录调用，初始化可延迟完成 */
class FakeWasmWorker implements WasmEngineWorkerLike {
  calls: string[] = [];
  initCanvas: unknown = undefined;
  state = "awaiting-init";
  tick = 0;
  private release: (() => void) | null = null;

  init(canvas: unknown, json: string): Promise<unknown> {
    this.initCanvas = canvas;
    this.calls.push(`init:${json}`);
    this.state = "initializing";
    return new Promise((resolve) => {
      this.release = () => {
        this.state = "ready";
        resolve(JSON.stringify([READY_GPU]));
      };
    });
  }
  finishInit(): void {
    this.release?.();
  }
  post(json: string): string {
    this.calls.push(`post:${json}`);
    const msg = JSON.parse(json) as { type: string };
    if (msg.type === "SHUTDOWN") this.state = "shut-down";
    if (msg.type === "STEP") {
      this.tick += 1;
      return JSON.stringify([{ type: "TICK_COMMITTED", tick: this.tick, stateRoot: ROOT }]);
    }
    return "[]";
  }
  frame(now: number): string {
    this.calls.push(`frame:${now}`);
    return JSON.stringify([{ type: "FRAME_RENDERED", tick: this.tick }]);
  }
  phase(): string {
    return this.state;
  }
}

class FakeScope implements WorkerScopeLike {
  out: unknown[] = [];
  raf: ((t: number) => void)[] = [];
  private listener: ((ev: { data?: unknown }) => void) | null = null;
  postMessage(m: unknown): void {
    this.out.push(m);
  }
  addEventListener(_t: "message", l: (ev: { data?: unknown }) => void): void {
    this.listener = l;
  }
  requestAnimationFrame(cb: (t: number) => void): number {
    this.raf.push(cb);
    return this.raf.length;
  }
  send(data: unknown): void {
    this.listener?.({ data });
  }
  runFrame(t: number): void {
    const cbs = this.raf.splice(0);
    for (const cb of cbs) cb(t);
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("Worker 侧胶水", () => {
  it("ENGINE_INIT 剥离 canvas 单独传给 init，初始化期间的消息排在其后按序处理", async () => {
    const scope = new FakeScope();
    const wasm = new FakeWasmWorker();
    runEngineWorker(scope, wasm);
    const canvas = new FakeOffscreen(1);
    scope.send({ type: "ENGINE_INIT", canvas, width: 4, height: 4, devicePixelRatio: 1, worldId: null, gpuMode: "auto" });
    scope.send({ type: "STEP", ticks: 1 });
    await flush();
    expect(wasm.initCanvas).toBe(canvas);
    expect(wasm.calls).toEqual([
      'init:{"type":"ENGINE_INIT","width":4,"height":4,"devicePixelRatio":1,"worldId":null,"gpuMode":"auto"}',
    ]);
    wasm.finishInit();
    await flush();
    expect(wasm.calls.slice(1)).toEqual(['post:{"type":"STEP","ticks":1}']);
    expect(scope.out).toEqual([[READY_GPU], [{ type: "TICK_COMMITTED", tick: 1, stateRoot: ROOT }]]);
  });

  it("就绪后以 requestAnimationFrame 驱动帧回调，SHUTDOWN 后停止", async () => {
    const scope = new FakeScope();
    const wasm = new FakeWasmWorker();
    const loop = runEngineWorker(scope, wasm);
    expect(scope.raf).toHaveLength(0);
    scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(2), width: 4, height: 4, devicePixelRatio: 1, gpuMode: "auto" });
    await flush();
    wasm.finishInit();
    await flush();
    expect(scope.raf).toHaveLength(1);
    scope.runFrame(16);
    scope.runFrame(32);
    expect(wasm.calls.filter((c) => c.startsWith("frame:"))).toEqual(["frame:16", "frame:32"]);
    expect(scope.out.at(-1)).toEqual([{ type: "FRAME_RENDERED", tick: 0 }]);

    scope.send({ type: "SHUTDOWN" });
    await flush();
    expect(loop.stopped).toBe(true);
    scope.runFrame(48);
    expect(scope.raf).toHaveLength(0);
    expect(wasm.calls.filter((c) => c.startsWith("frame:"))).toHaveLength(2);
    // 停止后到达的消息不再转交
    scope.send({ type: "STEP", ticks: 1 });
    await flush();
    expect(wasm.calls.filter((c) => c.includes(`"STEP"`))).toHaveLength(0);
  });

  it("引擎返回的非法事件不会转发给主线程", async () => {
    const scope = new FakeScope();
    const wasm = new FakeWasmWorker();
    wasm.post = () => '[{"type":"BOGUS"}]';
    runEngineWorker(scope, wasm);
    scope.send({ type: "PAUSE" });
    await flush();
    expect(scope.out).toEqual([]);
  });
});

it("WASM 初始化拒绝时报告错误并停止，不留下永久拒绝的静默消息队列", async () => {
  const scope = new FakeScope();
  const wasm = new FakeWasmWorker();
  wasm.init = async () => { throw new Error("初始化失败反例"); };
  const loop = runEngineWorker(scope, wasm);
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(3), width: 4, height: 4, gpuMode: "required" });
  await flush();
  expect(loop.stopped).toBe(true);
  expect(scope.out).toEqual([[{ type: "ENGINE_ERROR", code: 0x3003, message: "Worker 调用失败：初始化失败反例" }]]);
  scope.send({ type: "STEP", ticks: 1 });
  await flush();
  expect(wasm.calls).toHaveLength(0);
});

it("WASM 帧异常只报告一次并停止重调度", async () => {
  const scope = new FakeScope();
  const wasm = new FakeWasmWorker();
  const loop = runEngineWorker(scope, wasm);
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(4), width: 4, height: 4, gpuMode: "required" });
  await flush(); wasm.finishInit(); await flush();
  wasm.frame = () => { throw new Error("帧失败反例"); };
  scope.runFrame(16);
  expect(loop.stopped).toBe(true);
  expect(scope.raf).toHaveLength(0);
  loop.frame(32);
  expect(scope.out).toEqual([[READY_GPU], [{ type: "ENGINE_ERROR", code: 0x3003, message: "Worker 调用失败：帧失败反例" }]]);
});
