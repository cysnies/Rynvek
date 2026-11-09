// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 设计圣经 §5.2–§5.4 平衡表的前端镜像（HUD 造价、建造预览与科技树面板）。
 *
 * 数值全部为 milli 定点整数，与 Rust `rynvek_rts::{buildings, units, tech}` 逐项一致：
 * `tests/arena/balance.json` 由 Rust 侧生成并断言，本包测试再与该夹具逐项比对。
 * 权威校验永远在服务端，前端表只用于预览与提示。
 */

import type { BuildingKind, ResourceAmounts, TechId, UnitKind } from "./protocol.js";
import { TerrainTileKind } from "./terrain.js";

const r = (biomass: number, flux: number, neurogel: number): ResourceAmounts => ({
  biomass: biomass * 1000,
  flux: flux * 1000,
  neurogel: neurogel * 1000,
});

export interface BuildingSpec {
  readonly kind: BuildingKind;
  readonly nameZh: string;
  readonly footprint: number;
  readonly cost: ResourceAmounts;
  readonly buildTicks: number;
  readonly hpMilli: number;
  readonly armor: number;
  readonly fluxPerSecondMilli: number;
  readonly visionTiles: number;
}

export interface UnitSpec {
  readonly kind: UnitKind;
  readonly nameZh: string;
  readonly supply: number;
  readonly cost: ResourceAmounts;
  readonly hatchTicks: number;
  readonly hpMilli: number;
  readonly shieldMilli: number;
  readonly armor: number;
  readonly speedMilliPerSecond: number;
  readonly visionTiles: number;
  readonly unlockedBy: TechId | null;
}

export interface TechSpec {
  readonly id: TechId;
  readonly nameZh: string;
  readonly tier: 1 | 2 | 3;
  readonly researchSite: BuildingKind;
  readonly requiresBuildings: readonly BuildingKind[];
  readonly requiresTechs: readonly TechId[];
  readonly cost: ResourceAmounts;
  readonly researchTicks: number;
}

// prettier-ignore
export const BUILDING_SPECS: readonly BuildingSpec[] = [
  { kind: "bld.nexus_core", nameZh: "神经主脑核心", footprint: 4, cost: r(0, 0, 0), buildTicks: 0, hpMilli: 4_000_000, armor: 15, fluxPerSecondMilli: 30_000, visionTiles: 24 },
  { kind: "bld.biomass_harvester", nameZh: "生物质萃取器", footprint: 2, cost: r(120, 0, 0), buildTicks: 150, hpMilli: 450_000, armor: 3, fluxPerSecondMilli: -6_000, visionTiles: 8 },
  { kind: "bld.flux_reactor", nameZh: "通量反应堆", footprint: 3, cost: r(200, 0, 20), buildTicks: 240, hpMilli: 750_000, armor: 5, fluxPerSecondMilli: 60_000, visionTiles: 8 },
  { kind: "bld.synapse_refinery", nameZh: "突触精炼厂", footprint: 3, cost: r(250, 0, 0), buildTicks: 300, hpMilli: 800_000, armor: 5, fluxPerSecondMilli: -25_000, visionTiles: 8 },
  { kind: "bld.spawning_vat", nameZh: "神经孵化槽", footprint: 3, cost: r(180, 0, 40), buildTicks: 210, hpMilli: 900_000, armor: 6, fluxPerSecondMilli: -15_000, visionTiles: 8 },
  { kind: "bld.signal_beacon", nameZh: "神经调制信标", footprint: 2, cost: r(100, 0, 30), buildTicks: 120, hpMilli: 400_000, armor: 2, fluxPerSecondMilli: -12_000, visionTiles: 8 },
  { kind: "bld.pulse_turret", nameZh: "脉冲防御塔", footprint: 2, cost: r(150, 0, 15), buildTicks: 150, hpMilli: 650_000, armor: 8, fluxPerSecondMilli: -10_000, visionTiles: 12 },
  { kind: "bld.mutation_crucible", nameZh: "演化坩埚", footprint: 3, cost: r(300, 0, 120), buildTicks: 360, hpMilli: 1_000_000, armor: 8, fluxPerSecondMilli: -40_000, visionTiles: 8 },
];

// prettier-ignore
export const UNIT_SPECS: readonly UnitSpec[] = [
  { kind: "unit.forager", nameZh: "采集工蜂簇", supply: 1, cost: r(40, 20, 0), hatchTicks: 90, hpMilli: 120_000, shieldMilli: 0, armor: 1, speedMilliPerSecond: 4_500, visionTiles: 8, unlockedBy: null },
  { kind: "unit.striker", nameZh: "突击刺胞簇", supply: 2, cost: r(65, 30, 10), hatchTicks: 120, hpMilli: 200_000, shieldMilli: 0, armor: 3, speedMilliPerSecond: 5_200, visionTiles: 10, unlockedBy: null },
  { kind: "unit.carapace", nameZh: "重甲盾簇", supply: 4, cost: r(130, 50, 25), hatchTicks: 180, hpMilli: 620_000, shieldMilli: 200_000, armor: 12, speedMilliPerSecond: 2_800, visionTiles: 9, unlockedBy: "tech.multicellular_chitin" },
  { kind: "unit.spitter", nameZh: "腐蚀孢子簇", supply: 3, cost: r(90, 40, 30), hatchTicks: 150, hpMilli: 160_000, shieldMilli: 0, armor: 2, speedMilliPerSecond: 3_600, visionTiles: 12, unlockedBy: "tech.multicellular_chitin" },
  { kind: "unit.weaver", nameZh: "突触编织者", supply: 4, cost: r(110, 60, 55), hatchTicks: 180, hpMilli: 240_000, shieldMilli: 80_000, armor: 4, speedMilliPerSecond: 4_000, visionTiles: 11, unlockedBy: "tech.gap_junction_sync" },
  { kind: "unit.leviathan", nameZh: "巨型神经母体", supply: 10, cost: r(350, 150, 140), hatchTicks: 360, hpMilli: 1_500_000, shieldMilli: 400_000, armor: 16, speedMilliPerSecond: 2_200, visionTiles: 15, unlockedBy: "tech.morphogenesis_apex" },
];

