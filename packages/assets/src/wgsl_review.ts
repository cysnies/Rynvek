// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// `.rview` 视觉包 WGSL 片段着色器的静态安全审查（拒绝时抛 0x9004 UnsafeRviewShaderRejected）。
//
// 项目文档只冻结了一条硬约束：TrustLevel 3 的 WGSL 片段着色器指令数 ≤ 512。
// 浏览器端无法调用驱动编译器，这里按最佳实践给出可复现的保守估算与禁用清单（ERRATA E-019）：
//   - 指令数 = 运算符 + 函数 / 构造器调用 + 赋值 + 下标访问 + 控制流语句；
//     自定义函数按调用点展开计入入口函数；递归（WGSL 本身禁止）视为违规。
//   - 视觉包只允许单个 `@fragment` 入口；禁止计算 / 顶点阶段、存储缓冲、原子操作、
//     工作组同步、存储纹理写入与 `enable` 扩展——这些能力会越过“只读表现层”的边界。
//   - TrustLevel 3 额外禁止一切循环（静态无法给出上界），TrustLevel 2 允许循环但有 2048 的预算。

import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { TrustLevel } from "./trust.js";

/** 各信任等级的片段着色器指令预算；`null` 表示不限（官方与内置包由发布流程把关） */
export const WGSL_FRAGMENT_INSTRUCTION_BUDGET: Readonly<Record<TrustLevel, number | null>> = {
  [TrustLevel.CoreBuiltin]: null,
  [TrustLevel.OfficialSigned]: null,
  [TrustLevel.VerifiedCreator]: 2048,
  [TrustLevel.UntrustedEphemeral]: 512,
};

/** 着色器源码长度上限（字节） */
export const WGSL_MAX_SOURCE_BYTES = 64 * 1024;

/** 对 TrustLevel ≥ 2 一律禁用的能力（关键字 / 内建函数） */
export const WGSL_FORBIDDEN_IDENTIFIERS: readonly string[] = [
  "enable",
  "storage",
  "atomic",
  "atomicAdd",
  "atomicSub",
  "atomicMax",
  "atomicMin",
  "atomicAnd",
  "atomicOr",
  "atomicXor",
  "atomicExchange",
  "atomicCompareExchangeWeak",
  "atomicLoad",
  "atomicStore",
  "workgroup",
  "workgroupBarrier",
  "storageBarrier",
  "textureBarrier",
  "workgroupUniformLoad",
  "textureStore",
  "texture_storage_1d",
  "texture_storage_2d",
  "texture_storage_2d_array",
  "texture_storage_3d",
];

/** 禁止的入口属性 */
export const WGSL_FORBIDDEN_ATTRIBUTES: readonly string[] = ["compute", "vertex", "workgroup_size"];

/** TrustLevel 3 额外禁止的循环关键字 */
export const WGSL_LOOP_KEYWORDS: readonly string[] = ["loop", "for", "while"];

const CONTROL_KEYWORDS = new Set(["if", "switch", "return", "discard", "break", "continue", "loop", "for", "while", "continuing"]);

/** 可接模板参数的类型生成器，`<` 紧随其后时视为模板而非比较 */
const TEMPLATE_TYPES = new Set([
  "vec2", "vec3", "vec4", "mat2x2", "mat2x3", "mat2x4", "mat3x2", "mat3x3", "mat3x4", "mat4x2", "mat4x3", "mat4x4",
  "array", "ptr", "atomic", "var", "bitcast",
  "texture_1d", "texture_2d", "texture_2d_array", "texture_3d", "texture_cube", "texture_cube_array",
  "texture_multisampled_2d", "texture_storage_1d", "texture_storage_2d", "texture_storage_2d_array", "texture_storage_3d",
]);

const OPERATORS = [
  "<<=", ">>=", "&&", "||", "==", "!=", "<=", ">=", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "++", "--", "->",
  "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "<", ">", "=",
  "(", ")", "{", "}", "[", "]", ",", ";", ":", ".", "@",
];

type Token = { readonly kind: "ident" | "number" | "punct"; readonly text: string };

/** 审查结论 */
export interface WgslReviewReport {
  /** 入口函数名 */
  readonly entryPoint: string;
  /** 入口函数展开后的估算指令数 */
  readonly instructionCount: number;
  /** 适用预算（null 为不限） */
  readonly budget: number | null;
  /** 各函数自身（不含被调函数）的指令数 */
  readonly perFunction: Readonly<Record<string, number>>;
}

