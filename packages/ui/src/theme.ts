// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 五层主题隔离架构（GAP-14）：优先级由高到低为
 * `Accessibility > Visualization > Faction > World Biome > Application UI`。
 *
 * - 低四层各自只能覆盖本层命名空间内的令牌（例如阵营层只能写 `--rv-faction-*`），越界即抛错，
 *   防止世界包或视觉包借阵营色改写应用 UI；
 * - 无障碍层可以覆盖任何令牌，并且总是最后应用，保证色盲安全、减弱动效、高对比度等设置不会被其他层压住；
 * - 来自不可信内容（`.rview`）的覆盖值经过 `assertSafeCssValue` 校验，拒绝能逃逸声明块的字符。
 */

import { contrastRatio, isHexColor } from "./color.js";
import {
  ALL_TOKENS,
  OKABE_ITO,
  type RvToken,
  THEMES,
  THEME_NAMES,
  type ThemeName,
  type TokenLayer,
  type TokenTable,
  isRvToken,
  tokenLayer,
} from "./tokens.js";

/** 主题层（低 → 高） */
export const THEME_LAYER_ORDER = ["application", "world", "faction", "visualization", "accessibility"] as const;
export type ThemeLayerKind = (typeof THEME_LAYER_ORDER)[number];

export type TokenOverrides = Readonly<Partial<Record<RvToken, string>>>;

/** 一个主题层的覆盖 */
export interface ThemeLayer {
  readonly kind: ThemeLayerKind;
  /** 层来源（调试与报错用，如 `rview:rynvek.visual.industrial-default`） */
  readonly source: string;
  readonly tokens: TokenOverrides;
}

export class ThemeLayerError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ThemeLayerError";
  }
}

/** 拒绝可能逃逸 CSS 声明块或注入外部资源的值 */
export function assertSafeCssValue(token: string, value: string): void {
  if (value.length === 0 || value.length > 256) {
    throw new ThemeLayerError(`令牌 ${token} 的值长度非法`);
  }
  if (/[;{}<>\\]|\/\*|url\s*\(|expression\s*\(|@import/i.test(value)) {
    throw new ThemeLayerError(`令牌 ${token} 的值含有不安全的 CSS 片段：${value}`);
  }
}

/** 校验一层覆盖：令牌必须存在、属于本层命名空间（无障碍层除外），值必须安全 */
export function validateLayer(layer: ThemeLayer): void {
  for (const [name, value] of Object.entries(layer.tokens)) {
    if (!isRvToken(name)) throw new ThemeLayerError(`${layer.source}：未知令牌 ${name}`);
    if (value === undefined) continue;
    if (layer.kind !== "accessibility") {
      const owner: TokenLayer = tokenLayer(name);
      if (owner !== layer.kind) {
        throw new ThemeLayerError(`${layer.source}：${layer.kind} 层不能覆盖 ${owner} 层令牌 ${name}`);
      }
    }
    assertSafeCssValue(name, value);
  }
}

/** 主题解析输入 */
export interface ThemeStack {
  readonly base: ThemeName;
  readonly layers?: readonly ThemeLayer[];
}

/** 按五层优先级合并出完整令牌表；同一层内按数组顺序后者覆盖前者 */
export function resolveTheme(stack: ThemeStack): TokenTable {
  const out: Record<RvToken, string> = { ...THEMES[stack.base] };
  const layers = [...(stack.layers ?? [])];
  for (const l of layers) validateLayer(l);
  // 稳定排序：先按层级，再按原始顺序
  const ordered = layers
    .map((l, i) => ({ l, i }))
    .sort((a, b) => THEME_LAYER_ORDER.indexOf(a.l.kind) - THEME_LAYER_ORDER.indexOf(b.l.kind) || a.i - b.i);
  for (const { l } of ordered) {
    for (const [name, value] of Object.entries(l.tokens)) {
      if (value !== undefined) out[name as RvToken] = value;
    }
  }
  return out;
}

/** 无障碍预设层 */
export const ACCESSIBILITY_PRESETS = {
  /** 色盲安全：把语义色与可视化色换成 Okabe-Ito（与 `okabe-ito` 主题一致，可叠加在任意基础主题上） */
  "color-blind-safe": {
    kind: "accessibility",
    source: "a11y:color-blind-safe",
    tokens: {
      "--rv-color-success": OKABE_ITO[3],
      "--rv-color-warning": OKABE_ITO[4],
      "--rv-color-danger": OKABE_ITO[6],
      "--rv-color-info": OKABE_ITO[2],
      "--rv-heat-low": OKABE_ITO[5],
      "--rv-heat-mid": OKABE_ITO[2],
      "--rv-heat-high": OKABE_ITO[4],
      "--rv-neural-excitatory": OKABE_ITO[3],
      "--rv-neural-inhibitory": OKABE_ITO[6],
      "--rv-neural-sync": OKABE_ITO[2],
    },
  },
  /** 减弱动效：全部过渡时长归零 */
  "reduced-motion": {
    kind: "accessibility",
    source: "a11y:reduced-motion",
    tokens: { "--rv-duration-fast": "0ms", "--rv-duration-normal": "0ms" },
  },
} as const satisfies Record<string, ThemeLayer>;

export type AccessibilityPreset = keyof typeof ACCESSIBILITY_PRESETS;

/** 高对比度层：次要文本提升为正文色、边框换成强边框并加粗到 2px（依赖基础主题，故为函数） */
export function highContrastLayer(base: ThemeName): ThemeLayer {
  const t = THEMES[base];
  return {
    kind: "accessibility",
    source: "a11y:high-contrast",
    tokens: {
      "--rv-color-text-muted": t["--rv-color-text"],
      "--rv-color-border": t["--rv-color-border-strong"],
      "--rv-border-width": "2px",
    },
  };
}

/** 令牌表 → CSS 声明块正文（按令牌注册顺序输出，保证快照稳定） */
export function toCssDeclarations(table: TokenTable): string {
  return ALL_TOKENS.map((t) => `  ${t}: ${table[t]};`).join("\n");
}

/** 令牌表 → React `style` 对象 */
export function toStyleObject(table: TokenTable): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of ALL_TOKENS) out[t] = table[t];
  return out;
}

