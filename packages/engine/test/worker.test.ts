// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import { type WasmEngineWorkerLike, type WorkerScopeLike, runEngineWorker, bootstrapEngineWorker } from "../src/index.js";
import { FakeOffscreen, READY_GPU, ROOT } from "./fakes.js";

/** 模拟 rynvek-wasm 的 EngineWorker：记录调用，初始化可延迟完成 */
class FakeWasmWorker implements WasmEngineWorkerLike {
  calls: string[] = [];
  initCanvas: unknown = undefined;
  state = "awaiting-init";
  tick = 0;
  readyEvents: unknown[] = [READY_GPU];
  private release: (() => void) | null = null;

  init(canvas: unknown, json: string): Promise<unknown> {
    this.initCanvas = canvas;
    this.calls.push(`init:${json}`);
    this.state = "initializing";
    return new Promise((resolve) => {
      this.release = () => {
        this.state = "ready";
        resolve(JSON.stringify(this.readyEvents));
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


it.each([
  ["旧 WASM 未回显", [READY_GPU]],
  ["错误阵营", [{ ...READY_GPU, arenaFaction: 1 }]],
  ["空阵营", [{ ...READY_GPU, arenaFaction: null }]],
  ["字符串阵营", [{ ...READY_GPU, arenaFaction: "0" }]],
  ["负阵营", [{ ...READY_GPU, arenaFaction: -1 }]],
  ["越界阵营", [{ ...READY_GPU, arenaFaction: 256 }]],
  ["小数阵营", [{ ...READY_GPU, arenaFaction: 0.5 }]],
  ["CPU 冒充显示成功", [{ ...READY_GPU, arenaFaction: 0, backend: "cpu-reference" }]],
  ["没有就绪事件", []],
  ["重复就绪事件", [{ ...READY_GPU, arenaFaction: 0 }, { ...READY_GPU, arenaFaction: 0 }]],
])("竞技场能力握手失败必须先于就绪、帧与观察转发：%s", async (_label, events) => {
  const scope = new FakeScope();
  const wasm = new FakeWasmWorker();
  wasm.readyEvents = events;
  const loop = runEngineWorker(scope, wasm);
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(5), width: 4, height: 4,
    gpuMode: "required", arenaFaction: 0 });
  scope.send({ type: "ARENA_OBSERVATION", worldId: "0".repeat(32), observation: {} });
  await flush(); wasm.finishInit(); await flush();
  expect(loop.stopped).toBe(true);
  expect(scope.raf).toHaveLength(0);
  expect(scope.out).toEqual([[{ type: "ENGINE_ERROR", code: 0x3003,
    message: expect.stringContaining("阵营只读显示能力") }]]);
  scope.runFrame(16); loop.frame(32);
  expect(wasm.calls.filter((call) => call.startsWith("post:") || call.startsWith("frame:"))).toEqual([]);
});

it.each([0, 255])("成功回显阵营 %s 后才发布就绪、转发观察和调度帧", async (arenaFaction) => {
  const scope = new FakeScope();
  const wasm = new FakeWasmWorker();
  const ready = { ...READY_GPU, arenaFaction };
  wasm.readyEvents = [ready];
  const loop = runEngineWorker(scope, wasm);
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(6), width: 4, height: 4,
    gpuMode: "required", arenaFaction });
  scope.send({ type: "ARENA_OBSERVATION", worldId: "0".repeat(32), observation: { faction: arenaFaction } });
  await flush(); wasm.finishInit(); await flush();
  expect(loop.stopped).toBe(false);
  expect(scope.out).toEqual([[ready]]);
  expect(scope.raf).toHaveLength(1);
  expect(wasm.calls.at(-1)).toContain('post:{"type":"ARENA_OBSERVATION"');
});

it("竞技场设备初始化真实失败时保留原错误，不用握手错误覆盖它", async () => {
  const scope = new FakeScope();
  const wasm = new FakeWasmWorker();
  const error = { type: "ENGINE_ERROR", code: 0x6001, message: "没有可用 GPU" };
  wasm.init = async () => { wasm.state = "shut-down"; return JSON.stringify([error]); };
  const loop = runEngineWorker(scope, wasm);
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(7), width: 4, height: 4,
    gpuMode: "required", arenaFaction: 0 });
  await flush();
  expect(loop.stopped).toBe(true);
  expect(scope.out).toEqual([[error]]);
  expect(scope.raf).toHaveLength(0);
});


it("初始化未完成的 WASM 即使伪发正确能力也不能发布就绪", async () => {
  const scope = new FakeScope();
  const wasm = new FakeWasmWorker();
  wasm.init = async () => JSON.stringify([{ ...READY_GPU, arenaFaction: 0 }]);
  const loop = runEngineWorker(scope, wasm);
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(8), width: 4, height: 4,
    gpuMode: "required", arenaFaction: 0 });
  await flush();
  expect(loop.stopped).toBe(true);
  expect(scope.raf).toHaveLength(0);
  expect(scope.out).toEqual([[{ type: "ENGINE_ERROR", code: 0x3003,
    message: expect.stringContaining("阵营只读显示能力") }]]);
});


it("真实模块入口先监听再加载WASM，加载期间排队观察而非丢失", async () => {
  const scope = new FakeScope(); const wasm = new FakeWasmWorker();
  wasm.readyEvents = [{ ...READY_GPU, arenaFaction: 0 }];
  let release!: (engine: WasmEngineWorkerLike) => void;
  const loop = bootstrapEngineWorker(scope, () => new Promise((resolve) => { release = resolve; }));
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(9), width: 4, height: 4,
    gpuMode: "required", arenaFaction: 0 });
  scope.send({ type: "ARENA_OBSERVATION", worldId: "0".repeat(32), observation: { faction: 0 } });
  await flush(); expect(wasm.calls).toEqual([]);
  release(wasm); await flush(); wasm.finishInit(); await flush();
  expect(loop.stopped).toBe(false); expect(scope.raf).toHaveLength(1);
  expect(wasm.calls[0]).toContain("init:"); expect(wasm.calls[1]).toContain('post:{"type":"ARENA_OBSERVATION"');
});

it("WASM下载/实例化失败显式回报一次，停止队列且不调度空白假成功帧", async () => {
  const scope = new FakeScope();
  const loop = bootstrapEngineWorker(scope, async () => { throw new Error("真实WASM加载失败"); });
  scope.send({ type: "ENGINE_INIT", canvas: new FakeOffscreen(10), width: 4, height: 4,
    gpuMode: "required", arenaFaction: 0 });
  scope.send({ type: "ARENA_OBSERVATION" });
  await flush();
  expect(loop.stopped).toBe(true); expect(scope.raf).toHaveLength(0);
  expect(scope.out).toEqual([[{ type: "ENGINE_ERROR", code: 0x3003, message: "Worker 调用失败：真实WASM加载失败" }]]);
});