function reject(detail: string): never {
  throw new RynvekError(RynvekErrorCode.UnsafeRviewShaderRejected, detail);
}

/** 去掉行注释与可嵌套块注释 */
export function stripWgslComments(src: string): string {
  let out = "";
  let i = 0;
  let depth = 0;
  while (i < src.length) {
    if (depth === 0 && src.startsWith("//", i)) {
      while (i < src.length && src[i] !== "\n") i++;
    } else if (src.startsWith("/*", i)) {
      depth++;
      i += 2;
    } else if (depth > 0 && src.startsWith("*/", i)) {
      depth--;
      i += 2;
      if (depth === 0) out += " ";
    } else {
      if (depth === 0) out += src[i];
      i++;
    }
  }
  if (depth !== 0) reject("块注释未闭合");
  return out;
}

/** 词法切分（只区分标识符、数字与标点，足以完成预算估算） */
export function tokenizeWgsl(src: string): Token[] {
  const tokens: Token[] = [];
  const text = stripWgslComments(src);
  let i = 0;
  while (i < text.length) {
    const c = text[i] ?? "";
    if (/\s/.test(c)) {
      i++;
      continue;
    }
    const ident = /^[A-Za-z_][A-Za-z0-9_]*/.exec(text.slice(i, i + 128));
    if (ident) {
      tokens.push({ kind: "ident", text: ident[0] });
      i += ident[0].length;
      continue;
    }
    const num = /^(?:0[xX][0-9a-fA-F]+|[0-9]*\.?[0-9]+(?:[eE][+-]?[0-9]+)?)[iuhf]?/.exec(text.slice(i, i + 64));
    if (num && num[0] !== "" && /[0-9]/.test(num[0])) {
      tokens.push({ kind: "number", text: num[0] });
      i += num[0].length;
      continue;
    }
    const op = OPERATORS.find((o) => text.startsWith(o, i));
    if (op === undefined) reject(`无法识别的字符 ${JSON.stringify(c)}`);
    tokens.push({ kind: "punct", text: op });
    i += op.length;
  }
  return tokens;
}

interface FnInfo {
  readonly name: string;
  readonly attributes: readonly string[];
  readonly ownCost: number;
  readonly calls: readonly string[];
}

/** 跳过以 `<` 开头的模板参数，返回 `>` 之后的下标 */
function skipTemplate(tokens: readonly Token[], start: number): number {
  let depth = 0;
  for (let i = start; i < tokens.length; i++) {
    const t = tokens[i]?.text;
    if (t === "<") depth++;
    else if (t === ">") depth--;
    else if (t === ">>") depth -= 2;
    if (depth <= 0) return i + 1;
  }
  reject("模板参数未闭合");
}

/** 跳过成对括号，`start` 指向开括号，返回闭括号之后的下标 */
function skipBalanced(tokens: readonly Token[], start: number, open: string, close: string): number {
  let depth = 0;
  for (let i = start; i < tokens.length; i++) {
    const t = tokens[i]?.text;
    if (t === open) depth++;
    else if (t === close && --depth === 0) return i + 1;
  }
  reject(`括号 ${open}${close} 不配对`);
}

/** 统计一段函数体的指令数并收集调用名 */
function costOf(tokens: readonly Token[], fnNames: ReadonlySet<string>): { cost: number; calls: string[] } {
  let cost = 0;
  const calls: string[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    if (t === undefined) continue;
    const next = tokens[i + 1]?.text;
    if (t.kind === "ident") {
      if (CONTROL_KEYWORDS.has(t.text)) {
        cost++;
        continue;
      }
      if (next === "<" && TEMPLATE_TYPES.has(t.text)) {
        const after = skipTemplate(tokens, i + 1);
        if (tokens[after]?.text === "(") cost++; // 带模板参数的构造器调用
        i = after - 1;
        continue;
      }
      if (next === "(") {
        cost++;
        if (fnNames.has(t.text)) calls.push(t.text);
      }
      continue;
    }
    if (t.kind === "punct") {
      if (t.text === "@") {
        // 函数体内的属性（极少见）不计入
        i++;
        if (tokens[i + 1]?.text === "(") i = skipBalanced(tokens, i + 1, "(", ")") - 1;
        continue;
      }
      if (t.text === "[") cost++;
      else if (!["(", ")", "{", "}", "]", ",", ";", ":", ".", "->"].includes(t.text)) cost++;
    }
  }
  return { cost, calls };
}

