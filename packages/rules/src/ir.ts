// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * `Typed SSA Rule IR` 的公开常量与 `rynvek.rule-graph.v1` JSON 结构（与 Rust `rynvek-rules` 的 `ir` / `graph` 模块逐项一致）。
 *
 * 规则图是 `Kernel.define` 的输出、Rust 编译器管线的输入：节点下标即 SSA 值编号，
 * `imm` 一律以十进制字符串承载，避免 JS `number` 丢失 64 位精度。
 */

/** 规则图格式标识 */
export const RULE_GRAPH_FORMAT = "rynvek.rule-graph.v1";

/** IR 值类型（定点提升次序 `I32 < Q8_8 < Q32_32`） */
export type ValueType = "Void" | "Bool" | "I32" | "Q8_8" | "Q32_32";

/** 36 条 SSA 指令，数组下标即操作码字节值 */
export const OPCODES = [
  "ConstBool",
  "ConstI32",
  "ConstQ8_8",
  "ConstQ32_32",
  "Add",
  "Sub",
  "Mul",
  "Div",
  "Neg",
  "Abs",
  "Min",
  "Max",
  "Clamp",
  "CmpEq",
  "CmpNe",
  "CmpLt",
  "CmpLe",
  "CmpGt",
  "CmpGe",
  "And",
  "Or",
  "Not",
  "Select",
  "CastI32",
  "CastQ8_8",
  "CastQ32_32",
  "LoadComponent",
  "EmitComponentWrite",
  "SampleField",
  "EmitFieldDeposit",
  "NeighborCount",
  "LoadNeighborComponent",
  "RandomQ8_8",
  "BoundedForBegin",
  "BoundedForEnd",
  "Phi",
] as const;

export type Opcode = (typeof OPCODES)[number];

/** 场通道名称（下标即通道号，与 `rynvek-field` 的 5 通道一致） */
export const FIELD_CHANNEL_NAMES = ["nutrient", "energy", "signal_a", "signal_b", "hazard"] as const;

export type FieldChannel = (typeof FIELD_CHANNEL_NAMES)[number];

/** 场写入在写集中的声明前缀：`field.<channel>` */
export const FIELD_WRITE_PREFIX = "field.";

/** 单个循环的迭代上界（规范 08：`max_iters ≤ 64`） */
export const MAX_LOOP_ITERS = 64;

/** 嵌套循环总展开次数上界（规范 08：`≤ 256`） */
export const MAX_NESTED_UNROLL = 256;

export interface ComponentDecl {
  readonly component: string;
  readonly ty: ValueType;
}

/** 规则图节点；字段顺序即 JSON 规范序列化顺序 */
export interface GraphNode {
  readonly op: Opcode;
  readonly args?: readonly number[];
  readonly imm?: string;
  readonly component?: string;
  readonly channel?: FieldChannel;
  readonly maxIters?: number;
  readonly stream?: number;
}

export interface UntypedRuleGraph {
  readonly format: typeof RULE_GRAPH_FORMAT;
  readonly name: string;
  readonly reads: readonly ComponentDecl[];
  readonly writes: readonly ComponentDecl[];
  readonly nodes: readonly GraphNode[];
}

/** 规范 JSON 文本：2 空格缩进 + 末尾换行（与 Rust `serde_json::to_string_pretty` 输出逐字节一致） */
export function canonicalGraphJson(graph: UntypedRuleGraph): string {
  return `${JSON.stringify(graph, null, 2)}\n`;
}
