// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { RynvekError } from "@rynvek/core";
import { describe, expect, it } from "vitest";
import {
  AuthoringDocument,
  HybridLogicalClock,
  LiveEditSafetyClass,
  UndoManager,
  tileRunsToRects,
  type WorldPatch,
  type WorldPatchOp,
} from "../src/index.js";
import { baseline, idGen, manualClock, permutations, rng } from "./helpers.js";

const set = (key_path: string, v: unknown): WorldPatchOp => ({ SetWorldParam: { key_path, new_value_json: JSON.stringify(v) } });
const paint = (x0: number, y0: number, width: number, height: number, tile_kind: number): WorldPatchOp => ({
  PaintTerrainRect: { x0, y0, width, height, tile_kind },
});

/** 三位作者在同一基线上并发编辑（各自只看到自己的补丁） */
function concurrentPatches(): WorldPatch[] {
  const out: WorldPatch[] = [];
  const clock = manualClock(10_000);
  const authors = [1, 2, 3].map((a) => ({
    doc: new AuthoringDocument(baseline()),
    hlc: new HybridLogicalClock(a, clock.now),
    ids: idGen(a),
  }));
  const ops: WorldPatchOp[][] = [
    [set("field.nutrient.diffusion", 0.2), paint(0, 0, 4, 3, 2)],
    [set("field.nutrient.diffusion", 0.3), paint(2, 1, 4, 3, 3), { RemoveEntityTemplate: { template_id: "spore-pod" } }],
    [paint(1, 1, 2, 2, 4), { UpsertSeedGenome: { genome_slug: "amoeba-basic", genome_bytes_b64: "AQID" } }],
    [set("presentation.grid_alpha", 0.8)],
    [{ UpdateRuleModule: { module_id: "rts.economy", rule_graph_json: "v2", compiled_rir_blake3: new Array(32).fill(2) } }],
    [set("field.nutrient.diffusion", 0.4)],
  ];
  ops.forEach((o, i) => {
    const a = authors[i % 3]!;
    // 让部分补丁落在同一毫秒，靠逻辑计数与作者 ID 决胜
    if (i % 2 === 0) clock.advance(1);
    const p = a.doc.createPatch(a.hlc, o, a.ids());
    a.doc.integrate(p);
    out.push(p);
  });
  return out;
}

