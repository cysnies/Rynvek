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

/** `lineage`真实出生节点：目录不是祖先，非自然出生不猜测亲代。 */
export interface LineageRow {
  readonly run: number;
  readonly faction: number;
  readonly slug: string;
  readonly unit: string;
  readonly generation: number;
  readonly entity: number;
  readonly parent: number | null;
  readonly origin: "initial" | "spawn" | "mitosis";
  readonly born_tick: U64;
  readonly genome: string | Uint8Array;
  readonly parent_genome: string | Uint8Array | null;
  readonly allocated_q: U64;
}

/** `metrics` 表一行（节选分析所需列） */
export interface MetricRow {
  readonly run: number;
  readonly tick: U64;
  readonly faction: number | null;
  readonly score_milli: U64 | null;
  readonly units: number | null;
  readonly buildings: number | null;
  readonly wild_units: number | null;
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
  readonly natural_births: number;
  /** 自然出生因果链最大代数，不混入虫潮生成代数。 */
  readonly max_generation: number;
}

/** 野生虫潮阵营编号（`FactionId::WILD`） */
export const WILD_FACTION = 255;

const num = (v: U64): number => {
  const n=typeof v==="bigint" ? Number(v):v;
  if (!Number.isSafeInteger(n)) throw new Error("整数超出安全number范围，禁止静默丢精度");
  return n;
};

export function genomeKey(value:string|Uint8Array):string {
  const key=typeof value==="string"?value:[...value].map(b=>b.toString(16).padStart(2,"0")).join("");
  if (!/^[0-9a-fA-F]{64}$/.test(key)) throw new Error("基因指纹必须为32字节");
  return key.toLowerCase();
}

export interface LineageNode {readonly row:LineageRow; readonly children:readonly number[]}
/** 按真实实体建森林，拒绝孤儿、重复ID、跨阵营/代数/基因指纹伪链接和环。 */
export function lineageForest(rows:Iterable<LineageRow>):Map<number,Map<number,LineageNode>> {
  const forest=new Map<number,Map<number,{row:LineageRow;children:number[]}>>();
  for (const row of rows) {
    if (!Number.isInteger(row.entity)||row.entity<1||row.entity>0xffff_ffff) throw new Error("无效实体ID");
    const run=forest.get(row.run)??new Map();
    if (run.has(row.entity)) throw new Error("同一run出现重复实体ID");
    genomeKey(row.genome);
    run.set(row.entity,{row,children:[]}); forest.set(row.run,run);
  }
  for (const run of forest.values()) for (const node of run.values()) {
    const r=node.row;
    if (r.parent===null) {
      if (r.origin==="mitosis"||r.parent_genome!==null||num(r.allocated_q)!==0) throw new Error("出生来源与亲代账不符");
      continue;
    }
    const p=run.get(r.parent)?.row;
    if (!p||r.parent>=r.entity||r.origin!=="mitosis"||p.faction!==r.faction||r.generation!==p.generation+1
      ||num(r.born_tick)<num(p.born_tick)||r.parent_genome===null||genomeKey(r.parent_genome)!==genomeKey(p.genome)
      ||num(r.allocated_q)<=0) throw new Error("无效自然亲代链接");
    run.get(r.parent)!.children.push(r.entity);
  }
  for (const run of forest.values()) for (const n of run.values()) n.children.sort((a,b)=>a-b);
  return forest;
}

/** 子孙查询不依赖slug命名或采样间隔。 */
export function descendants(rows:Iterable<LineageRow>,run:number,entity:number):LineageRow[] {
  const tree=lineageForest(rows).get(run); const out:LineageRow[]=[];
  const queue=[...(tree?.get(entity)?.children??[])];
  for (let i=0;i<queue.length;i++) {const node=tree!.get(queue[i]!)!;out.push(node.row);queue.push(...node.children);}
  return out;
}

/** Arrow Decimal128解码后应保留bigint或十进制字符串，不能先经number。 */
export interface EnergyRow {
  readonly enabled:boolean; readonly run:number; readonly tick:U64;
  readonly received_q:bigint|string; readonly stored_q:bigint|string; readonly dissipated_q:bigint|string;
  readonly exported_q:bigint|string; readonly returned_q:bigint|string; readonly transferred_q:bigint|string;
  readonly external_received_q:bigint|string;
  readonly attack_q:bigint|string;
  readonly repair_q:bigint|string;
  readonly secretion_q:bigint|string;
  readonly synthesis_q:bigint|string;
  readonly continuous_q:bigint|string;
  readonly rule_q:bigint|string;
  readonly material_received_u:bigint|string;
  readonly material_external_u:bigint|string;
  readonly material_stored_u:bigint|string;
  readonly material_transferred_u:bigint|string;
  readonly material_returned_u:bigint|string;

}
const q=(v:bigint|string):bigint=>{
  if (typeof v!=="bigint" && typeof v!=="string") throw new Error("Decimal128禁止经number丢精度");
  if (typeof v==="string"&&!/^(0|[1-9][0-9]*)$/.test(v)) throw new Error("无效精确能量整数");
  const n=BigInt(v);if(n<0n)throw new Error("能量不能为负");return n;
};
export function energyBalance(row:EnergyRow):{externalReceived:bigint;accounted:bigint;balanced:boolean;materialExternal:bigint;materialAccounted:bigint;materialBalanced:boolean} {
  if(!row.enabled) throw new Error("此样本未启用生态，不能当作零能量验收");
  const externalReceived=q(row.received_q)-q(row.transferred_q);
  const accounted=q(row.stored_q)+q(row.dissipated_q)+q(row.exported_q)+q(row.returned_q);
  const activity=q(row.attack_q)+q(row.repair_q)+q(row.secretion_q)+q(row.synthesis_q)+q(row.continuous_q)+q(row.rule_q);
  const materialExternal=q(row.material_received_u)-q(row.material_transferred_u);
  const materialAccounted=q(row.material_stored_u)+q(row.material_returned_u);
  const materialBalanced=materialExternal>=0n&&materialExternal===q(row.material_external_u)&&materialExternal===materialAccounted;
  return {externalReceived,accounted,materialExternal,materialAccounted,materialBalanced,balanced:activity<=q(row.dissipated_q)&&externalReceived>=0n&&externalReceived===q(row.external_received_q)&&externalReceived===accounted};
}

