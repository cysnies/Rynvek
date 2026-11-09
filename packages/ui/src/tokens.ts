// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `--rv-*` 设计令牌（项目文档 6.1、GAP-01 统一前缀、GAP-14 冻结的三主题令牌表）。
 *
 * 三主题：
 * - `neutral-dark`：缺省主题，深石墨碳黑背景 `#090B0A`、1px 细边框 `#303A34`、工业青柠强调色 `#A3D65C`；
 * - `scientific-light`：科研浅色主题，用于论文截图与明亮环境；
 * - `okabe-ito`：色盲安全主题，在 Neutral Dark 的中性色基础上，把全部语义色与可视化色换成 Okabe-Ito 八色表。
 *
 * 规范只冻结了品牌三色与主题名称。其余令牌值按 WCAG 2.2 对比度最佳实践补全（ERRATA E-017）：
 * 正文文本与背景 ≥ 7:1，次要文本 ≥ 4.5:1，强调色 / 焦点环与背景 ≥ 3:1。测试会逐主题断言这些下限。
 */

/** 令牌所属的主题层（五层主题隔离，见 `theme.ts`） */
export type TokenLayer = "application" | "world" | "faction" | "visualization";

/** 应用 UI 层令牌（外层工作区面板） */
export const APPLICATION_TOKENS = [
  "--rv-color-bg",
  "--rv-color-surface",
  "--rv-color-surface-raised",
  "--rv-color-overlay",
  "--rv-color-border",
  "--rv-color-border-strong",
  "--rv-color-text",
  "--rv-color-text-muted",
  "--rv-color-text-inverse",
  "--rv-color-accent",
  "--rv-color-accent-hover",
  "--rv-color-accent-text",
  "--rv-color-focus-ring",
  "--rv-color-success",
  "--rv-color-warning",
  "--rv-color-danger",
  "--rv-color-info",
  "--rv-font-sans",
  "--rv-font-mono",
  "--rv-font-size-xs",
  "--rv-font-size-sm",
  "--rv-font-size-md",
  "--rv-font-size-lg",
  "--rv-line-height",
  "--rv-space-1",
  "--rv-space-2",
  "--rv-space-3",
  "--rv-space-4",
  "--rv-space-6",
  "--rv-space-8",
  "--rv-radius-sm",
  "--rv-radius-md",
  "--rv-radius-lg",
  "--rv-border-width",
  "--rv-duration-fast",
  "--rv-duration-normal",
  "--rv-ease",
  "--rv-z-dock",
  "--rv-z-overlay",
  "--rv-z-palette",
] as const;

/** 世界生物群系层令牌（WebGPU 画布外围与地形预览） */
export const WORLD_TOKENS = [
  "--rv-world-bg",
  "--rv-world-grid",
  "--rv-world-outline",
  "--rv-world-fog",
] as const;

/** 阵营层令牌：Okabe-Ito 去掉黑色后的 7 色（与 `rynvek-render::WorldPalette::faction` 一致） */
export const FACTION_TOKENS = [
  "--rv-faction-1",
  "--rv-faction-2",
  "--rv-faction-3",
  "--rv-faction-4",
  "--rv-faction-5",
  "--rv-faction-6",
  "--rv-faction-7",
] as const;

/** 可视化层令牌：场热力色阶与神经可视化 */
export const VISUALIZATION_TOKENS = [
  "--rv-heat-low",
  "--rv-heat-mid",
  "--rv-heat-high",
  "--rv-neural-excitatory",
  "--rv-neural-inhibitory",
  "--rv-neural-spike",
  "--rv-neural-sync",
] as const;

export type ApplicationToken = (typeof APPLICATION_TOKENS)[number];
export type WorldToken = (typeof WORLD_TOKENS)[number];
export type FactionToken = (typeof FACTION_TOKENS)[number];
export type VisualizationToken = (typeof VISUALIZATION_TOKENS)[number];
export type RvToken = ApplicationToken | WorldToken | FactionToken | VisualizationToken;

