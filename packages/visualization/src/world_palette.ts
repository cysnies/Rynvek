// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 由解析后的主题令牌表生成 GPU 世界画布色板（字段与 `rynvek-render::WorldPalette` 一一对应，值为 `Rgba8::packed`）。

import type { TokenTable } from "@rynvek/ui";
import { factionColorFromTheme, packRgba8 } from "./color_scales.js";

export interface PackedWorldPalette {
  readonly background: number;
  readonly gridLine: number;
  readonly accent: number;
  readonly heatLow: number;
  readonly heatHigh: number;
  readonly fog: number;
  /** 阵营 0..6 */
  readonly factions: readonly number[];
}

/** 迷雾不透明度：与 Rust `NEUTRAL_DARK.fog` 的 0xC0 一致 */
export const FOG_ALPHA = 0xc0;

export function packedWorldPalette(table: TokenTable): PackedWorldPalette {
  return {
    background: packRgba8(table["--rv-world-bg"]),
    gridLine: packRgba8(table["--rv-world-grid"]),
    accent: packRgba8(table["--rv-color-accent"]),
    heatLow: packRgba8(table["--rv-heat-low"]),
    heatHigh: packRgba8(table["--rv-heat-high"]),
    // `--rv-world-fog` 是给 DOM 用的 rgba()，GPU 侧以世界背景色叠固定不透明度
    fog: packRgba8(table["--rv-world-bg"], FOG_ALPHA),
    factions: Array.from({ length: 7 }, (_, i) => packRgba8(factionColorFromTheme(table, i))),
  };
}
