// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// WGSL 审查跨语言向量：`tests/packages/wgsl_review_vectors.json` 由本测试生成并校验，
// Rust `rynvek-world::wgsl_review`（Hub 服务端）必须对每条向量给出相同结论与指令数。
// 更新向量：RYNVEK_UPDATE_FIXTURES=1 pnpm --filter @rynvek/assets test

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { TrustLevel, reviewRviewShader } from "../src/index.js";
import { SIMPLE_FRAGMENT } from "./fixtures.js";

const VECTORS = fileURLToPath(new URL("../../../tests/packages/wgsl_review_vectors.json", import.meta.url));

const frag = (body: string, top = ""): string =>
  `${top}\n@fragment fn m() -> @location(0) vec4<f32> { ${body} return vec4<f32>(1.0); }`;
const ops = (lines: number): string =>
  `@fragment\nfn fs_main() -> @location(0) vec4<f32> {\n  var x: f32 = 0.0;\n${Array.from({ length: lines }, () => "  x = x + 1.0;").join("\n")}\n  return vec4<f32>(x);\n}\n`;
const looped = `@fragment fn m() -> @location(0) vec4<f32> {\n  var s = 0.0;\n  for (var i = 0; i < 4; i++) { s += 1.0; }\n  return vec4<f32>(s);\n}`;

const SOURCES: Record<string, string> = {
  simple: SIMPLE_FRAGMENT,
  ops_10: ops(10),
  ops_255: ops(255),
  ops_1100: ops(1100),
  calls: `fn shade(v: f32) -> f32 { return v * 2.0 + 1.0; }\n@fragment fn main() -> @location(0) vec4<f32> {\n  let a = shade(1.0);\n  let b = shade(a);\n  return vec4<f32>(a, b, 0.0, 1.0);\n}`,
  recursion: `fn a() -> f32 { return b(); }\nfn b() -> f32 { return a(); }\n@fragment fn m() -> @location(0) vec4<f32> { return vec4<f32>(a()); }`,
  storage: frag("", "@group(0) @binding(0) var<storage, read_write> buf: array<u32>;"),
  atomic: frag("atomicAdd(&c, 1u);", "var<workgroup> c: atomic<u32>;"),
  texture_store: frag("textureStore(t, vec2<i32>(0), vec4<f32>(0.0));"),
  enable: "enable f16;\n" + frag(""),
  compute: "@compute @workgroup_size(64) fn c() {}\n" + frag(""),
  vertex: "@vertex fn v() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }\n" + frag(""),
  no_fragment: "fn only() {}",
  two_fragments: frag("") + "\n" + frag("").replace("fn m", "fn m2"),
  looped,
  loop_kw: looped.replace(/for \(.*\) /, "loop "),
  unclosed_comment: "/* storage " + SIMPLE_FRAGMENT,
  bad_char: SIMPLE_FRAGMENT + "\n$",
  numbers: frag("let a = 0x1Fu + 3u; let b = 1.5e-3f * .5 - 2.; let c = array<f32, 2>(1e4, 0.25h)[1] >> 1;"),
  long_ident: frag(`let ${"v".repeat(200)} = 1.0;`),
  nested_comment: "/* a /* b */ c */\n" + frag("let k = select(1.0, 2.0, true) /* x */;"),
};

type Vector = { name: string; trust_level: number; source: string; ok: boolean; instruction_count: number | null };

function compute(): Vector[] {
  const out: Vector[] = [];
  for (const [name, source] of Object.entries(SOURCES)) {
    for (const trust_level of [TrustLevel.OfficialSigned, TrustLevel.VerifiedCreator, TrustLevel.UntrustedEphemeral]) {
      try {
        const r = reviewRviewShader(source, trust_level);
        out.push({ name, trust_level, source, ok: true, instruction_count: r.instructionCount });
      } catch {
        out.push({ name, trust_level, source, ok: false, instruction_count: null });
      }
    }
  }
  return out;
}

describe("WGSL 审查跨语言向量", () => {
  it("向量文件与 TypeScript 审查器一致", () => {
    const vectors = compute();
    if (process.env["RYNVEK_UPDATE_FIXTURES"] === "1" || !existsSync(VECTORS)) {
      writeFileSync(VECTORS, JSON.stringify(vectors, null, 2) + "\n");
    }
    expect(JSON.parse(readFileSync(VECTORS, "utf8"))).toEqual(vectors);
    expect(vectors.some((v) => v.ok) && vectors.some((v) => !v.ok)).toBe(true);
  });
});
