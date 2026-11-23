// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import { EngineHost, EngineHostError, type EngineEvent } from "../src/index.js";
import { FakeCanvas, FakeOffscreen, FakeTimers, FakeWorker, READY_GPU, ROOT } from "./fakes.js";

const INIT = { width: 800, height: 600, devicePixelRatio: 2, gpuMode: "auto" } as const;

function attached(opts: { timers?: FakeTimers } = {}) {
  const worker = new FakeWorker();
  const timers = opts.timers ?? new FakeTimers();
  const host = new EngineHost(worker, { timers, pickTimeoutMs: 100 });
  const canvas = new FakeCanvas();
  host.attach(canvas, INIT);
  return { worker, timers, host, canvas };
}

describe("OffscreenCanvas 一次性移交", () => {
  it("ENGINE_INIT 携带 OffscreenCanvas 并把它放进 transfer 列表", () => {
    const { worker, host, canvas } = attached();
    expect(host.state).toBe("initializing");
    expect(canvas.transfers).toBe(1);
    const [first] = worker.posted;
    const msg = first!.message as Record<string, unknown>;
    expect(msg["type"]).toBe("ENGINE_INIT");
    expect(msg["canvas"]).toBeInstanceOf(FakeOffscreen);
    expect(first!.transfer).toEqual([msg["canvas"]]);
    expect(msg).toMatchObject({ width: 800, height: 600, devicePixelRatio: 2, worldId: null, gpuMode: "auto" });
  });

  it("同一宿主第二次 attach 被拒绝且不再调用 transferControlToOffscreen", () => {
    const { worker, host, canvas } = attached();
    const other = new FakeCanvas();
    expect(() => host.attach(other, INIT)).toThrowError(EngineHostError);
    expect(() => host.attach(canvas, INIT)).toThrow(/只允许一次/);
    expect(canvas.transfers).toBe(1);
    expect(other.transfers).toBe(0);
    expect(worker.posted).toHaveLength(1);
  });

  it("同一画布不能移交给第二个宿主", () => {
    const { canvas } = attached();
    const second = new EngineHost(new FakeWorker(), { timers: new FakeTimers() });
    try {
      second.attach(canvas, INIT);
      expect.unreachable();
    } catch (e) {
      expect((e as EngineHostError).code).toBe("CANVAS_ALREADY_TRANSFERRED");
    }
    expect(canvas.transfers).toBe(1);
    expect(second.state).toBe("idle");
  });

  it("非法尺寸在移交之前就被拒绝，画布保持可用", () => {
    const host = new EngineHost(new FakeWorker(), { timers: new FakeTimers() });
    const canvas = new FakeCanvas();
    expect(() => host.attach(canvas, { ...INIT, width: 0 })).toThrow(/尺寸非法/);
    expect(canvas.transfers).toBe(0);
    host.attach(canvas, INIT);
    expect(host.state).toBe("initializing");
  });

  it("attach 之前发送命令报 NOT_ATTACHED", () => {
    const host = new EngineHost(new FakeWorker(), { timers: new FakeTimers() });
    expect(() => host.step()).toThrow(expect.objectContaining({ code: "NOT_ATTACHED" }));
  });
});