/** 把源码切分为顶层函数 */
function parseFunctions(tokens: readonly Token[]): FnInfo[] {
  const names = new Set<string>();
  for (let i = 0; i + 1 < tokens.length; i++) {
    if (tokens[i]?.text === "fn" && tokens[i + 1]?.kind === "ident") names.add(tokens[i + 1]?.text ?? "");
  }
  const fns: FnInfo[] = [];
  let pendingAttrs: string[] = [];
  let i = 0;
  while (i < tokens.length) {
    const t = tokens[i];
    if (t?.text === "@") {
      const attr = tokens[i + 1]?.text ?? "";
      pendingAttrs.push(attr);
      i += 2;
      if (tokens[i]?.text === "(") i = skipBalanced(tokens, i, "(", ")");
      continue;
    }
    if (t?.text === "fn") {
      const name = tokens[i + 1]?.text ?? "";
      let j = i + 2;
      if (tokens[j]?.text !== "(") reject(`函数 ${name} 缺少参数列表`);
      j = skipBalanced(tokens, j, "(", ")");
      while (j < tokens.length && tokens[j]?.text !== "{") j++;
      const end = skipBalanced(tokens, j, "{", "}");
      const { cost, calls } = costOf(tokens.slice(j + 1, end - 1), names);
      fns.push({ name, attributes: pendingAttrs, ownCost: cost, calls });
      pendingAttrs = [];
      i = end;
      continue;
    }
    if (t?.text === ";" || t?.text === "}") pendingAttrs = [];
    i++;
  }
  return fns;
}

/**
 * 审查视觉包着色器；通过时返回报告，违规时抛 `0x9004`。
 * TrustLevel 0/1 只做结构检查与计数，不施加预算与禁用清单。
 */
export function reviewRviewShader(source: string, trustLevel: TrustLevel): WgslReviewReport {
  if (new TextEncoder().encode(source).length > WGSL_MAX_SOURCE_BYTES) {
    reject(`着色器源码超过 ${WGSL_MAX_SOURCE_BYTES} 字节`);
  }
  const tokens = tokenizeWgsl(source);
  const restricted = trustLevel >= TrustLevel.VerifiedCreator;

  if (restricted) {
    for (let i = 0; i < tokens.length; i++) {
      const t = tokens[i];
      if (t?.kind !== "ident") continue;
      const isAttr = tokens[i - 1]?.text === "@";
      if (isAttr && WGSL_FORBIDDEN_ATTRIBUTES.includes(t.text)) reject(`禁止的入口属性 @${t.text}`);
      if (!isAttr && WGSL_FORBIDDEN_IDENTIFIERS.includes(t.text)) reject(`禁止使用 ${t.text}`);
      if (trustLevel === TrustLevel.UntrustedEphemeral && WGSL_LOOP_KEYWORDS.includes(t.text)) {
        reject(`TrustLevel 3 禁止循环（${t.text}）`);
      }
    }
  }

  const fns = parseFunctions(tokens);
  const entries = fns.filter((f) => f.attributes.includes("fragment"));
  if (entries.length !== 1) reject(`视觉包着色器必须恰好包含一个 @fragment 入口（实际 ${entries.length} 个）`);
  const entry = entries[0] as FnInfo;
  const byName = new Map(fns.map((f) => [f.name, f]));
  if (byName.size !== fns.length) reject("函数重名");

  // 展开调用图：记忆化 + 栈检测递归
  const memo = new Map<string, number>();
  const visiting = new Set<string>();
  const total = (name: string): number => {
    const cached = memo.get(name);
    if (cached !== undefined) return cached;
    if (visiting.has(name)) reject(`检测到递归调用 ${name}`);
    visiting.add(name);
    const f = byName.get(name);
    let sum = f?.ownCost ?? 0;
    for (const callee of f?.calls ?? []) sum += total(callee);
    visiting.delete(name);
    memo.set(name, sum);
    return sum;
  };
  const instructionCount = total(entry.name);
  const budget = WGSL_FRAGMENT_INSTRUCTION_BUDGET[trustLevel];
  if (budget !== null && instructionCount > budget) {
    reject(`片段着色器估算指令数 ${instructionCount} 超过 TrustLevel ${trustLevel} 上限 ${budget}`);
  }
  return {
    entryPoint: entry.name,
    instructionCount,
    budget,
    perFunction: Object.fromEntries(fns.map((f) => [f.name, f.ownCost])),
  };
}
