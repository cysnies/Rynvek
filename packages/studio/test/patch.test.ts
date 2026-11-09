// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { describe, expect, it } from "vitest";
import {
  HLC_MAX_DRIFT_MS,
  HybridLogicalClock,
  LiveEditSafetyClass,
  SAFETY_CLASS_NAMES,
  TerrainTileKind,
  WORLD_PATCH_OP_KINDS,
  compareHlc,
  parseWorldPatch,
  serializeWorldPatch,
} from "../src/index.js";

const SPEC = readFileSync(new URL("../../../docs/Rynvek_Development_Specification_v1.0.md", import.meta.url), "utf8");
const FIXTURE = readFileSync(new URL("./fixtures/world_patch.json", import.meta.url), "utf8").trim();

/** 规范代码中从 `start` 开始到对应 `}` 结束的条目 */
function specBlock(start: string): string {
  const i = SPEC.indexOf(start);
  expect(i).toBeGreaterThan(0);
  return SPEC.slice(i, SPEC.indexOf("\n}", i));
}

describe("WorldPatch 与规范 10.2 一致", () => {
  it("安全分级名称与数值逐项对应 Rust 枚举", () => {
    const block = specBlock("pub enum LiveEditSafetyClass");
    const variants = [...block.matchAll(/^\s+(\w+) = (\d),$/gm)].map(([, n, v]) => [n, Number(v)]);
    expect(variants).toEqual(SAFETY_CLASS_NAMES.map((n) => [n, LiveEditSafetyClass[n]]));
  });

  it("操作种类与字段名逐项对应 WorldPatchOp", () => {
    const block = specBlock("pub enum WorldPatchOp");
    const ops = [...block.matchAll(/^\s+(\w+) \{ (.+) \},$/gm)].map(([, name, fields]) => [
      name,
      [...(fields ?? "").matchAll(/(\w+):/g)].map((m) => m[1]),
    ]);
    expect(ops.map(([n]) => n)).toEqual(WORLD_PATCH_OP_KINDS);
    const fixture = JSON.parse(FIXTURE);
    for (const [name, fields] of ops) {
      const op = fixture.operations.find((o: Record<string, unknown>) => name as string in o);
      expect(Object.keys(op[name as string])).toEqual(fields);
    }
    expect(Object.keys(fixture)).toEqual([...specBlock("pub struct WorldPatch").matchAll(/pub (\w+):/g)].map((m) => m[1]));
    expect(Object.keys(fixture.hlc)).toEqual([...specBlock("pub struct HlcTimestamp").matchAll(/pub (\w+):/g)].map((m) => m[1]));
  });

  it("地形类型数值与规范 TerrainTileKind 一致", () => {
    const block = specBlock("pub enum TerrainTileKind");
    expect(Object.fromEntries([...block.matchAll(/(\w+) = (\d),/g)].map(([, n, v]) => [n, Number(v)]))).toEqual(TerrainTileKind);
  });

  it("serde JSON 往返逐字节一致，字段顺序与 Rust 声明一致", () => {
    expect(serializeWorldPatch(parseWorldPatch(JSON.parse(FIXTURE)))).toBe(FIXTURE);
    // 字段乱序输入也被规范化为声明顺序
    const shuffled = JSON.parse(FIXTURE);
    const reordered = { operations: shuffled.operations, safety_class: shuffled.safety_class, hlc: shuffled.hlc, base_authoring_revision: shuffled.base_authoring_revision, patch_id: shuffled.patch_id };
    expect(serializeWorldPatch(reordered as never)).toBe(FIXTURE);
  });

  const bad = (mut: (p: Record<string, any>) => void) => {
    const p = JSON.parse(FIXTURE);
    mut(p);
    try {
      parseWorldPatch(p);
      return null;
    } catch (e) {
      expect(e).toBeInstanceOf(RynvekError);
      expect((e as RynvekError).code).toBe(RynvekErrorCode.SimulationInvariantViolated);
      return (e as Error).message;
    }
  };

  it("拒绝非法补丁", () => {
    expect(bad((p) => (p.patch_id = p.patch_id.slice(1)))).toMatch(/patch_id/);
    expect(bad((p) => (p.patch_id[0] = 256))).toMatch(/patch_id\[0\]/);
    expect(bad((p) => (p.safety_class = 2))).toMatch(/safety_class/);
    expect(bad((p) => (p.hlc.logical_counter = 2 ** 32))).toMatch(/logical_counter/);
    expect(bad((p) => (p.extra = 1))).toMatch(/未知字段/);
    expect(bad((p) => (p.operations = []))).toMatch(/operations/);
    expect(bad((p) => (p.operations[0] = { SetWorldParam: { key_path: "Field", new_value_json: "1" } }))).toMatch(/key_path/);
    expect(bad((p) => (p.operations[0] = { SetWorldParam: { key_path: "a.b", new_value_json: "{" } }))).toMatch(/JSON/);
    expect(bad((p) => (p.operations[1].PaintTerrainRect.tile_kind = 5))).toMatch(/地形/);
    expect(bad((p) => (p.operations[1].PaintTerrainRect.width = 4096))).toBeNull();
    expect(bad((p) => (p.operations[1].PaintTerrainRect = { x0: 0, y0: 0, width: 2048, height: 1024, tile_kind: 0 }))).toMatch(/面积/);
    expect(bad((p) => (p.operations[2].UpsertSeedGenome.genome_bytes_b64 = "abc"))).toMatch(/Base64/);
    expect(bad((p) => (p.operations[3].UpdateRuleModule.compiled_rir_blake3 = [1]))).toMatch(/32 字节/);
    expect(bad((p) => (p.operations[4] = { RemoveEntityTemplate: { template_id: "x" }, SetWorldParam: {} }))).toMatch(/单键/);
    expect(bad((p) => (p.operations[4] = { SpawnUnit: {} }))).toMatch(/未知操作/);
  });

  it("HLC 全序与 Rust derive(Ord) 一致：wall → counter → author", () => {
    const t = (w: number, c: number, a: number) => ({ wall_time_ms: w, logical_counter: c, author_node_id: a });
    const sorted = [t(2, 0, 0), t(1, 5, 0), t(1, 0, 9), t(1, 0, 1)].sort(compareHlc);
    expect(sorted).toEqual([t(1, 0, 1), t(1, 0, 9), t(1, 5, 0), t(2, 0, 0)]);
  });
});