/** 生成三主题样式表：`:root` 取缺省主题，`[data-rv-theme="…"]` 切换主题，系统减弱动效时自动归零时长 */
export function themeStylesheet(defaultTheme: ThemeName = "neutral-dark"): string {
  const blocks = [`:root {\n${toCssDeclarations(THEMES[defaultTheme])}\n}`];
  for (const name of THEME_NAMES) {
    blocks.push(`[data-rv-theme="${name}"] {\n${toCssDeclarations(THEMES[name])}\n}`);
  }
  blocks.push(
    `@media (prefers-reduced-motion: reduce) {\n  :root, [data-rv-theme] {\n    --rv-duration-fast: 0ms;\n    --rv-duration-normal: 0ms;\n  }\n}`,
  );
  return `${blocks.join("\n\n")}\n`;
}

/** 主题对比度审计条目 */
export interface ContrastCheck {
  readonly foreground: RvToken;
  readonly background: RvToken;
  readonly minimum: number;
}

/** 每个主题必须满足的对比度下限（WCAG 2.2：AAA 正文 7:1、AA 次要文本 4.5:1、非文本 UI 3:1） */
export const CONTRAST_REQUIREMENTS: readonly ContrastCheck[] = [
  { foreground: "--rv-color-text", background: "--rv-color-bg", minimum: 7 },
  { foreground: "--rv-color-text", background: "--rv-color-surface", minimum: 7 },
  { foreground: "--rv-color-text", background: "--rv-color-overlay", minimum: 7 },
  { foreground: "--rv-color-text-muted", background: "--rv-color-bg", minimum: 4.5 },
  { foreground: "--rv-color-text-muted", background: "--rv-color-surface", minimum: 4.5 },
  { foreground: "--rv-color-accent-text", background: "--rv-color-accent", minimum: 4.5 },
  { foreground: "--rv-color-accent", background: "--rv-color-bg", minimum: 3 },
  { foreground: "--rv-color-focus-ring", background: "--rv-color-bg", minimum: 3 },
  { foreground: "--rv-color-danger", background: "--rv-color-surface", minimum: 3 },
  { foreground: "--rv-color-warning", background: "--rv-color-surface", minimum: 3 },
  { foreground: "--rv-color-success", background: "--rv-color-surface", minimum: 3 },
  { foreground: "--rv-color-info", background: "--rv-color-surface", minimum: 3 },
];

export interface ContrastFailure extends ContrastCheck {
  readonly ratio: number;
}

/** 审计一张令牌表的对比度；返回不达标项（颜色不是 `#RRGGBB` 的条目跳过） */
export function auditContrast(table: TokenTable): ContrastFailure[] {
  const failures: ContrastFailure[] = [];
  for (const c of CONTRAST_REQUIREMENTS) {
    const fg = table[c.foreground];
    const bg = table[c.background];
    if (!isHexColor(fg) || !isHexColor(bg)) continue;
    const ratio = contrastRatio(fg, bg);
    if (ratio < c.minimum) failures.push({ ...c, ratio });
  }
  return failures;
}
