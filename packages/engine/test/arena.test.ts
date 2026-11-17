// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { readFileSync } from "node:fs";
import { ArenaClient, MemoryTransport, type FactionObservation, type ArenaServerMessage } from "@rynvek/client";
import { describe, expect, it } from "vitest";
import { createArenaRenderHost } from "../src/index.js";
import { FakeCanvas, FakeWorker } from "./fakes.js";

const fixtures = JSON.parse(readFileSync(new URL("../../../tests/engine/protocol_messages.json", import.meta.url), "utf8")) as {
  commands: { type: string; observation?: FactionObservation }[];
};
const observation = fixtures.commands.find((c) => c.type === "ARENA_OBSERVATION")!.observation!;
const INIT = { width: 256, height: 256, devicePixelRatio: 1, gpuMode: "required", worldId: "0".repeat(32), arenaFaction: 0 } as const;
function setup() {
  const transport = new MemoryTransport();
  const client = new ArenaClient(transport, { principalId: "test:human", kind: "Human", faction: 0 });
  const worker = new FakeWorker(); const canvas = new FakeCanvas(); const errors: Error[] = [];
  const renderer = createArenaRenderHost(worker, canvas, client, { ...INIT, onError: (e) => errors.push(e) });
  return { transport, client, worker, canvas, errors, renderer };
}

describe("真实客户端观察到只读 Worker 桥", () => {
  it("初始化绑定阵营并只转发服务器观察，不生成演示状态", () => {
    const {transport, worker, canvas, renderer} = setup();
    expect(canvas.transfers).toBe(1);
    expect(worker.posted[0]!.message).toMatchObject({ type: "ENGINE_INIT", arenaFaction: 0, gpuMode: "required" });
    expect(worker.messages()).toHaveLength(1);
    transport.deliver({ type: "observation", observation, events: [] });
    expect(worker.posted[1]!.message).toEqual({ type: "ARENA_OBSERVATION", worldId: INIT.worldId, observation });
    renderer.dispose();
  });
  it("操作继续走正式命令/租约，Ack 到达也不会乐观修改渲染观察", async () => {
    const {transport, client, worker, renderer} = setup();
    transport.deliver({ type: "observation", observation, events: [] });
    transport.deliver({ type: "lease", seq: null, lease: { holder: { id: "test:human", kind: "Human" },
      faction: 0, level: "Sovereign", fencing_token: 7, issued_tick: 0, expires_tick: 10000 } });
    const command = { type: "issue_order", units: { kind: "ids", value: [5] }, order: "Move", target: [4294967296, 4294967296] } as const;
    const ack = client.command(command);
    const sent = transport.sent.at(-1)!;
    expect(sent).toMatchObject({ type: "command", fencing_token: 7, command });
    expect(worker.messages()).toHaveLength(2);
    if (sent.type !== "command") throw new Error("必须使用权威命令接口");
    transport.deliver({ type: "ack", seq: sent.seq, tick: 43, ack: { type: "order_issued", units: 1 } });
    await expect(ack).resolves.toEqual({ type: "order_issued", units: 1 });
    expect(worker.messages()).toHaveLength(2);
    transport.deliver({ type: "observation", observation: { ...observation, tick: 43 }, events: [] });
    expect(worker.messages()).toHaveLength(3);
    expect(worker.messages().every((m) => m.type !== "STEP" && m.type !== "START" && m.type !== "command")).toBe(true);
    renderer.dispose(); client.dispose();
  });
  it("其他阵营观察不转发；卸载退订并关闭 Worker，不因后续观察复活", () => {
    const {transport, worker, errors, renderer} = setup();
    transport.deliver({ type: "observation", observation: { ...observation, faction: 1 }, events: [] });
    expect(worker.messages()).toHaveLength(1); expect(errors[0]!.message).toContain("其他阵营");
    renderer.dispose(); renderer.dispose();
    expect(worker.terminated).toBe(true);
    transport.deliver({ type: "observation", observation, events: [] });
    expect(worker.messages().map((m) => m.type)).toEqual(["ENGINE_INIT", "SHUTDOWN"]);
  });
  it("Worker 错误可见，GPU 不可用不冒充成功", () => {
    const {worker, errors, renderer} = setup();
    worker.deliver({ type: "ENGINE_ERROR", code: 0x6001, message: "缺少 GPU" });
    expect(errors[0]!.message).toBe("缺少 GPU");
    renderer.dispose();
    const canvas = new FakeCanvas();
    expect(() => createArenaRenderHost(new FakeWorker(), canvas,
      { observation: null, on: () => () => {} }, { ...INIT, gpuMode: "disabled", onError: () => {} })).toThrow(/要求 GPU/);
    expect(canvas.transfers).toBe(0);
  });
});

it("真实录制welcome经SDK解码后只转发一次地图，顺序先于观察", () => {
  const recording = JSON.parse(readFileSync(new URL("../../../tests/arena/opening.transcript.json", import.meta.url), "utf8")) as {recv: {to: string; message: ArenaServerMessage}[]}[];
  const messages = recording.flatMap(x => x.recv).filter(x => x.to === "human:p0").map(x => x.message);
  const welcome = messages.find(m => m.type === "welcome")!;
  const observed = messages.find(m => m.type === "observation")!;
  const {transport, client, worker, renderer} = setup();
  transport.deliver(welcome); transport.deliver(observed);
  const map = worker.messages().find(m => m.type === "ARENA_MAP");
  expect(map).toEqual({type: "ARENA_MAP", worldId: INIT.worldId, map: {width: client.terrain!.width,
    height: client.terrain!.height, tiles: Array.from(client.terrain!.tiles), tilesHash: client.welcome!.map.tiles_hash}});
  expect(worker.messages().slice(1).map(m => m.type)).toEqual(["ARENA_MAP", "ARENA_OBSERVATION"]);
  transport.deliver(observed);
  expect(worker.messages().filter(m => m.type === "ARENA_MAP")).toHaveLength(1);
  renderer.dispose(); client.dispose();
});
