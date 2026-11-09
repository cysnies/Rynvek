// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 建造虚影预览：在客户端复现服务端 `ArenaWorld::validate_placement` 的四条规则
 * （越界 → 地形 → 与已知建筑重叠 → 落点在当前视野内），用于虚影着色与自动选址。
 *
 * 客户端只拥有 `4×4` 聚合可见性格，因此 "在视野内" 以所在聚合格为 `1` 近似：预览可能比权威层乐观，
 * 不会更严格；最终以服务端 `0x8006` 为准（ERRATA E-036）。
 */

import {
  allowsBuilding,
  buildingSpec,
  canAfford,
  type BuildingKind,
  type FactionObservation,
  type TerrainGrid,
} from "@rynvek/client";

export type PlacementIssue = "out_of_bounds" | "terrain" | "overlap" | "not_visible" | "unaffordable" | "not_buildable";

/** 聚合可见性格边长（与 Rust `VISIBILITY_CELL_TILES` 一致） */
export const VISIBILITY_CELL_TILES = 4;

export function cellVisible(obs: FactionObservation, x: number, y: number): boolean {
  const c = Math.floor(y / VISIBILITY_CELL_TILES) * obs.visibility_width + Math.floor(x / VISIBILITY_CELL_TILES);
  return obs.visibility[c] === 1;
}

/** 已知建筑（实体 + 幽灵）占用的格子集合 */
export function occupiedTiles(obs: FactionObservation): Set<number> {
  const out = new Set<number>();
  const mark = (kind: BuildingKind, origin: readonly [number, number]) => {
    const f = buildingSpec(kind).footprint;
    for (let y = origin[1]; y < origin[1] + f; y++) for (let x = origin[0]; x < origin[0] + f; x++) out.add(y * 65536 + x);
  };
  for (const b of obs.buildings) mark(b.kind, b.origin);
  for (const g of obs.ghosts) mark(g.kind, g.origin);
  return out;
}

/** 预览一个落点的全部问题（空数组 = 预计合法） */
export function placementIssues(
  kind: BuildingKind,
  origin: readonly [number, number],
  terrain: TerrainGrid,
  obs: FactionObservation,
  occupied: Set<number> = occupiedTiles(obs),
): PlacementIssue[] {
  const issues: PlacementIssue[] = [];
  if (kind === "bld.nexus_core") issues.push("not_buildable");
  const f = buildingSpec(kind).footprint;
  const [ox, oy] = origin;
  if (ox < 0 || oy < 0 || ox + f > terrain.width || oy + f > terrain.height) return [...issues, "out_of_bounds"];
  let badTerrain = false;
  let overlap = false;
  let hidden = false;
  for (let y = oy; y < oy + f; y++) {
    for (let x = ox; x < ox + f; x++) {
      if (!allowsBuilding(terrain.at(x, y), kind)) badTerrain = true;
      if (occupied.has(y * 65536 + x)) overlap = true;
      if (!cellVisible(obs, x, y)) hidden = true;
    }
  }
  if (badTerrain) issues.push("terrain");
  if (overlap) issues.push("overlap");
  if (hidden) issues.push("not_visible");
  if (!canAfford(obs.ledger.balance, buildingSpec(kind).cost)) issues.push("unaffordable");
  return issues;
}

export interface PlacementSearch {
  /** 与中心至少保持的切比雪夫距离（避免贴脸堵路） */
  readonly minRing?: number;
  readonly maxRing?: number;
  /** 额外偏好，例如萃取器优先覆盖沼泽 */
  readonly accept?: (origin: readonly [number, number]) => boolean;
}

/**
 * 确定性选址：以 `center` 为中心按切比雪夫环由内向外、环内按 `(y, x)` 升序扫描，返回第一个预计合法的落点。
 * 同一观测下结果唯一，人类 UI 的 "自动放置" 与脚本 / Agent 共用。
 */
export function findPlacement(
  kind: BuildingKind,
  center: readonly [number, number],
  terrain: TerrainGrid,
  obs: FactionObservation,
  search: PlacementSearch = {},
): [number, number] | null {
  const occupied = occupiedTiles(obs);
  const minRing = search.minRing ?? 0;
  const maxRing = search.maxRing ?? 24;
  for (let ring = minRing; ring <= maxRing; ring++) {
    for (let dy = -ring; dy <= ring; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        if (Math.max(Math.abs(dx), Math.abs(dy)) !== ring) continue;
        const origin: [number, number] = [center[0] + dx, center[1] + dy];
        if (placementIssues(kind, origin, terrain, obs, occupied).length > 0) continue;
        if (search.accept && !search.accept(origin)) continue;
        return origin;
      }
    }
  }
  return null;
}
