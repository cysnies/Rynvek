// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import {
  COUPLING_EPOCH_TICKS,
  LiveEditScheduler,
  LiveEditSession,
  isSafetyClassSufficient,
  requiredClassOf,
  requiredSafetyClass,
  scheduleFor,
  type WorldPatch,
  type WorldPatchOp,
} from "../src/index.js";
import { baseline, idGen, manualClock } from "./helpers.js";

const set = (key_path: string): WorldPatchOp => ({ SetWorldParam: { key_path, new_value_json: "1" } });
const rule: WorldPatchOp = { UpdateRuleModule: { module_id: "m", ts_source: "", compiled_rir_blake3: new Array(32).fill(0) } };

describe("LiveEditSafetyClass 判定与调度", () => {
  it("操作分级", () => {
    expect(requiredClassOf(set("presentation.grid_alpha"))).toBe(0);
    expect(requiredClassOf(set("audio.master"))).toBe(0);
    expect(requiredClassOf(set("field.nutrient.diffusion"))).toBe(1);
    expect(requiredClassOf(set("buildings.extractor.cost"))).toBe(1);
    expect(requiredClassOf(set("neural.topology.max_synapses"))).toBe(2);
    expect(requiredClassOf(set("world.chunk_size"))).toBe(3);
    expect(requiredClassOf(set("mystery.param"))).toBe(3); // 未登记命名空间按最保守处理
    expect(requiredClassOf({ PaintTerrainRect: { x0: 0, y0: 0, width: 1, height: 1, tile_kind: 0 } })).toBe(1);
    expect(requiredClassOf({ UpsertSeedGenome: { genome_slug: "a", genome_bytes_b64: "" } })).toBe(2);
    expect(requiredClassOf(rule)).toBe(2);
    expect(requiredClassOf({ RemoveEntityTemplate: { template_id: "t" } })).toBe(1);
    expect(requiredSafetyClass([set("ui.x"), rule, set("field.a")])).toBe(2);
  });

  it("生效时机：帧 / 下一 Tick Phase 01 / 下一 Epoch（tick % 4 == 0）/ Fork", () => {
    expect(COUPLING_EPOCH_TICKS).toBe(4);
    expect(scheduleFor(0, 10)).toEqual({ kind: "frame" });
    expect(scheduleFor(1, 10)).toEqual({ kind: "tick", tick: 11, phase: "Phase01" });
    expect(scheduleFor(2, 10)).toEqual({ kind: "epoch", tick: 12 });
    expect(scheduleFor(2, 11)).toEqual({ kind: "epoch", tick: 12 });
    expect(scheduleFor(2, 12)).toEqual({ kind: "epoch", tick: 16 });
    expect(scheduleFor(2, 0)).toEqual({ kind: "epoch", tick: 4 });
    expect(scheduleFor(3, 10)).toEqual({ kind: "fork" });
  });

  it("调度器按时机分派并按 HLC 排序", () => {
    const s = new LiveEditScheduler();
    const p = (n: number, cls: WorldPatch["safety_class"], ops: WorldPatchOp[]): WorldPatch => ({
      patch_id: new Array(16).fill(n),
      base_authoring_revision: 0,
      hlc: { wall_time_ms: 100 - n, logical_counter: 0, author_node_id: 1 },
      safety_class: cls,
      operations: ops,
    });
    s.submit(p(1, "HotSafePresentation", [set("ui.a")]), 9);
    s.submit(p(2, "TickBoundarySafe", [set("field.a")]), 9);
    s.submit(p(3, "TickBoundarySafe", [set("field.b")]), 9);
    s.submit(p(4, "EpochBoundarySafe", [rule]), 9);
    s.submit(p(5, "RequiresForkRestart", [set("world.chunk_size")]), 9);
    expect(() => s.submit(p(6, "HotSafePresentation", [rule]), 9)).toThrow(/低于/);
    expect(s.drainFrame().map((x) => x.patch_id[0])).toEqual([1]);
    expect(s.drainTick(10).map((x) => x.patch_id[0])).toEqual([3, 2]); // HLC 小者在前
    expect(s.drainTick(11)).toEqual([]);
    expect(s.drainTick(12).map((x) => x.patch_id[0])).toEqual([4]);
    expect(s.drainForkRequests().map((x) => x.patch_id[0])).toEqual([5]);
    expect(s.pendingCount).toBe(0);
    expect(isSafetyClassSufficient(p(7, "EpochBoundarySafe", [set("ui.a")]))).toBe(true);
  });
});

describe("LiveEditSession 双端协作", () => {
  it("两位作者互发补丁后收敛，重复到达不再调度", () => {
    const clock = manualClock(1_000);
    const outA: WorldPatch[] = [];
    const outB: WorldPatch[] = [];
    let tick = 7;
    const mk = (author: number, out: WorldPatch[]) =>
      new LiveEditSession({
        baseline: baseline(),
        authorNodeId: author,
        committedTick: () => tick,
        physicalNow: clock.now,
        patchIds: idGen(author),
        broadcast: (p) => out.push(p),
      });
    const a = mk(1, outA);
    const b = mk(2, outB);

    expect(a.edit([set("field.nutrient.diffusion")]).timing).toEqual({ kind: "tick", tick: 8, phase: "Phase01" });
    expect(b.edit([rule]).timing).toEqual({ kind: "epoch", tick: 8 });
    clock.advance(1);
    b.edit([{ PaintTerrainRect: { x0: 0, y0: 0, width: 2, height: 2, tile_kind: 1 } }]);

    for (const p of outB) expect(a.receive(p).timing).not.toBeNull();
    for (const p of outA) b.receive(p);
    expect(a.receive(outB[0]!).timing).toBeNull();
    expect(a.document.stateHash()).toBe(b.document.stateHash());

    // 接收后本地时钟晚于所有已见补丁
    const next = a.edit([set("ui.x")]);
    for (const p of outB) expect(next.patch.hlc.wall_time_ms * 2 ** 32 + next.patch.hlc.logical_counter).toBeGreaterThan(p.hlc.wall_time_ms * 2 ** 32 + p.hlc.logical_counter);

    tick = 8;
    expect(a.scheduler.drainFrame()).toHaveLength(1);
    expect(a.scheduler.drainTick(8)).toHaveLength(3);
    // 撤销经会话发布，进入调度并广播
    const undone = a.undo.undo()!;
    const published = a.publish(undone);
    expect(published.timing).toEqual({ kind: "frame" });
    expect(outA.at(-1)).toBe(undone.patch);
  });
});
