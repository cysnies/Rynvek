// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { type LineageRow, type RunRow, WILD_FACTION, genomeIdentityCurve, scoreSeries, summarizeLineage, variantStats, lineageForest, descendants, energyBalance } from "../src/index.js";

const lineage: LineageRow[] = [
  { run: 0, faction: 0, slug: "g-a", unit: "unit.forager", generation: 0, born_tick: 0n },
  { run: 0, faction: WILD_FACTION, slug: "g-b", unit: "unit.striker", generation: 1, born_tick: 300n },
  { run: 0, faction: WILD_FACTION, slug: "g-c", unit: "unit.striker", generation: 2, born_tick: 600 },
  { run: 1, faction: 0, slug: "g-d", unit: "unit.forager", generation: 0, born_tick: 0 },
].map((r,i):LineageRow=>({...r,entity:i+1,parent:null,origin:r.generation===0?"initial":"spawn",
 genome:i.toString(16).padStart(64,"0"),parent_genome:null,allocated_q:0n}));

describe("谱系与指标分析", () => {
  it("按 run 汇总谱系", () => {
    const s = summarizeLineage(lineage);
    expect([...s.keys()]).toEqual([0, 1]);
    const r0 = s.get(0)!;
    expect(r0.genomes).toBe(3);
    expect(r0.maxGeneration).toBe(2);
    expect(r0.wild).toBe(2);
    expect(r0.byUnit.get("unit.striker")).toBe(2);
    expect([...r0.generations]).toEqual([
      [0, 1],
      [1, 1],
      [2, 1],
    ]);
  });

  it("累计规范基因身份曲线（接受 bigint Tick）", () => {
    expect(genomeIdentityCurve(lineage, 0, [0, 299, 300, 900])).toEqual([1, 1, 2, 3]);
    expect(genomeIdentityCurve(lineage, 9, [0])).toEqual([0]);
  });

  it("变体统计与得分序列", () => {
    const runs: RunRow[] = [
      { run: 0, variant: "a", seed: 1n, mode: "pve", players: 1, final_tick: 300n, winner: 0, end_reason: "waves_cleared", genomes_bred: 2, waves_spawned: 3, natural_births: 4, max_generation: 2 },
      { run: 1, variant: "a", seed: 2n, mode: "pve", players: 1, final_tick: 100n, winner: null, end_reason: "defeat", genomes_bred: 0, waves_spawned: 1, natural_births: 0, max_generation: 0 },
      { run: 2, variant: "b", seed: 1n, mode: "duel", players: 2, final_tick: 600n, winner: null, end_reason: "running", genomes_bred: 4, waves_spawned: 0, natural_births: 0, max_generation: 0 },
    ];
    const [a, b] = variantStats(runs);
    expect(a).toMatchObject({ variant: "a", runs: 2, winRate: 0.5, meanFinalTick: 200, meanGenomesBred: 1 });
    expect(a?.endReasons.get("defeat")).toBe(1);
    expect(b).toMatchObject({ variant: "b", runs: 1, winRate: 0 });
    const series = scoreSeries(
      [
        { run: 0, tick: 60n, faction: 0, score_milli: 2000n, units: 1, buildings: 1, wild_units: 0 },
        { run: 0, tick: 30n, faction: 0, score_milli: 1000n, units: 1, buildings: 1, wild_units: 0 },
        { run: 0, tick: 30n, faction: 1, score_milli: 5n, units: 1, buildings: 1, wild_units: 0 },
      ],
      0,
      0,
    );
    expect(series).toEqual([
      [30, 1000],
      [60, 2000],
    ]);
  });
});

 it("真实亲代森林/子孙查询拒绝伪链接，能量分析不丢大整数",()=>{
   const root:LineageRow={...lineage[0]!,run:7,entity:1};
   const child:LineageRow={...root,entity:2,parent:1,origin:"mitosis",generation:1,born_tick:10n,genome:"1".repeat(64),parent_genome:root.genome,allocated_q:4000000n};
   const grand:LineageRow={...child,entity:3,parent:2,generation:2,born_tick:20n,genome:"2".repeat(64),parent_genome:child.genome};
   expect(lineageForest([grand,root,child]).get(7)?.get(1)?.children).toEqual([2]);
   expect(descendants([grand,root,child],7,1).map(r=>r.entity)).toEqual([2,3]);
   expect(()=>lineageForest([child])).toThrow();
   expect(()=>lineageForest([root,{...child,parent_genome:"f".repeat(64)}])).toThrow();
   const n=9007199254740993n;
   const row={attack_q:0n,repair_q:0n,secretion_q:0n,synthesis_q:0n,continuous_q:1n,enabled:true,run:0,tick:30n,received_q:n+4n,transferred_q:4n,external_received_q:n.toString(),stored_q:n-3n,dissipated_q:1n,exported_q:1n,returned_q:1n};
   expect(energyBalance(row).balanced).toBe(true);
   expect(energyBalance({...row,continuous_q:2n}).balanced).toBe(false);
 });
