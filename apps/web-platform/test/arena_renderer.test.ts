// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { ArenaClient, MemoryTransport, type FactionObservation } from "@rynvek/client";
import { createArenaApp } from "@rynvek/arena";
import type { ArenaServerMessage } from "@rynvek/client";
import type { WorkerLike } from "@rynvek/engine";
import { project, unproject } from "../src/arena-view.js";
import { mountArenaRenderer } from "../src/arena-renderer.js";

class WorkerStub implements WorkerLike {
  posted: unknown[] = []; terminated = false;
  listeners = new Map<string, Set<(event: {data?: unknown}) => void>>();
  postMessage(message: unknown) { this.posted.push(message); }
  addEventListener(type: string, listener: (event: {data?: unknown}) => void) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set()); this.listeners.get(type)!.add(listener);
  }
  removeEventListener(type: string, listener: (event: {data?: unknown}) => void) { this.listeners.get(type)?.delete(listener); }
  terminate() { this.terminated = true; }
  deliver(data: unknown) { for (const listener of [...this.listeners.get("message") ?? []]) listener({data}); }
  fail(type = "error") { for (const listener of [...this.listeners.get(type) ?? []]) listener({}); }
}
const ready = {type: "ENGINE_READY", backend: "webgpu", profile: "TierC", adapter: "测试适配器", generation: 0, arenaFaction: 0};
const view = {x: 32, y: 32, width: 640, height: 480, zoom: 1};
const fixture = JSON.parse(readFileSync(resolve(process.cwd(), "../../tests/engine/protocol_messages.json"), "utf8"));
const observation = fixture.commands.find((command: {type: string}) => command.type === "ARENA_OBSERVATION").observation as FactionObservation;
function setup() {
  vi.useFakeTimers();
  const canvas = document.createElement("canvas"); const transfer = vi.fn(() => ({}));
  Object.defineProperty(canvas, "transferControlToOffscreen", {value: transfer});
  const worker = new WorkerStub(), transport = new MemoryTransport();
  const client = new ArenaClient(transport, {principalId: "test:human", kind: "Human", faction: 0});
  const status = vi.fn(), error = vi.fn();
  const renderer = mountArenaRenderer(canvas, client, {worldId: "0".repeat(32), faction: 0, view, dpr: 2,
    createWorker: () => worker, onStatus: status, onError: error, readyTimeoutMs: 20});
  return {canvas, transfer, worker, transport, client, status, error, renderer};
}
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

it("页面挂载只移交一次画布，要求GPU并按物理像素设置同一世界相机，不抢2D上下文", () => {
  const get = vi.spyOn(HTMLCanvasElement.prototype, "getContext");
  const {canvas, transfer, worker, renderer} = setup();
  expect(get).not.toHaveBeenCalled(); expect(transfer).toHaveBeenCalledTimes(1);
  expect([canvas.width, canvas.height]).toEqual([1280, 960]);
  expect(worker.posted[0]).toMatchObject({type: "ENGINE_INIT", arenaFaction: 0, gpuMode: "required", width: 1280, height: 960});
  expect(worker.posted[1]).toEqual({type: "SET_CAMERA", centerX: 32, centerY: 32, pxPerCell: 20});
  renderer.dispose();
});

it("缩放/高DPI/resize只发协议，不修改已移交画布或运行仿真", () => {
  const {canvas, worker, renderer} = setup(); const count = worker.posted.length;
  renderer.sync(view, 2); expect(worker.posted).toHaveLength(count);
  renderer.sync({...view, x: 40, zoom: 2}, 3);
  expect(worker.posted.slice(-2)).toEqual([{type: "RESIZE", width: 1920, height: 1440, devicePixelRatio: 3},
    {type: "SET_CAMERA", centerX: 40, centerY: 32, pxPerCell: 60}]);
  expect(canvas.width).toBe(1280); expect(canvas.height).toBe(960);
  renderer.dispose();
});

