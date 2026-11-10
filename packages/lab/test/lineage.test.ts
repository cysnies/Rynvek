// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { type LineageRow, type RunRow, WILD_FACTION, diversityCurve, scoreSeries, summarizeLineage, variantStats } from "../src/index.js";

const lineage: LineageRow[] = [
  { run: 0, faction: 0, slug: "g-a", unit: "unit.forager", generation: 0, first_seen_tick: 0n },
  { run: 0, faction: WILD_FACTION, slug: "g-b", unit: "unit.striker", generation: 1, first_seen_tick: 300n },
  { run: 0, faction: WILD_FACTION, slug: "g-c", unit: "unit.striker", generation: 2, first_seen_tick: 600 },
  { run: 1, faction: 0, slug: "g-d", unit: "unit.forager", generation: 0, first_seen_tick: 0 },
];

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

  it("累计多样性曲线（接受 bigint Tick）", () => {
    expect(diversityCurve(lineage, 0, [0, 299, 300, 900])).toEqual([1, 1, 2, 3]);
    expect(diversityCurve(lineage, 9, [0])).toEqual([0]);
  });

  it("变体统计与得分序列", () => {
    const runs: RunRow[] = [
      { run: 0, variant: "a", seed: 1n, mode: "pve", players: 1, final_tick: 300n, winner: 0, end_reason: "waves_cleared", genomes_bred: 2, waves_spawned: 3 },
      { run: 1, variant: "a", seed: 2n, mode: "pve", players: 1, final_tick: 100n, winner: null, end_reason: "defeat", genomes_bred: 0, waves_spawned: 1 },
      { run: 2, variant: "b", seed: 1n, mode: "duel", players: 2, final_tick: 600n, winner: null, end_reason: "running", genomes_bred: 4, waves_spawned: 0 },
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
