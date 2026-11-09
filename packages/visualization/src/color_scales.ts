// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 科学可视化色标：热力图、阵营色、兴奋/抑制突触色与同步度。
// 热力图与 GPU 着色器 `mix(unpack4x8unorm(low), unpack4x8unorm(high), v)` 相同：8 位 sRGB 空间线性插值。

import { OKABE_ITO, mixHex, parseHex, toHex, type TokenTable } from "@rynvek/ui";

/** 默认热力图端点（项目文档 / ERRATA E-015） */
export const HEAT_LOW = "#0072B2";
export const HEAT_HIGH = "#F0E442";

/** 夹紧到 [0, 1]；NaN 视为 0 */
export function clamp01(v: number): number {
  return Number.isNaN(v) ? 0 : Math.max(0, Math.min(1, v));
}

/** 热力图取色，返回 [0, 1] 浮点 RGB（与 GPU 逐位一致的插值公式） */
export function heatRampRgb(t: number, low = HEAT_LOW, high = HEAT_HIGH): [number, number, number] {
  const v = clamp01(t);
  const a = parseHex(low);
  const b = parseHex(high);
  const lerp = (x: number, y: number): number => (x / 255) * (1 - v) + (y / 255) * v;
  return [lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b)];
}

/** 热力图取色（十六进制，供图例与 DOM 叠加层使用） */
export function heatRampHex(t: number, low = HEAT_LOW, high = HEAT_HIGH): string {
  return mixHex(low, high, clamp01(t));
}

/** 从主题令牌表取热力图端点 */
export function heatStopsFromTheme(table: TokenTable): { low: string; mid: string; high: string } {
  return { low: table["--rv-heat-low"], mid: table["--rv-heat-mid"], high: table["--rv-heat-high"] };
}

/** 阵营色：0 号起循环使用 Okabe-Ito 1..7（跳过黑色），与 `WorldPalette::faction` 一致 */
export function factionColor(index: number): string {
  if (!Number.isInteger(index) || index < 0) throw new RangeError(`阵营索引必须是非负整数：${index}`);
  return OKABE_ITO[1 + (index % 7)] as string;
}

/** 主题中的阵营色（`--rv-faction-1..7`，索引 0 对应 `--rv-faction-1`） */
export function factionColorFromTheme(table: TokenTable, index: number): string {
  if (!Number.isInteger(index) || index < 0) throw new RangeError(`阵营索引必须是非负整数：${index}`);
  return table[`--rv-faction-${(index % 7) + 1}` as "--rv-faction-1"];
}

/**
 * 突触权重着色：正（兴奋）取 `--rv-neural-excitatory`，负（抑制）取 `--rv-neural-inhibitory`，
 * 绝对值经 `wMax` 归一化后作为不透明度，零权重完全透明。
 */
export function synapseStyle(weight: number, wMax: number, table: TokenTable): { color: string; alpha: number } {
  const color = weight < 0 ? table["--rv-neural-inhibitory"] : table["--rv-neural-excitatory"];
  const alpha = wMax > 0 ? clamp01(Math.abs(weight) / wMax) : 0;
  return { color, alpha };
}

/** Kuramoto 序参量 r = |mean(e^{iθ})|，取值 [0, 1]；空集为 0 */
export function kuramotoOrder(phases: ArrayLike<number>): number {
  const n = phases.length;
  if (n === 0) return 0;
  let re = 0;
  let im = 0;
  for (let i = 0; i < n; i++) {
    const p = phases[i] ?? 0;
    re += Math.cos(p);
    im += Math.sin(p);
  }
  return Math.min(1, Math.hypot(re, im) / n);
}

/** 同步脉冲环颜色：序参量越高越接近 `--rv-neural-sync`，低同步时退向网格色 */
export function syncRingColor(r: number, table: TokenTable): string {
  return mixHex(table["--rv-world-grid"], table["--rv-neural-sync"], clamp01(r));
}

/** 打包为 `rynvek-render::Rgba8::packed` 的 u32（小端 RGBA） */
export function packRgba8(hex: string, alpha = 255): number {
  const { r, g, b } = parseHex(hex);
  return (r | (g << 8) | (b << 16) | ((alpha & 0xff) << 24)) >>> 0;
}

/** 规范化十六进制写法（大写 #RRGGBB） */
export function normalizeHex(hex: string): string {
  return toHex(parseHex(hex));
}