describe("AuthoringDocument CRDT 合并", () => {
  it("收敛：6 个并发补丁的全部 720 种到达顺序得到逐位相同的状态", () => {
    const patches = concurrentPatches();
    const hashes = new Set<string>();
    let reference = "";
    for (const order of permutations(patches)) {
      const doc = new AuthoringDocument(baseline());
      doc.integrateAll(order);
      hashes.add(doc.stateHash());
      reference = doc.canonicalState();
    }
    expect(hashes.size).toBe(1);
    const state = JSON.parse(reference);
    // 参数：HLC 最大者胜出
    expect(state.params["field.nutrient.diffusion"]).toBe("0.4");
    expect(state.templates).toEqual(["genome/amoeba-basic", "legacy-turret"]);
    expect(state.genomes["amoeba-basic"]).toBe("AQID");
    expect(state.rules["rts.economy"].rule_graph_json).toBe("v2");
  });

  it("幂等与结合律：重复投递、分批合并与一次性合并结果一致", () => {
    const patches = concurrentPatches();
    const once = new AuthoringDocument(baseline());
    once.integrateAll(patches);

    const dup = new AuthoringDocument(baseline());
    expect(dup.integrate(patches[0]!).applied).toBe(true);
    expect(dup.integrate(patches[0]!).applied).toBe(false);
    dup.integrateAll([...patches, ...patches].reverse());
    expect(dup.revision).toBe(patches.length);
    expect(dup.stateHash()).toBe(once.stateHash());

    // 两个副本各收一半，再互相交换
    const a = new AuthoringDocument(baseline());
    const b = new AuthoringDocument(baseline());
    a.integrateAll(patches.slice(0, 3));
    b.integrateAll(patches.slice(3));
    a.integrateAll(b.allPatches());
    b.integrateAll(a.allPatches());
    expect(a.stateHash()).toBe(b.stateHash());
    expect(a.stateHash()).toBe(once.stateHash());
  });

  it("随机化收敛：200 轮确定性随机补丁与随机到达顺序", () => {
    const next = rng(0x5eed);
    const clock = manualClock(1);
    const clocks = [1, 2, 3, 4].map((a) => new HybridLogicalClock(a, clock.now));
    const ids = [1, 2, 3, 4].map(idGen);
    const template = new AuthoringDocument(baseline());
    const patches: WorldPatch[] = [];
    for (let i = 0; i < 200; i++) {
      const who = Math.floor(next() * 4);
      if (next() < 0.5) clock.advance(1);
      const x = Math.floor(next() * 8);
      const y = Math.floor(next() * 6);
      const op: WorldPatchOp =
        next() < 0.5
          ? paint(x, y, 1 + Math.floor(next() * (8 - x)), 1 + Math.floor(next() * (6 - y)), Math.floor(next() * 5))
          : set(`field.k${Math.floor(next() * 4)}`, Math.floor(next() * 100));
      patches.push(template.createPatch(clocks[who]!, [op], ids[who]!()));
    }
    const shuffled = (seed: number) => {
      const r = rng(seed);
      const xs = patches.slice();
      for (let i = xs.length - 1; i > 0; i--) {
        const j = Math.floor(r() * (i + 1));
        [xs[i], xs[j]] = [xs[j]!, xs[i]!];
      }
      return xs;
    };
    const hashes = new Set<string>();
    for (const seed of [1, 2, 3, 4, 5, 6, 7, 8]) {
      const d = new AuthoringDocument(baseline());
      d.integrateAll(shuffled(seed));
      hashes.add(d.stateHash());
    }
    expect(hashes.size).toBe(1);
  });

  it("冲突报告：不同作者写同一寄存器时给出胜者与败者", () => {
    const [p1, p2] = concurrentPatches();
    const doc = new AuthoringDocument(baseline());
    doc.integrate(p2!); // 较晚的 0.3 先到
    const r = doc.integrate(p1!); // 较早的 0.2 后到，败
    expect(r.conflicts).toContainEqual({
      register: { kind: "param", key: "field.nutrient.diffusion" },
      winner: expect.any(String),
      loser: expect.any(String),
    });
    const c = r.conflicts.find((x) => x.register.kind === "param")!;
    expect(c.loser).toBe(p1!.patch_id.map((b) => b.toString(16).padStart(2, "0")).join(""));
    expect(doc.param("field.nutrient.diffusion")).toBe("0.3");
  });

  it("原子校验：越界涂刷与分级声明不足的补丁整体拒绝", () => {
    const doc = new AuthoringDocument(baseline());
    const clock = new HybridLogicalClock(1, () => 5);
    const ids = idGen(1);
    const oob = doc.createPatch(clock, [set("field.a", 1), paint(6, 0, 3, 1, 1)], ids());
    expect(() => doc.integrate(oob)).toThrow(RynvekError);
    expect(doc.param("field.a")).toBeUndefined();
    expect(doc.revision).toBe(0);
    const under = { ...doc.createPatch(clock, [{ UpdateRuleModule: { module_id: "m", rule_graph_json: "", compiled_rir_blake3: new Array(32).fill(0) } }], ids()), safety_class: "TickBoundarySafe" as const };
    expect(() => doc.integrate(under)).toThrow(/低于所需/);
    // 声明更保守的分级是允许的
    const over = { ...doc.createPatch(clock, [set("presentation.x", 1)], ids()), safety_class: "RequiresForkRestart" as const };
    expect(doc.integrate(over).applied).toBe(true);
  });

  it("物化视图：null 删除参数、模板只能删除", () => {
    const doc = new AuthoringDocument(baseline());
    const clock = new HybridLogicalClock(1, () => 5);
    const ids = idGen(1);
    doc.integrate(doc.createPatch(clock, [set("presentation.grid_alpha", null), { RemoveEntityTemplate: { template_id: "ghost" } }], ids()));
    expect(doc.param("presentation.grid_alpha")).toBeUndefined();
    expect(doc.hasTemplate("ghost")).toBe(false);
    expect(doc.hasTemplate("spore-pod")).toBe(true);
    expect(() => doc.tileAt(8, 0)).toThrow(RangeError);
  });
});

