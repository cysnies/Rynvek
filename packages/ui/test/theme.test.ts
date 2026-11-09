// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import {
  ACCESSIBILITY_PRESETS,
  THEMES,
  type ThemeLayer,
  ThemeLayerError,
  assertSafeCssValue,
  resolveTheme,
  themeStylesheet,
  toCssDeclarations,
} from "../src/index.js";

const faction: ThemeLayer = { kind: "faction", source: "world:test", tokens: { "--rv-faction-1": "#111111" } };

describe("五层主题隔离", () => {
  it("低四层只能覆盖本层命名空间", () => {
    expect(() =>
      resolveTheme({ base: "neutral-dark", layers: [{ kind: "faction", source: "rview:evil", tokens: { "--rv-color-bg": "#FF0000" } }] }),
    ).toThrowError(ThemeLayerError);
    expect(() =>
      resolveTheme({ base: "neutral-dark", layers: [{ kind: "world", source: "rworld:x", tokens: { "--rv-heat-low": "#000000" } }] }),
    ).toThrow(/world 层不能覆盖 visualization 层令牌/);
    expect(() => resolveTheme({ base: "neutral-dark", layers: [{ kind: "application", source: "x", tokens: { ["--rv-nope" as "--rv-color-bg"]: "#000000" } }] })).toThrow(/未知令牌/);
  });

  it("优先级 Accessibility > Visualization > Faction > World > Application，与数组顺序无关", () => {
    const vis: ThemeLayer = { kind: "visualization", source: "rview:pack", tokens: { "--rv-heat-low": "#123456" } };
    const a11y = ACCESSIBILITY_PRESETS["color-blind-safe"];
    const t1 = resolveTheme({ base: "scientific-light", layers: [a11y, vis, faction] });
    const t2 = resolveTheme({ base: "scientific-light", layers: [faction, vis, a11y] });
    expect(t1).toEqual(t2);
    expect(t1["--rv-heat-low"]).toBe("#0072B2");
    expect(t1["--rv-faction-1"]).toBe("#111111");
    expect(resolveTheme({ base: "neutral-dark", layers: [vis] })["--rv-heat-low"]).toBe("#123456");
  });

  it("同一层内后者覆盖前者", () => {
    const later: ThemeLayer = { kind: "faction", source: "b", tokens: { "--rv-faction-1": "#222222" } };
    expect(resolveTheme({ base: "neutral-dark", layers: [faction, later] })["--rv-faction-1"]).toBe("#222222");
  });

  it("不改动基础主题对象", () => {
    const before = { ...THEMES["neutral-dark"] };
    resolveTheme({ base: "neutral-dark", layers: [faction, ACCESSIBILITY_PRESETS["reduced-motion"]] });
    expect(THEMES["neutral-dark"]).toEqual(before);
  });

  it("拒绝可逃逸声明块的不安全值", () => {
    for (const v of ["red; } body { display:none", "url(https://x)", "/* x */", "", "a".repeat(300), "expression(alert(1))"]) {
      expect(() => assertSafeCssValue("--rv-faction-1", v)).toThrowError(ThemeLayerError);
    }
    expect(() => assertSafeCssValue("--rv-faction-1", "rgba(1, 2, 3, 0.5)")).not.toThrow();
  });
});

describe("样式表生成", () => {
  it("包含 :root 缺省主题、三主题选择器与减弱动效媒体查询", () => {
    const css = themeStylesheet();
    expect(css).toContain(":root {\n  --rv-color-bg: #090B0A;");
    for (const n of ["neutral-dark", "scientific-light", "okabe-ito"]) expect(css).toContain(`[data-rv-theme="${n}"] {`);
    expect(css).toContain("@media (prefers-reduced-motion: reduce)");
    expect(css).not.toMatch(/--nv-|neuroverse/i);
  });

  it("声明顺序稳定", () => {
    expect(toCssDeclarations(THEMES["okabe-ito"])).toBe(toCssDeclarations({ ...THEMES["okabe-ito"] }));
    expect(toCssDeclarations(THEMES["neutral-dark"]).split("\n")[0]).toBe("  --rv-color-bg: #090B0A;");
  });
});