describe("宿主状态机", () => {
  it("就绪 → DeviceLost 恢复 → 再次就绪，代数随 RECOVERED 更新", () => {
    const { worker, host } = attached();
    const seen: string[] = [];
    host.on("*", (e) => seen.push(e.type));
    worker.deliver([READY_GPU, { type: "TICK_COMMITTED", tick: 5, stateRoot: ROOT }]);
    expect(host.state).toBe("ready");
    expect(host.backend).toBe("webgpu");
    expect(host.lastTick).toBe(5);
    expect(host.lastStateRoot).toBe(ROOT);

    worker.deliver({ type: "DEVICE_LOST", tick: 6, reason: "Destroyed" });
    expect(host.state).toBe("recovering");
    for (let step = 1; step <= 5; step++) worker.deliver({ type: "RECOVERY_PROGRESS", step, phase: "x" });
    worker.deliver({ type: "RECOVERED", generation: 1, resumeTick: 6 });
    expect(host.state).toBe("ready");
    expect(host.generation).toBe(1);
    expect(seen.filter((t) => t === "RECOVERY_PROGRESS")).toHaveLength(5);
  });

  it("恢复失败降级为 CPU Reference 后仍可发送命令", () => {
    const { worker, host } = attached();
    worker.deliver(READY_GPU);
    worker.deliver({ type: "DEVICE_LOST", tick: 1, reason: "Unknown" });
    worker.deliver({ type: "BACKEND_DEGRADED", reason: "重获设备连续失败" });
    expect(host.state).toBe("degraded");
    expect(host.backend).toBe("cpu-reference");
    host.step(2);
    expect(worker.messages().at(-1)).toEqual({ type: "STEP", ticks: 2 });
  });

  it("初始化阶段的 ENGINE_ERROR（gpuMode=required 无适配器）使宿主失败并拒绝后续命令", async () => {
    const { worker, host } = attached();
    const pick = host.pick(1, 1);
    worker.deliver({ type: "ENGINE_ERROR", code: 0x6001, message: "无适配器" });
    expect(host.state).toBe("failed");
    expect(host.lastError?.code).toBe(0x6001);
    await expect(pick).rejects.toMatchObject({ code: "HOST_CLOSED" });
    expect(() => host.step()).toThrow(expect.objectContaining({ code: "HOST_CLOSED" }));
  });

  it("就绪后的 ENGINE_ERROR 只上报不改变状态", () => {
    const { worker, host } = attached();
    worker.deliver(READY_GPU);
    worker.deliver({ type: "ENGINE_ERROR", code: 0x1004, message: "未知场通道" });
    expect(host.state).toBe("ready");
    expect(host.lastError?.code).toBe(0x1004);
  });

  it("Worker 脚本错误使宿主失败", () => {
    const { worker, host } = attached();
    worker.raiseError();
    expect(host.state).toBe("failed");
  });

  it("非法 Worker 消息被计数丢弃，不触发监听器", () => {
    const { worker, host } = attached();
    const seen: EngineEvent[] = [];
    host.on("*", (e) => seen.push(e));
    worker.deliver([{ type: "TICK_COMMITTED", tick: 1, stateRoot: "bad" }, "garbage", READY_GPU]);
    expect(host.rejectedMessages).toBe(2);
    expect(seen).toEqual([READY_GPU]);
  });

  it("按类型订阅与取消订阅", () => {
    const { worker, host } = attached();
    const ticks: number[] = [];
    const off = host.on("TICK_COMMITTED", (e) => ticks.push(e.tick));
    worker.deliver({ type: "TICK_COMMITTED", tick: 1, stateRoot: ROOT });
    off();
    worker.deliver({ type: "TICK_COMMITTED", tick: 2, stateRoot: ROOT });
    expect(ticks).toEqual([1]);
  });

  it("shutdown 发送 SHUTDOWN、终止 Worker、拒绝未完成拾取且幂等", async () => {
    const { worker, host, timers } = attached();
    worker.deliver(READY_GPU);
    const p = host.pick(3, 4);
    host.shutdown();
    host.shutdown();
    expect(host.state).toBe("shutdown");
    expect(worker.terminated).toBe(true);
    expect(worker.messages().filter((m) => m.type === "SHUTDOWN")).toHaveLength(1);
    await expect(p).rejects.toMatchObject({ code: "HOST_CLOSED" });
    expect(timers.size).toBe(0);
    // 关闭后的迟到事件被忽略
    worker.deliver({ type: "TICK_COMMITTED", tick: 99, stateRoot: ROOT });
    expect(host.lastTick).toBe(0);
  });
});

describe("拾取请求关联", () => {
  it("乱序回复按 requestId 各自解析", async () => {
    const { worker, host } = attached();
    worker.deliver(READY_GPU);
    const a = host.pick(10.7, 20.2);
    const b = host.pick(30, 40);
    const reqs = worker.messages().filter((m) => m.type === "PICK_REQUEST") as unknown as {
      requestId: number;
      x: number;
      y: number;
    }[];
    expect(reqs.map((r) => [r.x, r.y])).toEqual([
      [10, 20],
      [30, 40],
    ]);
    const [ra, rb] = reqs;
    expect(ra!.requestId).not.toBe(rb!.requestId);
    worker.deliver({ type: "PICK_RESULT", requestId: rb!.requestId, entitySerial: null });
    worker.deliver({ type: "PICK_RESULT", requestId: ra!.requestId, entitySerial: 17 });
    await expect(a).resolves.toBe(17);
    await expect(b).resolves.toBeNull();
    expect(host.pendingPicks).toBe(0);
  });

  it("超时拒绝，迟到的回复被忽略", async () => {
    const timers = new FakeTimers();
    const { worker, host } = attached({ timers });
    const p = host.pick(1, 1);
    timers.advance(100);
    await expect(p).rejects.toMatchObject({ code: "PICK_TIMEOUT" });
    worker.deliver({ type: "PICK_RESULT", requestId: 1, entitySerial: 5 });
    expect(host.pendingPicks).toBe(0);
  });

  it("非法坐标直接拒绝且不发送", async () => {
    const { worker, host } = attached();
    await expect(host.pick(-1, 0)).rejects.toMatchObject({ code: "INVALID_ARGUMENT" });
    await expect(host.pick(Number.NaN, 0)).rejects.toMatchObject({ code: "INVALID_ARGUMENT" });
    expect(worker.messages().some((m) => m.type === "PICK_REQUEST")).toBe(false);
  });
});

describe("通用 World 明确安装", () => {
  it("只发送明确提供的原包字节，调用后修改输入不改变消息", () => {
    const { host, worker } = attached();
    const bytes = new Uint8Array([82, 89, 87, 76]);
    host.loadGeneralWorld(bytes); bytes[0] = 0;
    expect(worker.posted.at(-1)?.message).toEqual({ type: "GENERAL_WORLD", packageBytes: [82, 89, 87, 76] });
  });
  it("空包和超限包在宿主就拒绝，不发送占位命令", () => {
    const { host, worker } = attached();
    expect(() => host.loadGeneralWorld(new Uint8Array())).toThrow(/通用 World/);
    expect(() => host.loadGeneralWorld(new Uint8Array(128 * 1024 + 1))).toThrow(/通用 World/);
    expect(worker.posted).toHaveLength(1);
  });
});
