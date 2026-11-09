// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 小地图光栅化（RGBA8，行主序，`y = 0` 在第一行）：地形底色 → 迷雾（未探索全黑、已探索压暗 50%）→ 实体点。
 * 阵营色取 Okabe-Ito（与 `--rv-faction-*` 及 `rynvek-render::OKABE_ITO` 一致）：玩家 `i` 用第 `i + 1` 色，野生虫潮用第 7 色。
 */

import { buildingSpec, rawToTile, TerrainTileKind, type FactionObservation, type TerrainGrid } from "@rynvek/client";
import { OKABE_ITO } from "@rynvek/ui";

import { VISIBILITY_CELL_TILES } from "./placement.js";

/** 五种地形底色（工业像素风格的低饱和色） */
export const TERRAIN_COLORS: Readonly<Record<TerrainTileKind, readonly [number, number, number]>> = {
  [TerrainTileKind.OpenSubstrate]: [0x2a, 0x30, 0x2c],
  [TerrainTileKind.ConductiveBasalt]: [0x3c, 0x42, 0x55],
  [TerrainTileKind.NutrientMarsh]: [0x3f, 0x5a, 0x2e],
  [TerrainTileKind.AcidicFissure]: [0x7a, 0x3b, 0x1f],
  [TerrainTileKind.ImperviousWall]: [0x0f, 0x11, 0x10],
};

const WILD_FACTION = 255;

function hexRgb(hex: string): [number, number, number] {
  const v = Number.parseInt(hex.slice(1), 16);
  return [(v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff];
}

export function factionColor(faction: number): [number, number, number] {
  const idx = faction === WILD_FACTION ? 7 : (faction % 7) + 1;
  return hexRgb(OKABE_ITO[idx]!);
}

export interface Minimap {
  readonly width: number;
  readonly height: number;
  readonly rgba: Uint8ClampedArray;
}

/** 按 `scale` Tile / 像素降采样（取每块左下角 Tile） */
export function renderMinimap(terrain: TerrainGrid, obs: FactionObservation, scale = 2): Minimap {
  const w = Math.ceil(terrain.width / scale);
  const h = Math.ceil(terrain.height / scale);
  const rgba = new Uint8ClampedArray(w * h * 4);
  const put = (px: number, py: number, c: readonly [number, number, number]) => {
    if (px < 0 || py < 0 || px >= w || py >= h) return;
    const i = (py * w + px) * 4;
    rgba[i] = c[0];
    rgba[i + 1] = c[1];
    rgba[i + 2] = c[2];
    rgba[i + 3] = 255;
  };
  for (let py = 0; py < h; py++) {
    for (let px = 0; px < w; px++) {
      const x = px * scale;
      const y = py * scale;
      const cell = obs.visibility[Math.floor(y / VISIBILITY_CELL_TILES) * obs.visibility_width + Math.floor(x / VISIBILITY_CELL_TILES)];
      const base = TERRAIN_COLORS[terrain.at(x, y)];
      put(px, py, cell === 1 ? base : cell === 2 ? [base[0] >> 1, base[1] >> 1, base[2] >> 1] : [0, 0, 0]);
    }
  }
  for (const g of obs.ghosts) put(Math.floor(g.origin[0] / scale), Math.floor(g.origin[1] / scale), factionColor(g.faction).map((v) => v >> 1) as [number, number, number]);
  for (const b of obs.buildings) {
    const f = buildingSpec(b.kind).footprint;
    for (let y = b.origin[1]; y < b.origin[1] + f; y += scale)
      for (let x = b.origin[0]; x < b.origin[0] + f; x += scale) put(Math.floor(x / scale), Math.floor(y / scale), factionColor(b.faction));
  }
  for (const u of obs.units) {
    const [x, y] = rawToTile(u.position);
    put(Math.floor(x / scale), Math.floor(y / scale), factionColor(u.faction));
  }
  return { width: w, height: h, rgba };
}

/** 读取像素（测试与拾取用） */
export function pixel(m: Minimap, px: number, py: number): [number, number, number, number] {
  const i = (py * m.width + px) * 4;
  return [m.rgba[i]!, m.rgba[i + 1]!, m.rgba[i + 2]!, m.rgba[i + 3]!];
}