export const ALL_TOKENS: readonly RvToken[] = [
  ...APPLICATION_TOKENS,
  ...WORLD_TOKENS,
  ...FACTION_TOKENS,
  ...VISUALIZATION_TOKENS,
];

/** 令牌 → 所属层 */
export function tokenLayer(token: RvToken): TokenLayer {
  if ((WORLD_TOKENS as readonly string[]).includes(token)) return "world";
  if ((FACTION_TOKENS as readonly string[]).includes(token)) return "faction";
  if ((VISUALIZATION_TOKENS as readonly string[]).includes(token)) return "visualization";
  return "application";
}

export function isRvToken(name: string): name is RvToken {
  return (ALL_TOKENS as readonly string[]).includes(name);
}

/** 完整令牌表（每个主题必须为全部令牌赋值） */
export type TokenTable = Readonly<Record<RvToken, string>>;

/** 主题名称 */
export const THEME_NAMES = ["neutral-dark", "scientific-light", "okabe-ito"] as const;
export type ThemeName = (typeof THEME_NAMES)[number];

/** Okabe-Ito 色盲安全八色表（黑、橙、天蓝、蓝绿、黄、蓝、朱红、红紫），与 `rynvek-render::OKABE_ITO` 一致 */
export const OKABE_ITO = [
  "#000000",
  "#E69F00",
  "#56B4E9",
  "#009E73",
  "#F0E442",
  "#0072B2",
  "#D55E00",
  "#CC79A7",
] as const;

/** 品牌三色（项目文档 6.1 冻结） */
export const BRAND = {
  background: "#090B0A",
  border: "#303A34",
  accent: "#A3D65C",
} as const;

/** 与主题无关的版式、间距、圆角、动效与层级令牌 */
const STRUCTURE = {
  "--rv-font-sans": '"Geist Sans", "Noto Sans SC", ui-sans-serif, system-ui, sans-serif',
  "--rv-font-mono": '"Geist Mono", "Noto Sans SC", ui-monospace, SFMono-Regular, monospace',
  "--rv-font-size-xs": "11px",
  "--rv-font-size-sm": "12px",
  "--rv-font-size-md": "14px",
  "--rv-font-size-lg": "16px",
  "--rv-line-height": "1.5",
  "--rv-space-1": "4px",
  "--rv-space-2": "8px",
  "--rv-space-3": "12px",
  "--rv-space-4": "16px",
  "--rv-space-6": "24px",
  "--rv-space-8": "32px",
  "--rv-radius-sm": "2px",
  "--rv-radius-md": "4px",
  "--rv-radius-lg": "6px",
  "--rv-border-width": "1px",
  "--rv-duration-fast": "80ms",
  "--rv-duration-normal": "160ms",
  "--rv-ease": "cubic-bezier(0.2, 0, 0, 1)",
  "--rv-z-dock": "10",
  "--rv-z-overlay": "100",
  "--rv-z-palette": "1000",
} as const;

/** 阵营色在三主题中一致（阵营辨识不随应用主题变化） */
const FACTIONS = {
  "--rv-faction-1": OKABE_ITO[1],
  "--rv-faction-2": OKABE_ITO[2],
  "--rv-faction-3": OKABE_ITO[3],
  "--rv-faction-4": OKABE_ITO[4],
  "--rv-faction-5": OKABE_ITO[5],
  "--rv-faction-6": OKABE_ITO[6],
  "--rv-faction-7": OKABE_ITO[7],
} as const;

