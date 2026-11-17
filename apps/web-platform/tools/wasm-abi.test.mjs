// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import test from "node:test";
import init, { EngineWorker } from "../src/generated/wasm/rynvek_wasm.js";
import { buildWasm } from "./build-wasm.mjs";

const module = new WebAssembly.Module(readFileSync(new URL("../src/generated/wasm/rynvek_wasm_bg.wasm", import.meta.url)));
await init({ module_or_path: module });
const fixture = JSON.parse(readFileSync(new URL("../../../tests/engine/protocol_messages.json", import.meta.url), "utf8"));

test("真实产物仍是浏览器unknown-unknown目标，不引入WASI运行时替代", () => {
  assert.equal(WebAssembly.Module.imports(module).filter((entry) => entry.module.startsWith("wasi")).length, 0);
});

test("真实生成的浏览器WASM ABI：阵营握手、观察Tick、禁本地仿真及终止", async () => {
  const worker = new EngineWorker();
  try {
    assert.equal(worker.phase(), "awaiting-init");
    // Node 无画布/GPU：只验证 ABI 与只读逻辑，不把 disabled 当成显示或浏览器通过。
    const ready = JSON.parse(await worker.init(null, JSON.stringify({ type: "ENGINE_INIT", width: 256, height: 256,
      gpuMode: "disabled", worldId: fixture.commands[0].worldId, arenaFaction: 0 })));
    assert.equal(ready[0].arenaFaction, 0); assert.equal(ready[0].backend, "cpu-reference");
    const observation = fixture.commands.find((c) => c.type === "ARENA_OBSERVATION");
    assert.ok(JSON.parse(worker.post(JSON.stringify({ ...observation, worldId: "0".repeat(32) })))
      .some((event) => event.type === "ENGINE_ERROR"));
    assert.deepEqual(JSON.parse(worker.post(JSON.stringify(observation))), []);
    const frame = JSON.parse(worker.frame(1000));
    assert.deepEqual(frame, [], "无GPU时不得伪造FRAME_RENDERED");
    assert.deepEqual(JSON.parse(worker.post(JSON.stringify(observation))), [], "同Tick观察仍可接受，真实状态没有被帧循环推进");
    assert.ok(JSON.parse(worker.post(JSON.stringify({ ...observation, observation: { ...observation.observation, tick: 41 } })))
      .some((event) => event.type === "ENGINE_ERROR"), "旧Tick必须拒绝，证明真实观察已应用");
    for (const command of [{ type: "START", tickRate: 30 }, { type: "STEP", ticks: 1 }]) {
      assert.ok(JSON.parse(worker.post(JSON.stringify(command))).some((event) => event.type === "ENGINE_ERROR"));
    }
    assert.deepEqual(JSON.parse(worker.frame(2000)), []);
    assert.deepEqual(JSON.parse(worker.post(JSON.stringify(observation))), [], "START/STEP拒绝后依然停在服务器Tick42");
    worker.post('{"type":"SHUTDOWN"}'); assert.equal(worker.phase(), "shut-down");
    worker.post(JSON.stringify(observation)); assert.equal(worker.phase(), "shut-down");
  } finally { worker.free(); }
});

test("构建前拒绝旧CLI，不能悄悄沿用旧WASM", () => {
  let calls = 0;
  assert.throws(() => buildWasm({ run: () => { calls++; return { status: 0, stdout: "wasm-bindgen 0.2.1" }; } }), /同版/);
  assert.equal(calls, 1);
});

test("实际cargo失败不得调用胶水生成或宣称成功", () => {
  const calls = [];
  assert.throws(() => buildWasm({ run: (command) => { calls.push(command);
    return calls.length === 1 ? { status: 0, stdout: "wasm-bindgen 0.2.130" } : { status: 101 }; } }), /失败/);
  assert.deepEqual(calls, [process.env.WASM_BINDGEN || "wasm-bindgen", "cargo"]);
});
