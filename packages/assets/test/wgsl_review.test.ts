// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { describe, expect, it } from "vitest";
import { TrustLevel, WGSL_FRAGMENT_INSTRUCTION_BUDGET, reviewRviewShader, stripWgslComments } from "../src/index.js";
import { SIMPLE_FRAGMENT } from "./fixtures.js";

function rejected(src: string, level: TrustLevel = TrustLevel.UntrustedEphemeral): string | null {
  try {
    reviewRviewShader(src, level);
    return null;
  } catch (e) {
    expect(e).toBeInstanceOf(RynvekError);
    expect((e as RynvekError).code).toBe(RynvekErrorCode.UnsafeRviewShaderRejected);
    return (e as Error).message;
  }
}

/** 生成恰好 n 条估算指令的片段着色器：每行 `x = x + 1.0;` 计 2 条，`return x4;` 中 return 与构造器各 1 条 */
function shaderWithOps(lines: number): string {
  const body = Array.from({ length: lines }, () => "  x = x + 1.0;").join("\n");
  return `@fragment\nfn fs_main() -> @location(0) vec4<f32> {\n  var x: f32 = 0.0;\n${body}\n  return vec4<f32>(x);\n}\n`;
}

describe("WGSL 片段着色器安全审查", () => {
  it("TrustLevel 3 预算为 512，TrustLevel 0/1 不限", () => {
    expect(WGSL_FRAGMENT_INSTRUCTION_BUDGET).toEqual({ 0: null, 1: null, 2: 2048, 3: 512 });
  });

  it("估算指令数可复现：赋值、运算、调用、构造器、控制流", () => {
    expect(reviewRviewShader(SIMPLE_FRAGMENT, TrustLevel.UntrustedEphemeral).instructionCount).toBe(6);
    // var 初始化 1 + 每行 2 + return 1 + 构造器 1
    expect(reviewRviewShader(shaderWithOps(10), TrustLevel.UntrustedEphemeral).instructionCount).toBe(1 + 20 + 2);
  });

  it("恰好 512 通过，513 拒绝（0x9004）；TrustLevel 2 与 1 放宽", () => {
    // 1 + 2n + 2 = 511 → n = 254；补一个一元取反凑到 512
    const at512 = shaderWithOps(254).replace("var x: f32 = 0.0;", "var x: f32 = -0.0;");
    expect(reviewRviewShader(at512, TrustLevel.UntrustedEphemeral).instructionCount).toBe(512);
    const over = shaderWithOps(255);
    expect(reviewRviewShader(over, TrustLevel.VerifiedCreator).instructionCount).toBe(513);
    expect(rejected(over)).toMatch(/513.*512/);
    expect(reviewRviewShader(shaderWithOps(3000), TrustLevel.OfficialSigned).budget).toBeNull();
  });

  it("自定义函数按调用点展开计入入口，递归被拒绝", () => {
    const src = `
fn shade(v: f32) -> f32 { return v * 2.0 + 1.0; }
@fragment fn main() -> @location(0) vec4<f32> {
  let a = shade(1.0);
  let b = shade(a);
  return vec4<f32>(a, b, 0.0, 1.0);
}`;
    const r = reviewRviewShader(src, TrustLevel.UntrustedEphemeral);
    expect(r.perFunction).toEqual({ shade: 3, main: 6 });
    expect(r.instructionCount).toBe(6 + 3 * 2);
    expect(rejected(`fn a() -> f32 { return b(); }\nfn b() -> f32 { return a(); }\n@fragment fn m() -> @location(0) vec4<f32> { return vec4<f32>(a()); }`)).toMatch(/递归/);
  });

  it("越过只读表现层的能力一律拒绝", () => {
    const frag = (body: string, top = "") => `${top}\n@fragment fn m() -> @location(0) vec4<f32> { ${body} return vec4<f32>(1.0); }`;
    expect(rejected(frag("", "@group(0) @binding(0) var<storage, read_write> buf: array<u32>;"))).toMatch(/storage/);
    expect(rejected(frag("atomicAdd(&c, 1u);", "var<workgroup> c: atomic<u32>;"))).not.toBeNull();
    expect(rejected(frag("textureStore(t, vec2<i32>(0), vec4<f32>(0.0));"))).toMatch(/textureStore/);
    expect(rejected("enable f16;\n" + frag(""))).toMatch(/enable/);
    expect(rejected("@compute @workgroup_size(64) fn c() {}\n" + frag(""))).toMatch(/@compute/);
    expect(rejected("@vertex fn v() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }\n" + frag(""))).toMatch(/@vertex/);
    expect(rejected("fn only() {}")).toMatch(/@fragment/);
    expect(rejected(frag("") + "\n" + frag("").replace("fn m", "fn m2"))).toMatch(/恰好包含一个/);
  });

  it("TrustLevel 3 禁止循环，TrustLevel 2 允许", () => {
    const looped = `@fragment fn m() -> @location(0) vec4<f32> {
  var s = 0.0;
  for (var i = 0; i < 4; i++) { s += 1.0; }
  return vec4<f32>(s);
}`;
    expect(rejected(looped)).toMatch(/循环/);
    expect(reviewRviewShader(looped, TrustLevel.VerifiedCreator).instructionCount).toBeGreaterThan(0);
    expect(rejected(looped.replace(/for \(.*\) /, "loop "))).toMatch(/循环/);
  });

  it("注释（含嵌套块注释）不参与审查；未闭合注释与非法字符被拒绝", () => {
    expect(stripWgslComments("a /* x /* y */ z */ b // c\nd")).toBe("a   b \nd");
    expect(rejected("/* storage " + SIMPLE_FRAGMENT)).toMatch(/未闭合/);
    expect(rejected(SIMPLE_FRAGMENT + "\n$")).toMatch(/无法识别/);
    expect(rejected("x".repeat(64 * 1024 + 1))).toMatch(/超过/);
  });
});
