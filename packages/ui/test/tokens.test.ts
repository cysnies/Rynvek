// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  ACCESSIBILITY_PRESETS,
  ALL_TOKENS,
  BRAND,
  FACTION_TOKENS,
  OKABE_ITO,
  THEMES,
  THEME_NAMES,
  auditContrast,
  contrastRatio,
  highContrastLayer,
  mixHex,
  resolveTheme,
  tokenLayer,
} from "../src/index.js";

describe("--rv-* 令牌表", () => {
  it("三主题名称与令牌前缀符合规范", () => {
    expect(THEME_NAMES).toEqual(["neutral-dark", "scientific-light", "okabe-ito"]);
    for (const t of ALL_TOKENS) expect(t).toMatch(/^--rv-[a-z0-9-]+$/);
    expect(new Set(ALL_TOKENS).size).toBe(ALL_TOKENS.length);
  });

  it("每个主题为全部令牌赋值，且没有多余键", () => {
    for (const name of THEME_NAMES) {
      expect(Object.keys(THEMES[name]).sort()).toEqual([...ALL_TOKENS].sort());
      for (const v of Object.values(THEMES[name])) expect(v.length).toBeGreaterThan(0);
    }
  });

  it("Neutral Dark 使用项目文档 6.1 冻结的品牌三色", () => {
    const t = THEMES["neutral-dark"];
    expect(t["--rv-color-bg"]).toBe(BRAND.background);
    expect(t["--rv-color-border"]).toBe(BRAND.border);
    expect(t["--rv-color-accent"]).toBe(BRAND.accent);
    expect(BRAND).toEqual({ background: "#090B0A", border: "#303A34", accent: "#A3D65C" });
    expect(t["--rv-font-sans"]).toMatch(/^"Geist Sans"/);
    expect(t["--rv-font-mono"]).toMatch(/^"Geist Mono"/);
    expect(t["--rv-border-width"]).toBe("1px");
  });

  it("阵营色与 rynvek-render 的 Okabe-Ito 表逐项一致且不随主题变化", () => {
    const rust = readFileSync(new URL("../../../crates/rynvek-render/src/palette.rs", import.meta.url), "utf8");
    const table = /pub const OKABE_ITO: \[Rgba8; 8\] = \[([\s\S]*?)\];/.exec(rust)![1]!;
    const hex = [...table.matchAll(/Rgba8::hex\(0x([0-9A-F]{6})\)/g)].map((m) => `#${m[1]}`);
    expect(hex).toEqual([...OKABE_ITO]);
    for (const name of THEME_NAMES) {
      FACTION_TOKENS.forEach((tok, i) => expect(THEMES[name][tok]).toBe(OKABE_ITO[i + 1]));
    }
  });

  it("热力色阶端点与渲染器一致（#0072B2 → #F0E442）", () => {
    expect(THEMES["neutral-dark"]["--rv-heat-low"]).toBe("#0072B2");
    expect(THEMES["neutral-dark"]["--rv-heat-high"]).toBe("#F0E442");
  });

  it("Okabe-Ito 主题的语义色全部取自 Okabe-Ito 八色表", () => {
    const t = THEMES["okabe-ito"];
    for (const tok of ["--rv-color-accent", "--rv-color-success", "--rv-color-warning", "--rv-color-danger", "--rv-color-info"] as const) {
      expect(OKABE_ITO).toContain(t[tok]);
    }
  });

  it("令牌按命名空间归属到正确的主题层", () => {
    expect(tokenLayer("--rv-color-bg")).toBe("application");
    expect(tokenLayer("--rv-world-grid")).toBe("world");
    expect(tokenLayer("--rv-faction-3")).toBe("faction");
    expect(tokenLayer("--rv-heat-low")).toBe("visualization");
  });
});

describe("WCAG 2.2 对比度", () => {
  it("对比度公式与已知值一致", () => {
    expect(contrastRatio("#000000", "#FFFFFF")).toBeCloseTo(21, 5);
    expect(contrastRatio("#FFFFFF", "#FFFFFF")).toBeCloseTo(1, 5);
    expect(mixHex("#000000", "#FFFFFF", 0.5)).toBe("#808080");
  });

  it.each(THEME_NAMES)("主题 %s 全部满足对比度下限", (name) => {
    expect(auditContrast(THEMES[name])).toEqual([]);
  });

  it("深色主题叠加色盲安全 / 高对比度层后仍满足下限", () => {
    const t = resolveTheme({ base: "neutral-dark", layers: [ACCESSIBILITY_PRESETS["color-blind-safe"], highContrastLayer("neutral-dark")] });
    expect(auditContrast(t)).toEqual([]);
    expect(t["--rv-color-text-muted"]).toBe(t["--rv-color-text"]);
  });

  it("审计能发现不达标的配色", () => {
    const bad = { ...THEMES["neutral-dark"], "--rv-color-text-muted": "#202421" };
    expect(auditContrast(bad).map((f) => f.foreground)).toContain("--rv-color-text-muted");
  });
});
