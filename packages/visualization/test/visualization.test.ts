// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { TrustLevel, loadRview, writeRview, type RviewInputEntry } from "@rynvek/assets";
import { RenderLayer, layerMask } from "@rynvek/engine";
import { THEMES, resolveTheme } from "@rynvek/ui";
import { describe, expect, it } from "vitest";
import {
  NEURAL_LOD_THRESHOLDS,
  RingSeries,
  downsampleMinMax,
  factionColor,
  factionColorFromTheme,
  heatRampHex,
  heatRampRgb,
  kuramotoOrder,
  minMaxToSvgPath,
  neuralLodFor,
  packRgba8,
  packedWorldPalette,
  rviewThemeLayers,
  synapseStyle,
  syncRingColor,
  visibleLayersFor,
} from "../src/index.js";

const rust = (file: string) => readFileSync(new URL(`../../../crates/rynvek-render/src/${file}`, import.meta.url), "utf8");
const dark = THEMES["neutral-dark"];

describe("神经 LOD 与 Rust 渲染层一致", () => {
  it("阈值逐项等于 NeuralLod::THRESHOLDS", () => {
    const m = /THRESHOLDS: \[f32; 5\] = \[([^\]]+)\]/.exec(rust("lod.rs"));
    expect(m?.[1]?.split(",").map((s) => Number.parseFloat(s))).toEqual([...NEURAL_LOD_THRESHOLDS]);
  });

  it("复现 Rust 单测中的分级断言与边界", () => {
    const cases = [...rust("lod.rs").matchAll(/for_zoom\(([\d.]+)\), NeuralLod::L(\d)\)/g)];
    expect(cases.length).toBeGreaterThanOrEqual(5);
    for (const [, px, lod] of cases) {
      expect(neuralLodFor(Number(px))).toBe(Number(lod));
    }
    expect([0, 1.99, 2, 7.99, 8, 31.99, 32, 127.9, 128].map(neuralLodFor)).toEqual([0, 0, 1, 1, 2, 2, 3, 3, 4]);
  });

  it("各级可见图层", () => {
    const base = [RenderLayer.Terrain, RenderLayer.FieldHeatmap, RenderLayer.Structures, RenderLayer.FogOfWar, RenderLayer.Overlay];
    expect(visibleLayersFor(0)).toBe(layerMask(base));
    expect(visibleLayersFor(1) & (1 << RenderLayer.Organisms)).not.toBe(0);
    expect(visibleLayersFor(2) & (1 << RenderLayer.NeuralOverlay)).toBe(0);
    expect(visibleLayersFor(3)).toBe(visibleLayersFor(4));
    expect(visibleLayersFor(4)).toBe(0xff);
  });
});

describe("色标", () => {
  it("热力图端点与中点；中点等于主题 --rv-heat-mid", () => {
    expect(heatRampHex(0)).toBe("#0072B2");
    expect(heatRampHex(1)).toBe("#F0E442");
    expect(heatRampHex(-3)).toBe("#0072B2");
    expect(heatRampHex(Number.NaN)).toBe("#0072B2");
    for (const t of Object.values(THEMES)) expect(heatRampHex(0.5, t["--rv-heat-low"], t["--rv-heat-high"])).toBe(t["--rv-heat-mid"]);
    const [r, g, b] = heatRampRgb(0.25);
    expect(r).toBeCloseTo(0.75 * 0 + 0.25 * (240 / 255), 12);
    expect(g).toBeCloseTo(0.75 * (114 / 255) + 0.25 * (228 / 255), 12);
    expect(b).toBeCloseTo(0.75 * (178 / 255) + 0.25 * (66 / 255), 12);
  });

  it("阵营色与 WorldPalette::faction 一致，主题阵营令牌一致", () => {
    const cases = [...rust("palette.rs").matchAll(/faction\((\d+)\), Rgba8::hex\(0x([0-9A-F]{6})\)/g)];
    expect(cases.length).toBeGreaterThanOrEqual(2);
    for (const [, idx, hex] of cases) {
      expect(factionColor(Number(idx))).toBe(`#${hex}`);
    }
    for (let i = 0; i < 14; i++) expect(factionColorFromTheme(dark, i)).toBe(factionColor(i));
    expect(() => factionColor(-1)).toThrow(RangeError);
  });

  it("Rgba8 打包与 Rust 小端布局一致", () => {
    expect(packRgba8("#010203", 4)).toBe(0x04030201);
    expect(packRgba8("#A3D65C")).toBe(0xff5cd6a3);
    const p = packedWorldPalette(dark);
    expect(p.background).toBe(packRgba8("#090B0A"));
    expect(p.fog).toBe(0xc00a0b09);
    expect(p.factions).toHaveLength(7);
  });

  it("突触着色、Kuramoto 序参量与同步环", () => {
    expect(synapseStyle(0.5, 1, dark)).toEqual({ color: dark["--rv-neural-excitatory"], alpha: 0.5 });
    expect(synapseStyle(-2, 1, dark)).toEqual({ color: dark["--rv-neural-inhibitory"], alpha: 1 });
    expect(synapseStyle(1, 0, dark).alpha).toBe(0);
    expect(kuramotoOrder([])).toBe(0);
    expect(kuramotoOrder([1, 1, 1])).toBeCloseTo(1, 12);
    expect(kuramotoOrder([0, Math.PI])).toBeCloseTo(0, 12);
    expect(kuramotoOrder([0, Math.PI / 2])).toBeCloseTo(Math.SQRT1_2, 12);
    expect(syncRingColor(1, dark)).toBe(dark["--rv-neural-sync"]);
    expect(syncRingColor(0, dark)).toBe(dark["--rv-world-grid"]);
  });
});