it("仅服务器观察进入Worker；卸载后退订，重复dispose不复活", () => {
  const {worker, transport, renderer} = setup();
  transport.deliver({type: "observation", observation, events: []});
  expect(worker.posted.at(-1)).toEqual({type: "ARENA_OBSERVATION", worldId: "0".repeat(32), observation});
  renderer.dispose(); const count = worker.posted.length; renderer.dispose();
  transport.deliver({type: "observation", observation: {...observation, tick: 43}, events: []});
  expect(worker.posted).toHaveLength(count); expect(worker.terminated).toBe(true);
  expect(worker.posted.filter((m) => ["START", "STEP", "command"].includes((m as {type: string}).type))).toEqual([]);
});

it("就绪清除超时；GPU/设备错误可见并终止Worker，不静默降级2D", () => {
  const {worker, status, error} = setup(); worker.deliver(ready); vi.advanceTimersByTime(21);
  expect(status).toHaveBeenLastCalledWith("WebGPU Worker 已就绪"); expect(error).not.toHaveBeenCalled();
  worker.deliver({type: "BACKEND_DEGRADED", reason: "真实设备不可用"});
  expect(error.mock.calls[0]![0].message).toContain("真实设备不可用"); expect(worker.terminated).toBe(true);
});

it("下载或GPU初始化永不完成时显示超时并停止，不假报成功", () => {
  const {worker, status, error, renderer} = setup(); vi.advanceTimersByTime(21);
  expect(error.mock.calls[0]![0].message).toContain("超时"); expect(worker.terminated).toBe(true);
  worker.deliver(ready); renderer.sync(view, 2); expect(status).not.toHaveBeenCalledWith("WebGPU Worker 已就绪");
});

it.each(["error", "messageerror"])("模块Worker的%s立即可见，即使已READY且无剩余超时", (type) => {
  const {worker, error} = setup(); worker.deliver(ready); worker.fail(type);
  expect(error.mock.calls[0]![0].message).toContain("模块 Worker"); expect(worker.terminated).toBe(true);
});

it("缺少OffscreenCanvas在创建Worker前明确拒绝", () => {
  const createWorker = vi.fn();
  expect(() => mountArenaRenderer(document.createElement("canvas"), {on: () => () => {}, observation: null}, {
    worldId: "0".repeat(32), faction: 0, view, dpr: 1, createWorker, onStatus: vi.fn(), onError: vi.fn(),
  })).toThrow(/OffscreenCanvas/);
  expect(createWorker).not.toHaveBeenCalled();
});

it("GPU屏幕命中沿用Camera2D的Y向下坐标，不能把上方点击映射为下方命令", () => {
  const gpuView = {...view, yDown: true};
  expect(project(gpuView, 34, 35)).toEqual([340, 270]);
  expect(unproject(gpuView, 340, 270)).toEqual([34, 35]);
  expect(unproject(gpuView, 320, 210)).toEqual([32, 29]);
});
it("GPU选择叠层只传显示状态，去重且卸载后不再发送", () => {
  const {renderer, worker} = setup();
  renderer.select([5, 2, 5], null);
  expect(worker.posted.at(-1)).toEqual({type: "ARENA_SELECTION", worldId: "0".repeat(32), units: [2, 5], building: null});
  const count = worker.posted.length; renderer.select([2, 5], null); expect(worker.posted).toHaveLength(count);
  renderer.select([], 7); expect(worker.posted.at(-1)).toMatchObject({units: [], building: 7});
  renderer.dispose(); const done = worker.posted.length; renderer.select([5], null); expect(worker.posted).toHaveLength(done);
});

