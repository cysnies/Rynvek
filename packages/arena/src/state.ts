// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Arena 界面状态（选择、编队、摄像机、建造模式、面板开关）与由观测派生的 HUD 视图模型。
 * 纯数据、无 DOM 依赖，渲染层（WebGPU 画布与 React 面板）只读它。
 */

import {
  buildingSpec,
  milliToWhole,
  rawToFloat,
  techAvailable,
  unitUnlocked,
  TECH_SPECS,
  UNIT_SPECS,
  type BuildingKind,
  type FactionObservation,
  type MatchOutcome,
  type TechId,
  type UnitKind,
} from "@rynvek/client";

export interface Camera {
  /** 视口中心（世界 Tile 坐标） */
  x: number;
  y: number;
  zoom: number;
}

export const CAMERA_PAN_TILES = 8;
export const CAMERA_ZOOM_MIN = 0.25;
export const CAMERA_ZOOM_MAX = 4;

export class ArenaUiState {
  selection = new Set<number>();
  selectedBuilding: number | null = null;
  readonly controlGroups = new Map<number, number[]>();
  camera: Camera = { x: 0, y: 0, zoom: 1 };
  buildMenuOpen = false;
  buildMode: BuildingKind | null = null;
  /** 建造虚影朝向（0..3 × 90°）：规范建筑均为正方形占地，朝向只影响外观（ERRATA E-036） */
  buildRotation = 0;
  beaconPanelOpen = false;
  minimapVisible = true;
  inspectorVisible = false;
  neuralOverlay = false;
  fieldOverlay = false;
  commandPaletteOpen = false;
  /** 最近的提示（错误码与说明），最多 8 条 */
  readonly notices: { code: number | null; text: string }[] = [];

  notify(code: number | null, text: string): void {
    this.notices.push({ code, text });
    if (this.notices.length > 8) this.notices.shift();
  }

  /** 剔除已不存在的选中单位 */
  prune(obs: FactionObservation): void {
    const alive = new Set(obs.units.filter((u) => u.faction === obs.faction).map((u) => u.id));
    for (const id of [...this.selection]) if (!alive.has(id)) this.selection.delete(id);
    if (this.selectedBuilding !== null && !obs.buildings.some((b) => b.id === this.selectedBuilding)) this.selectedBuilding = null;
  }
}

export interface HudResource {
  readonly amount: number;
  readonly capacity: number;
}

export interface HudView {
  readonly tick: number;
  readonly seconds: number;
  readonly biomass: HudResource;
  readonly flux: HudResource;
  readonly neurogel: HudResource;
  readonly supply: { readonly used: number; readonly cap: number };
  readonly unitCounts: Readonly<Partial<Record<UnitKind, number>>>;
  readonly buildingCounts: Readonly<Partial<Record<BuildingKind, number>>>;
  readonly underConstruction: number;
  readonly techs: readonly TechId[];
  readonly availableTechs: readonly TechId[];
  readonly hatchable: readonly UnitKind[];
  readonly scoreMilli: number;
  readonly pveWave: number | null;
  readonly outcome: MatchOutcome | null;
}

/** 由观测派生 HUD（资源以整单位显示，向零截断） */
export function hudView(obs: FactionObservation): HudView {
  const own = obs.faction;
  const unitCounts: Partial<Record<UnitKind, number>> = {};
  for (const u of obs.units) if (u.faction === own) unitCounts[u.kind] = (unitCounts[u.kind] ?? 0) + 1;
  const buildingCounts: Partial<Record<BuildingKind, number>> = {};
  let underConstruction = 0;
  const completed: BuildingKind[] = [];
  for (const b of obs.buildings) {
    if (b.faction !== own) continue;
    buildingCounts[b.kind] = (buildingCounts[b.kind] ?? 0) + 1;
    if (b.complete) completed.push(b.kind);
    else underConstruction++;
  }
  const res = (k: "biomass" | "flux" | "neurogel"): HudResource => ({
    amount: milliToWhole(obs.ledger.balance[k]),
    capacity: milliToWhole(obs.ledger.capacity[k]),
  });
  return {
    tick: obs.tick,
    seconds: Math.floor(obs.tick / 30),
    biomass: res("biomass"),
    flux: res("flux"),
    neurogel: res("neurogel"),
    supply: { used: obs.supply[0], cap: obs.supply[1] },
    unitCounts,
    buildingCounts,
    underConstruction,
    techs: obs.techs,
    availableTechs: TECH_SPECS.filter((t) => techAvailable(t.id, obs.techs, completed)).map((t) => t.id),
    hatchable: UNIT_SPECS.filter((u) => unitUnlocked(u.kind, obs.techs)).map((u) => u.kind),
    scoreMilli: obs.score_milli,
    pveWave: obs.pve_wave,
    outcome: obs.outcome,
  };
}

/** 本阵营主脑中心（世界 Tile 坐标） */
export function nexusCenter(obs: FactionObservation): [number, number] | null {
  const n = obs.buildings.find((b) => b.faction === obs.faction && b.kind === "bld.nexus_core");
  if (!n) return null;
  const f = buildingSpec(n.kind).footprint;
  return [n.origin[0] + f / 2, n.origin[1] + f / 2];
}

/** 单位浮点位置 */
export function unitPosition(u: { readonly position: readonly [number, number] }): [number, number] {
  return rawToFloat(u.position);
}