describe("HybridLogicalClock", () => {
  it("本地事件严格递增；物理时钟回拨时靠逻辑计数推进", () => {
    let pt = 100;
    const c = new HybridLogicalClock(3, () => pt);
    const a = c.now();
    const b = c.now();
    pt = 50;
    const d = c.now();
    pt = 200;
    const e = c.now();
    expect([a, b, d, e].map((x) => [x.wall_time_ms, x.logical_counter])).toEqual([
      [100, 0],
      [100, 1],
      [100, 2],
      [200, 0],
    ]);
    for (const [x, y] of [[a, b], [b, d], [d, e]] as const) expect(compareHlc(x, y)).toBeLessThan(0);
  });

  it("接收远端时间戳后，本地新事件因果地晚于远端", () => {
    const c = new HybridLogicalClock(1, () => 100);
    const remote = { wall_time_ms: 150, logical_counter: 7, author_node_id: 2 };
    expect(c.receive(remote)).toEqual({ wall_time_ms: 150, logical_counter: 8, author_node_id: 1 });
    expect(compareHlc(c.now(), remote)).toBeGreaterThan(0);
    // 三者同墙钟：取最大计数 + 1
    const same = new HybridLogicalClock(1, () => 10);
    same.now();
    same.now(); // (10, 1)
    expect(same.receive({ wall_time_ms: 10, logical_counter: 0, author_node_id: 2 }).logical_counter).toBe(2);
    expect(same.receive({ wall_time_ms: 10, logical_counter: 9, author_node_id: 2 }).logical_counter).toBe(10);
    // 物理时钟最大：计数清零
    expect(new HybridLogicalClock(1, () => 999).receive(remote)).toEqual({ wall_time_ms: 999, logical_counter: 0, author_node_id: 1 });
  });

  it("拒绝领先过多的远端时间戳；计数溢出借位到墙钟", () => {
    const c = new HybridLogicalClock(1, () => 0);
    expect(() => c.receive({ wall_time_ms: HLC_MAX_DRIFT_MS + 1, logical_counter: 0, author_node_id: 2 })).toThrow(RynvekError);
    c.receive({ wall_time_ms: 5, logical_counter: 0xffff_fffe, author_node_id: 2 });
    expect(c.peek()).toMatchObject({ wall_time_ms: 5, logical_counter: 0xffff_ffff });
    expect(c.now()).toMatchObject({ wall_time_ms: 6, logical_counter: 0 });
    expect(() => new HybridLogicalClock(-1)).toThrow(RangeError);
  });
});