it("真实开局观察的GPU屏幕点选→选择环→地形采集命令复用租约fencing，Ack不改世界", async () => {
  const recording = JSON.parse(readFileSync(resolve(process.cwd(), "../../tests/arena/opening.transcript.json"), "utf8")) as {recv: {to: string; message: ArenaServerMessage}[]}[];
  const messages = recording.flatMap(x => x.recv).filter(x => x.to === "human:p0").map(x => x.message);
  const transport = new MemoryTransport();
  const app = createArenaApp(transport, {principalId: "human:p0", kind: "Human", faction: 0});
  transport.deliver(messages.find(m => m.type === "welcome")!);
  transport.deliver(messages.find(m => m.type === "observation")!);
  transport.deliver({type: "lease", seq: null, lease: {holder: {id: "human:p0", kind: "Human"}, faction: 0,
    level: "Sovereign", fencing_token: 77, issued_tick: 0, expires_tick: 10000}});
  const canvas = document.createElement("canvas"); Object.defineProperty(canvas, "transferControlToOffscreen", {value: () => ({})});
  const worker = new WorkerStub(); const error = vi.fn();
  const gpuView = {...view, x: 128, y: 128, zoom: .15, yDown: true};
  const renderer = mountArenaRenderer(canvas, app.client, {worldId: "0".repeat(32), faction: 0,
    view: gpuView, dpr: 2, createWorker: () => worker, onStatus: vi.fn(), onError: error});
  worker.deliver(ready);
  const obs = app.client.observation!, unit = obs.units.find(u => u.kind === "unit.forager")!;
  const world = [unit.position[0] / 4294967296, unit.position[1] / 4294967296] as const;
  const screen = project(gpuView, ...world); const hit = unproject(gpuView, ...screen);
  app.router.handlePointer({kind: "click", button: "left", x: hit[0], y: hit[1]});
  expect([...app.ui.selection]).toEqual([unit.id]);
  renderer.select([...app.ui.selection], app.ui.selectedBuilding);
  expect(worker.posted.at(-1)).toEqual({type: "ARENA_SELECTION", worldId: "0".repeat(32), units: [unit.id], building: null});
  const terrain = app.client.terrain!, marsh = terrain.tiles.findIndex(t => t === 2);
  expect(marsh).toBeGreaterThanOrEqual(0);
  const target = [marsh % terrain.width + .5, Math.floor(marsh / terrain.width) + .5] as const;
  const destination = unproject(gpuView, ...project(gpuView, ...target));
  const before = worker.posted.length;
  app.router.handlePointer({kind: "click", button: "right", x: destination[0], y: destination[1]});
  const sent = transport.sent.at(-1)!;
  expect(sent).toMatchObject({type: "command", fencing_token: 77, command: {type: "issue_order",
    units: {kind: "ids", value: [unit.id]}, order: "Harvest", target: target.map(x => x * 4294967296)}});
  if (sent.type !== "command") throw new Error("命令必须走原SDK");
  transport.deliver({type: "ack", seq: sent.seq, tick: obs.tick + 1, ack: {type: "order_issued", units: 1}});
  await app.controller.idle(); expect(app.client.observation).toBe(obs); expect(worker.posted).toHaveLength(before);
  transport.deliver({type: "lease_revoked", faction: 0, by: "other:human"});
  const count = transport.sent.length;
  app.router.handlePointer({kind: "click", button: "right", x: destination[0], y: destination[1]});
  await app.controller.idle(); expect(transport.sent).toHaveLength(count); expect(app.ui.notices.length).toBeGreaterThan(0);
  expect(worker.posted.some(m => ["START", "STEP", "command"].includes((m as {type: string}).type))).toBe(false);
  expect(error).not.toHaveBeenCalled(); renderer.dispose(); app.dispose();
});

it("GPU坐标系的W/方向上键必须向屏幕上方平移，不反转既有2D模式", () => {
  const app = createArenaApp(new MemoryTransport(), {principalId: "human:p0", kind: "Human", faction: 0});
  const key = {key: "ArrowUp", ctrlKey: false, altKey: false, metaKey: false, shiftKey: false};
  const initial = app.ui.camera.y; app.router.handleKey(key); expect(app.ui.camera.y).toBeGreaterThan(initial);
  app.ui.camera.y = 32; (app.ui.camera as {yDown?: boolean}).yDown = true;
  app.router.handleKey(key); expect(app.ui.camera.y).toBe(24); app.dispose();
});

it("超宽高DPI缩放的命中尺度与Rust Camera2D上限512一致", () => {
  const {worker, renderer} = setup();
  const wide = {...view, width: 8192, zoom: 4, yDown: true, pixelRatio: 3};
  renderer.sync(wide, 3);
  expect(worker.posted.at(-1)).toEqual({type: "SET_CAMERA", centerX: 32, centerY: 32, pxPerCell: 512});
  const screen = project(wide, 33, 33);
  expect((screen[0] - wide.width / 2) * 3).toBeCloseTo(512);
  expect(unproject(wide, ...screen)[1]).toBeCloseTo(33); renderer.dispose();
});