describe("选择性撤销 / 重做", () => {
  function setup() {
    const clock = manualClock(100);
    const mine = new AuthoringDocument(baseline());
    const hlc = new HybridLogicalClock(1, clock.now);
    const undo = new UndoManager(mine, hlc, idGen(1));
    return { clock, mine, hlc, undo };
  }

  it("撤销只回退自己仍胜出的寄存器，不覆盖他人更新", () => {
    const { clock, mine, undo } = setup();
    const e1 = undo.commit([set("field.nutrient.diffusion", 0.9), set("field.other", 1), paint(0, 0, 3, 2, 4)]);
    expect(e1.patch.safety_class).toBe("TickBoundarySafe");
    // 另一位作者随后改了其中一个参数和一个格
    clock.advance(10);
    const other = new HybridLogicalClock(2, clock.now);
    mine.integrate(mine.createPatch(other, [set("field.other", 2), paint(1, 0, 1, 1, 3)], idGen(2)()));

    clock.advance(10);
    const u = undo.undo()!;
    expect(u.patch.operations).toEqual([
      { SetWorldParam: { key_path: "field.nutrient.diffusion", new_value_json: "0.1" } },
      // 第 0 行 (1, 0) 已被他人改写，剩余格压缩为行程矩形
      paint(0, 0, 1, 1, 0),
      paint(2, 0, 1, 1, 0),
      paint(0, 1, 3, 1, 0),
    ]);
    expect(u.skipped).toEqual([
      { register: { kind: "param", key: "field.other" }, reason: "superseded" },
      { register: { kind: "tile", x: 1, y: 0 }, reason: "superseded" },
    ]);
    expect(mine.param("field.nutrient.diffusion")).toBe("0.1");
    expect(mine.param("field.other")).toBe("2");
    expect(mine.tileAt(1, 0)).toBe(3);
    expect(mine.tileAt(0, 0)).toBe(0);

    clock.advance(10);
    undo.redo();
    expect(mine.param("field.nutrient.diffusion")).toBe("0.9");
    expect(mine.tileAt(0, 1)).toBe(4);
    expect(mine.tileAt(1, 0)).toBe(3);
    expect(undo.canRedo).toBe(false);
    expect(undo.canUndo).toBe(true);
  });

  it("新参数撤销为 null；新增基因组可目录删除，非基因模板仍不可逆", () => {
    const { mine, undo } = setup();
    undo.commit([
      set("field.fresh", 1),
      { UpsertSeedGenome: { genome_slug: "brand-new", genome_bytes_b64: "AA==" } },
      { RemoveEntityTemplate: { template_id: "spore-pod" } },
    ]);
    const u = undo.undo()!;
    expect(u.patch.operations).toEqual([{ SetWorldParam: { key_path: "field.fresh", new_value_json: "null" } },{RemoveEntityTemplate:{template_id:"genome/brand-new"}}]);
    expect(mine.genome("brand-new")).toBeUndefined();
    expect(u.skipped.map((s) => s.reason)).toEqual(["not_invertible"]);
    expect(mine.param("field.fresh")).toBeUndefined();
  });

  it("被他人完全覆盖的编辑在撤销时被跳过；新编辑清空重做栈", () => {
    const { clock, mine, undo } = setup();
    undo.commit([set("field.a", 1)]);
    clock.advance(5);
    undo.commit([set("field.b", 1)]);
    clock.advance(5);
    mine.integrate(mine.createPatch(new HybridLogicalClock(2, clock.now), [set("field.b", 7)], idGen(2)()));
    clock.advance(5);
    const u = undo.undo()!; // field.b 已被覆盖 → 直接撤销更早的 field.a
    expect(u.patch.operations).toEqual([{ SetWorldParam: { key_path: "field.a", new_value_json: "null" } }]);
    expect(undo.undo()).toBeNull();
    expect(undo.canRedo).toBe(true);
    undo.commit([set("field.c", 1)]);
    expect(undo.canRedo).toBe(false);
  });

  it("逆操作的地形行程压缩", () => {
    const rows = new Map([
      [0, new Map([[0, 1], [1, 1], [2, 2]])],
      [1, new Map([[0, 1], [1, 1], [2, 3]])],
      [3, new Map([[0, 1], [1, 1]])],
    ]);
    expect(tileRunsToRects(rows)).toEqual([
      paint(0, 0, 2, 2, 1),
      paint(2, 0, 1, 1, 2),
      paint(2, 1, 1, 1, 3),
      paint(0, 3, 2, 1, 1),
    ]);
  });

  it("撤销补丁的分级按其操作重新计算", () => {
    const { undo } = setup();
    undo.commit([set("presentation.grid_alpha", 0.2)]);
    expect(undo.undo()!.patch.safety_class).toBe("HotSafePresentation");
    expect(LiveEditSafetyClass.HotSafePresentation).toBe(0);
  });
});