// prettier-ignore
export const TECH_SPECS: readonly TechSpec[] = [
  { id: "tech.chemotaxis_amp", nameZh: "趋化受体增敏", tier: 1, researchSite: "bld.synapse_refinery", requiresBuildings: ["bld.spawning_vat"], requiresTechs: [], cost: r(150, 100, 30), researchTicks: 450 },
  { id: "tech.flux_capacitance", nameZh: "超导通量电容", tier: 1, researchSite: "bld.synapse_refinery", requiresBuildings: ["bld.flux_reactor"], requiresTechs: [], cost: r(200, 150, 40), researchTicks: 600 },
  { id: "tech.kinetic_resonance", nameZh: "高频脉冲聚焦", tier: 1, researchSite: "bld.synapse_refinery", requiresBuildings: ["bld.pulse_turret"], requiresTechs: [], cost: r(180, 120, 50), researchTicks: 600 },
  { id: "tech.multicellular_chitin", nameZh: "多细胞几丁质装甲", tier: 2, researchSite: "bld.mutation_crucible", requiresBuildings: ["bld.mutation_crucible"], requiresTechs: [], cost: r(250, 200, 80), researchTicks: 750 },
  { id: "tech.hebbian_plasticity", nameZh: "四因子可塑性激活", tier: 2, researchSite: "bld.mutation_crucible", requiresBuildings: ["bld.mutation_crucible"], requiresTechs: [], cost: r(300, 250, 100), researchTicks: 900 },
  { id: "tech.gap_junction_sync", nameZh: "间隙连接电耦合", tier: 2, researchSite: "bld.mutation_crucible", requiresBuildings: [], requiresTechs: ["tech.hebbian_plasticity"], cost: r(320, 250, 120), researchTicks: 900 },
  { id: "tech.morphogenesis_apex", nameZh: "高阶形态发生学", tier: 3, researchSite: "bld.mutation_crucible", requiresBuildings: [], requiresTechs: ["tech.gap_junction_sync"], cost: r(500, 400, 220), researchTicks: 1200 },
  { id: "tech.metabolic_superconduct", nameZh: "超导代谢闭环", tier: 3, researchSite: "bld.mutation_crucible", requiresBuildings: ["bld.mutation_crucible"], requiresTechs: ["tech.flux_capacitance"], cost: r(400, 350, 180), researchTicks: 1050 },
  { id: "tech.directed_speciation", nameZh: "定向物种跃迁", tier: 3, researchSite: "bld.mutation_crucible", requiresBuildings: [], requiresTechs: ["tech.morphogenesis_apex"], cost: r(450, 400, 250), researchTicks: 1200 },
];

/** 开局资源 `500 Biomass / 300 Flux / 100 Neurogel` */
export const STARTER_RESOURCES: ResourceAmounts = r(500, 300, 100);
/** 主脑存储上限 */
export const NEXUS_STORAGE_CAP: ResourceAmounts = r(2000, 1000, 500);
/** 每座建成的孵化槽提供的人口上限 */
export const VAT_SUPPLY_BONUS = 24;
/** 孵化槽队列上限 */
export const VAT_QUEUE_LIMIT = 5;

const BUILDINGS = new Map(BUILDING_SPECS.map((b) => [b.kind, b]));
const UNITS = new Map(UNIT_SPECS.map((u) => [u.kind, u]));
const TECHS = new Map(TECH_SPECS.map((t) => [t.id, t]));

export function buildingSpec(kind: BuildingKind): BuildingSpec {
  return BUILDINGS.get(kind)!;
}

export function unitSpec(kind: UnitKind): UnitSpec {
  return UNITS.get(kind)!;
}

export function techSpec(id: TechId): TechSpec {
  return TECHS.get(id)!;
}

/** 地形是否允许建造：墙不可建；热液裂隙只允许通量反应堆（与 Rust `allows_building` 一致） */
export function allowsBuilding(terrain: TerrainTileKind, kind: BuildingKind): boolean {
  if (terrain === TerrainTileKind.ImperviousWall) return false;
  if (terrain === TerrainTileKind.AcidicFissure) return kind === "bld.flux_reactor";
  return true;
}

/** 余额是否足以支付 */
export function canAfford(balance: ResourceAmounts, cost: ResourceAmounts): boolean {
  return balance.biomass >= cost.biomass && balance.flux >= cost.flux && balance.neurogel >= cost.neurogel;
}

/** 科技是否满足前置（拥有已建成的前置建筑且已研发前置科技） */
export function techAvailable(
  id: TechId,
  researched: readonly TechId[],
  completedBuildings: readonly BuildingKind[],
): boolean {
  const t = techSpec(id);
  if (researched.includes(id)) return false;
  return (
    t.requiresBuildings.every((b) => completedBuildings.includes(b)) &&
    t.requiresTechs.every((x) => researched.includes(x))
  );
}

/** 单位原型是否已解锁 */
export function unitUnlocked(kind: UnitKind, researched: readonly TechId[]): boolean {
  const need = unitSpec(kind).unlockedBy;
  return need === null || researched.includes(need);
}