describe("时间序列", () => {
  it("环形缓冲覆盖最旧样本，跳号补 NaN", () => {
    const s = new RingSeries(4);
    s.push(10, 1);
    s.push(11, 2);
    s.push(13, 4);
    expect(Array.from(s.toArray())).toEqual([1, 2, Number.NaN, 4]);
    s.push(14, 5);
    expect(Array.from(s.toArray())).toEqual([2, Number.NaN, 4, 5]);
    expect(s.firstTick).toBe(11);
    expect(() => s.push(14, 0)).toThrow(RangeError);
    s.push(100, 9);
    expect(Array.from(s.toArray())).toEqual([Number.NaN, Number.NaN, Number.NaN, 9]);
  });

  it("min/max 降采样保留单个尖峰，确定且与桶边界一致", () => {
    const v = new Float64Array(1000).fill(-65);
    v[503] = 30; // 单个动作电位
    const b = downsampleMinMax(v, 7);
    expect(b.max.length).toBe(7);
    expect(Array.from(b.max).filter((x) => x === 30)).toHaveLength(1);
    expect(b.max[Math.floor((503 * 7) / 1000)]).toBe(30);
    expect(downsampleMinMax([1, 2], 10).min.length).toBe(2);
    expect(Array.from(downsampleMinMax([Number.NaN, Number.NaN, 3, 1], 2).min)).toEqual([Number.NaN, 1]);
  });

  it("SVG 路径", () => {
    const b = downsampleMinMax([0, 1, Number.NaN, Number.NaN], 2);
    expect(minMaxToSvgPath(b, 100, 10, 0, 1)).toBe("M25.00 0.00V10.00");
    expect(minMaxToSvgPath(b, 100, 10, 1, 1)).toBe("");
  });
});

describe(".rview → 主题层", () => {
  const enc = new TextEncoder();
  const entry = (path: string, kind: RviewInputEntry["kind"], v: unknown): RviewInputEntry => ({
    path,
    kind,
    media_type: "application/json",
    data: enc.encode(JSON.stringify(v)),
  });
  const pack = (trustLevel: TrustLevel) =>
    loadRview(
      writeRview({
        id: "rynvek.visual.industrial-default",
        version: "1.0.0",
        name: "Industrial",
        license: "CC0-1.0",
        engine_compat: ">=0.1.0 <0.2.0",
        entries: [
          entry("palette/main.json", "palette", { colors: { "faction-1": "#112233", "heat-low": "#000080", "rock": "#555555" } }),
          entry("theme/override.json", "theme", {
            tokens: {
              "--rv-world-bg": "#0B0F0C",
              "--rv-faction-1": "#223344",
              "--rv-color-accent": "#FF00FF",
              "--rv-unknown": "#000000",
              "--rv-world-fog": "rgba(0,0,0,0.5)",
              "--rv-world-grid": "red; background: url(x)",
            },
          }),
        ],
      }),
      { trustLevel },
    );

  it("按命名空间分层，社区包不能改写应用层，非法值被剔除", () => {
    const { layers, rejected } = rviewThemeLayers(pack(TrustLevel.UntrustedEphemeral));
    expect(layers.map((l) => l.kind)).toEqual(["world", "faction", "visualization"]);
    expect(layers[0]?.tokens).toEqual({ "--rv-world-bg": "#0B0F0C", "--rv-world-fog": "rgba(0,0,0,0.5)" });
    expect(layers[1]?.tokens).toEqual({ "--rv-faction-1": "#223344" });
    expect(layers[2]?.tokens).toEqual({ "--rv-heat-low": "#000080" });
    expect(layers.every((l) => l.source === "rview:rynvek.visual.industrial-default@1.0.0")).toBe(true);
    expect(rejected.map((r) => [r.token, r.reason])).toEqual([
      ["--rv-color-accent", "application_layer_untrusted"],
      ["--rv-unknown", "unknown_token"],
      ["--rv-world-grid", "unsafe_value"],
    ]);
    const resolved = resolveTheme({ base: "neutral-dark", layers });
    expect(resolved["--rv-world-bg"]).toBe("#0B0F0C");
    expect(resolved["--rv-color-accent"]).toBe(dark["--rv-color-accent"]);
  });

  it("官方包可以改写应用层", () => {
    const { layers } = rviewThemeLayers(pack(TrustLevel.OfficialSigned));
    expect(layers[0]).toMatchObject({ kind: "application", tokens: { "--rv-color-accent": "#FF00FF" } });
  });
});
