// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 结果表行类型与谱系 / 指标分析工具。
 *
 * 行类型与 Parquet 列结构（`rynvek_rts::lab::arrow_io`）逐列对应；Parquet 解码交给调用方选择的
 * Arrow 实现（浏览器 Arrow JS / DuckDB-WASM、Python `pyarrow`），本包只处理解码后的行对象，
 * 不引入额外依赖。`u64` 列以 `number | bigint` 接收（Arrow JS 对 64 位整数产出 `bigint`）。
 */

type U64 = number | bigint;

/** `lineage` 表一行：基因组 slug 首次出现记录 */
export interface LineageRow {
  readonly run: number;
  readonly faction: number;
  readonly slug: string;
  readonly unit: string;
  readonly generation: number;
  readonly first_seen_tick: U64;
}

/** `metrics` 表一行（节选分析所需列） */
export interface MetricRow {
  readonly run: number;
  readonly tick: U64;
  readonly faction: number;
  readonly score_milli: U64;
  readonly units: number;
  readonly buildings: number;
  readonly wild_units: number;
}

/** `runs` 表一行（节选） */
export interface RunRow {
  readonly run: number;
  readonly variant: string;
  readonly seed: U64;
  readonly mode: string;
  readonly players: number;
  readonly final_tick: U64;
  readonly winner: number | null;
  readonly end_reason: string;
  readonly genomes_bred: number;
  readonly waves_spawned: number;
}

/** 野生虫潮阵营编号（`FactionId::WILD`） */
export const WILD_FACTION = 255;

const num = (v: U64): number => (typeof v === "bigint" ? Number(v) : v);

/** 每个 run 的谱系摘要 */
export interface LineageSummary {
  readonly run: number;
  readonly genomes: number;
  readonly maxGeneration: number;
  /** 代数 → 基因组数 */
  readonly generations: ReadonlyMap<number, number>;
  /** 单位原型 → 基因组数 */
  readonly byUnit: ReadonlyMap<string, number>;
  /** 野生虫潮贡献的基因组数（自然选择） */
  readonly wild: number;
}

/** 按 run 汇总谱系 */
export function summarizeLineage(rows: Iterable<LineageRow>): Map<number, LineageSummary> {
  const acc = new Map<
    number,
    { genomes: number; maxGeneration: number; generations: Map<number, number>; byUnit: Map<string, number>; wild: number }
  >();
  for (const r of rows) {
    let a = acc.get(r.run);
    if (!a) {
      a = { genomes: 0, maxGeneration: 0, generations: new Map(), byUnit: new Map(), wild: 0 };
      acc.set(r.run, a);
    }
    a.genomes += 1;
    a.maxGeneration = Math.max(a.maxGeneration, r.generation);
    a.generations.set(r.generation, (a.generations.get(r.generation) ?? 0) + 1);
    a.byUnit.set(r.unit, (a.byUnit.get(r.unit) ?? 0) + 1);
    if (r.faction === WILD_FACTION) a.wild += 1;
  }
  const out = new Map<number, LineageSummary>();
  for (const [run, a] of [...acc].sort((x, y) => x[0] - y[0])) out.set(run, { run, ...a });
  return out;
}

/**
 * 累计谱系多样性曲线：每个采样 Tick 时已出现的不同基因组数（按 run）。
 * `ticks` 须升序；返回与 `ticks` 等长的数组。
 */
export function diversityCurve(rows: Iterable<LineageRow>, run: number, ticks: readonly number[]): number[] {
  const firstSeen = [...rows]
    .filter((r) => r.run === run)
    .map((r) => num(r.first_seen_tick))
    .sort((a, b) => a - b);
  const out: number[] = [];
  let i = 0;
  for (const t of ticks) {
    while (i < firstSeen.length && (firstSeen[i] as number) <= t) i += 1;
    out.push(i);
  }
  return out;
}

/** 变体级对局统计 */
export interface VariantStats {
  readonly variant: string;
  readonly runs: number;
  /** 有胜者的 run 中玩家阵营获胜的比例（PvE 的 waves_cleared 亦计为胜） */
  readonly winRate: number;
  readonly meanFinalTick: number;
  readonly meanGenomesBred: number;
  readonly endReasons: ReadonlyMap<string, number>;
}

/** 按变体聚合 `runs` 表 */
export function variantStats(rows: Iterable<RunRow>): VariantStats[] {
  const groups = new Map<string, RunRow[]>();
  for (const r of rows) {
    const g = groups.get(r.variant) ?? [];
    g.push(r);
    groups.set(r.variant, g);
  }
  return [...groups].map(([variant, g]) => {
    const reasons = new Map<string, number>();
    for (const r of g) reasons.set(r.end_reason, (reasons.get(r.end_reason) ?? 0) + 1);
    const wins = g.filter((r) => r.winner !== null && r.winner !== WILD_FACTION).length;
    const mean = (f: (r: RunRow) => number) => g.reduce((s, r) => s + f(r), 0) / g.length;
    return {
      variant,
      runs: g.length,
      winRate: wins / g.length,
      meanFinalTick: mean((r) => num(r.final_tick)),
      meanGenomesBred: mean((r) => r.genomes_bred),
      endReasons: reasons,
    };
  });
}

/** 某 run 某阵营的得分时间序列（Tick 升序；score 为 milli 定点） */
export function scoreSeries(rows: Iterable<MetricRow>, run: number, faction: number): Array<[number, number]> {
  return [...rows]
    .filter((r) => r.run === run && r.faction === faction)
    .map((r): [number, number] => [num(r.tick), num(r.score_milli)])
    .sort((a, b) => a[0] - b[0]);
}

/** 三张结果表的完整列名（顺序冻结；测试直接解析 Rust `arrow_io.rs` 比对） */
export const TABLE_COLUMNS = {
  runs: [
    "run",
    "variant",
    "seed",
    "mode",
    "players",
    "policy",
    "final_tick",
    "winner",
    "end_reason",
    "final_state_root",
    "commands_accepted",
    "commands_rejected",
    "genomes_bred",
    "waves_spawned",
  ],
  metrics: [
    "run",
    "tick",
    "faction",
    "biomass_milli",
    "flux_milli",
    "neurogel_milli",
    "biomass_harvested_total_milli",
    "supply_used",
    "supply_cap",
    "units",
    "buildings",
    "score_milli",
    "wild_units",
    "pve_wave",
    "state_root",
  ],
  lineage: ["run", "faction", "slug", "unit", "generation", "first_seen_tick"],
} as const;