/** 每个 run 的谱系摘要 */
export interface LineageSummary {
  readonly run: number;
  readonly genomes: number;
  readonly entities: number;
  readonly naturalBirths: number;
  readonly maxGeneration: number;
  /** 代数 → 基因组数 */
  readonly generations: ReadonlyMap<number, number>;
  /** 单位原型 → 基因组数 */
  readonly byUnit: ReadonlyMap<string, number>;
  /** 野生单位节点数（不是自然选择的证明） */
  readonly wild: number;
}

/** 按 run 汇总谱系 */
export function summarizeLineage(rows: Iterable<LineageRow>): Map<number, LineageSummary> {
  const acc = new Map<
    number,
    { genomes: number; entities:number; naturalBirths:number; digests:Set<string>; maxGeneration: number; generations: Map<number, number>; byUnit: Map<string, number>; wild: number }
  >();
  for (const r of rows) {
    let a = acc.get(r.run);
    if (!a) {
      a = { genomes:0,entities:0,naturalBirths:0,digests:new Set(),maxGeneration: 0, generations: new Map(), byUnit: new Map(), wild: 0 };
      acc.set(r.run, a);
    }
    a.entities += 1;
    a.digests.add(genomeKey(r.genome)); a.genomes=a.digests.size;
    if(r.origin==="mitosis") a.naturalBirths+=1;
    a.maxGeneration = Math.max(a.maxGeneration, r.generation);
    a.generations.set(r.generation, (a.generations.get(r.generation) ?? 0) + 1);
    a.byUnit.set(r.unit, (a.byUnit.get(r.unit) ?? 0) + 1);
    if (r.faction === WILD_FACTION) a.wild += 1;
  }
  const out = new Map<number, LineageSummary>();
  for (const [run, a] of [...acc].sort((x, y) => x[0] - y[0])) {
    const {digests:_,...summary}=a;
    out.set(run, { run, ...summary });
  }
  return out;
}

/**
 * 累计规范基因身份曲线：摘要含slug/代数，不冒充去标签后的遗传多样性测度。
 * `ticks` 须升序；返回与 `ticks` 等长的数组。
 */
export function genomeIdentityCurve(rows: Iterable<LineageRow>, run: number, ticks: readonly number[]): number[] {
  const firstByGenome=new Map<string,number>();
  for(const r of rows) if(r.run===run) {
    const key=genomeKey(r.genome); const tick=num(r.born_tick);
    firstByGenome.set(key,Math.min(firstByGenome.get(key)??tick,tick));
  }
  const firstSeen=[...firstByGenome.values()].sort((a,b)=>a-b);
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
  readonly meanNaturalBirths: number;
  readonly maxGeneration: number;
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
      meanNaturalBirths: mean((r) => r.natural_births),
      maxGeneration: Math.max(...g.map(r=>r.max_generation)),
      endReasons: reasons,
    };
  });
}

/** 某 run 某阵营的得分时间序列（Tick 升序；score 为 milli 定点） */
export function scoreSeries(rows: Iterable<MetricRow>, run: number, faction: number): Array<[number, number]> {
  return [...rows]
    .filter((r) => r.run === run && r.faction === faction && r.score_milli !== null)
    .map((r): [number, number] => [num(r.tick), num(r.score_milli!)])
    .sort((a, b) => a[0] - b[0]);
}

/** 四张结果表的完整列名（顺序冻结；测试直接解析 Rust `arrow_io.rs` 比对） */
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
    "natural_births",
    "max_generation",
    "model_digest",
    "parameter_fault",
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
    "parameter_entity",
    "parameter_component",
    "parameter_type",
    "parameter_value",
  ],
  lineage: ["run", "faction", "slug", "unit", "generation", "entity", "parent", "origin", "born_tick", "genome", "parent_genome", "allocated_q"],
  energy: ["enabled","run","tick","live_bodies","remains","natural_births","max_generation","received_q","stored_q","dissipated_q","exported_q","returned_q","transferred_q","external_received_q","state_root","attack_q","repair_q","secretion_q","synthesis_q","continuous_q","rule_q","material_received_u","material_external_u","material_stored_u","material_transferred_u","material_returned_u"],
} as const;
