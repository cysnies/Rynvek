// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 测试用视觉包构造器。

import { writeRview, type RviewInput, type RviewInputEntry } from "../src/index.js";

const enc = new TextEncoder();

export const SIMPLE_FRAGMENT = `
// 注释里的 storage / loop / @compute 不应触发审查
struct Params { tint: vec4<f32>, }
@group(0) @binding(0) var<uniform> params: Params;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
  let c = mix(vec3<f32>(0.0), vec3<f32>(1.0), uv.x);
  return vec4<f32>(c, 1.0);
}
`;

export const PNG_1x1 = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0,
]);

export function jsonEntry(path: string, kind: RviewInputEntry["kind"], value: unknown): RviewInputEntry {
  return { path, kind, media_type: "application/json", data: enc.encode(JSON.stringify(value)) };
}

export function sampleInput(overrides: Partial<RviewInput> = {}): RviewInput {
  return {
    id: "rynvek.visual.industrial-default",
    version: "1.0.0",
    name: "Industrial Default",
    license: "CC0-1.0",
    engine_compat: ">=0.1.0 <0.2.0",
    entries: [
      jsonEntry("palette/factions.json", "palette", {
        colors: { "faction-1": "#e69f00", "faction-2": "#56B4E9", "heat-low": "#0072B2" },
      }),
      jsonEntry("theme/biome.json", "theme", { tokens: { "--rv-world-bg": "#101412", "--rv-world-grid": "#2a332e" } }),
      { path: "shaders/glow.wgsl", kind: "shader", media_type: "text/wgsl", data: enc.encode(SIMPLE_FRAGMENT) },
      { path: "sprites/atlas.png", kind: "sprite_atlas", media_type: "image/png", data: PNG_1x1 },
      jsonEntry("sprites/atlas.json", "sprite_atlas", {
        image: "sprites/atlas.png",
        frames: { extractor: { x: 0, y: 0, w: 32, h: 32 } },
      }),
    ],
    ...overrides,
  };
}

export function samplePack(overrides: Partial<RviewInput> = {}): Uint8Array {
  return writeRview(sampleInput(overrides));
}