const NEUTRAL_DARK: TokenTable = {
  ...STRUCTURE,
  ...FACTIONS,
  "--rv-color-bg": BRAND.background,
  "--rv-color-surface": "#111412",
  "--rv-color-surface-raised": "#171B18",
  "--rv-color-overlay": "#1D221F",
  "--rv-color-border": BRAND.border,
  "--rv-color-border-strong": "#4A564E",
  "--rv-color-text": "#E6EBE7",
  "--rv-color-text-muted": "#9AA59E",
  "--rv-color-text-inverse": BRAND.background,
  "--rv-color-accent": BRAND.accent,
  "--rv-color-accent-hover": "#B7E37A",
  "--rv-color-accent-text": BRAND.background,
  "--rv-color-focus-ring": BRAND.accent,
  "--rv-color-success": "#7BC86C",
  "--rv-color-warning": "#E5B53B",
  "--rv-color-danger": "#F06A60",
  "--rv-color-info": "#58A6D8",
  "--rv-world-bg": BRAND.background,
  "--rv-world-grid": BRAND.border,
  "--rv-world-outline": "#050605",
  "--rv-world-fog": "rgba(9, 11, 10, 0.75)",
  "--rv-heat-low": "#0072B2",
  "--rv-heat-mid": "#7FAB7A",
  "--rv-heat-high": "#F0E442",
  "--rv-neural-excitatory": BRAND.accent,
  "--rv-neural-inhibitory": "#E5534B",
  "--rv-neural-spike": "#F4F7F5",
  "--rv-neural-sync": "#56B4E9",
};

const SCIENTIFIC_LIGHT: TokenTable = {
  ...STRUCTURE,
  ...FACTIONS,
  "--rv-color-bg": "#F7F8F6",
  "--rv-color-surface": "#FFFFFF",
  "--rv-color-surface-raised": "#FFFFFF",
  "--rv-color-overlay": "#EEF1ED",
  "--rv-color-border": "#D3DAD5",
  "--rv-color-border-strong": "#8C978F",
  "--rv-color-text": "#111412",
  "--rv-color-text-muted": "#535E57",
  "--rv-color-text-inverse": "#F7F8F6",
  // 青柠在浅底上对比度不足，改用同色相的深橄榄绿
  "--rv-color-accent": "#4C7A1C",
  "--rv-color-accent-hover": "#3E6516",
  "--rv-color-accent-text": "#FFFFFF",
  "--rv-color-focus-ring": "#4C7A1C",
  "--rv-color-success": "#2F7D32",
  "--rv-color-warning": "#8A6100",
  "--rv-color-danger": "#B42318",
  "--rv-color-info": "#1F6FB2",
  "--rv-world-bg": "#F7F8F6",
  "--rv-world-grid": "#D3DAD5",
  "--rv-world-outline": "#2A312D",
  "--rv-world-fog": "rgba(247, 248, 246, 0.75)",
  "--rv-heat-low": "#0072B2",
  "--rv-heat-mid": "#7FAB7A",
  "--rv-heat-high": "#F0E442",
  "--rv-neural-excitatory": "#4C7A1C",
  "--rv-neural-inhibitory": "#B42318",
  "--rv-neural-spike": "#111412",
  "--rv-neural-sync": "#0072B2",
};

const OKABE_ITO_THEME: TokenTable = {
  ...NEUTRAL_DARK,
  "--rv-color-accent": OKABE_ITO[1],
  "--rv-color-accent-hover": "#F0B53A",
  "--rv-color-accent-text": BRAND.background,
  "--rv-color-focus-ring": OKABE_ITO[4],
  "--rv-color-success": OKABE_ITO[3],
  "--rv-color-warning": OKABE_ITO[4],
  "--rv-color-danger": OKABE_ITO[6],
  "--rv-color-info": OKABE_ITO[2],
  "--rv-heat-low": OKABE_ITO[5],
  "--rv-heat-mid": OKABE_ITO[2],
  "--rv-heat-high": OKABE_ITO[4],
  "--rv-neural-excitatory": OKABE_ITO[3],
  "--rv-neural-inhibitory": OKABE_ITO[6],
  "--rv-neural-spike": OKABE_ITO[4],
  "--rv-neural-sync": OKABE_ITO[2],
};

/** 三主题完整令牌表 */
export const THEMES: Readonly<Record<ThemeName, TokenTable>> = {
  "neutral-dark": NEUTRAL_DARK,
  "scientific-light": SCIENTIFIC_LIGHT,
  "okabe-ito": OKABE_ITO_THEME,
};

/** CSS 中引用令牌：`var(--rv-color-accent)` */
export function cssVar(token: RvToken, fallback?: string): string {
  return fallback === undefined ? `var(${token})` : `var(${token}, ${fallback})`;
}
