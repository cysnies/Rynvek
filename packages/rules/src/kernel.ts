// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * Tier-0 规则创作入口 `Kernel.define`：声明式 AST Builder（规范 08 §8.1 第 1 步）。
 *
 * 规则体是普通 TS 回调，但它**不在运行时执行业务逻辑**，而是记录一张无类型 SSA 规则图，
 * 交给 Rust `rynvek-rules` 做类型推断、读写集与循环上界验证并转译为 WGSL / CPU 字节码。
 *
 * 本端提前做与 Rust 一致的廉价检查，尽早给出统一错误码：
 * - 写入未声明的组件或场通道 → `RuleUndeclaredWriteAccess (0x5002)`；
 * - 循环上界 `∉ [1, 64]` 或嵌套总展开 `> 256` → `RuleLoopBoundExceeded (0x5003)`；
 * - 读取未声明组件、跨 Kernel / 越出循环作用域使用值、未知场通道、非法常量 → `RuleStaticTypeCheckFailed (0x5001)`。
 *
 * 完整类型检查（定点提升、窄化禁止等）以 Rust 编译器为准。
 */

import { Q32_32, Q8_8, RynvekError, RynvekErrorCode } from "@rynvek/core";

import {
  FIELD_CHANNEL_NAMES,
  FIELD_WRITE_PREFIX,
  MAX_LOOP_ITERS,
  MAX_NESTED_UNROLL,
  RULE_GRAPH_FORMAT,
  canonicalGraphJson,
  type ComponentDecl,
  type FieldChannel,
  type GraphNode,
  type Opcode,
  type UntypedRuleGraph,
  type ValueType,
} from "./ir.js";

/** 读写集声明：组件名 → 值类型；场写入以 `field.<channel>: "Q8_8"` 声明 */
export interface KernelDeclaration {
  readonly reads?: Readonly<Record<string, ValueType>>;
  readonly writes?: Readonly<Record<string, ValueType>>;
}

function typeError(detail: string): RynvekError {
  return new RynvekError(RynvekErrorCode.RuleStaticTypeCheckFailed, detail);
}

/** 循环作用域：根作用域 depth = 0 */
interface Scope {
  readonly parent: Scope | undefined;
  readonly unroll: number;
}

/** 规则图中的一个 SSA 值句柄 */
export class RuleValue {
  /** @internal */
  public constructor(
    /** @internal */ public readonly builder: KernelBuilder,
    /** 节点下标（SSA 值编号） */ public readonly id: number,
    /** @internal */ public readonly scope: Scope,
  ) {}

  private bin(op: Opcode, rhs: RuleValue): RuleValue {
    return this.builder.op(op, [this, rhs]);
  }

  public add(rhs: RuleValue): RuleValue {
    return this.bin("Add", rhs);
  }
  public sub(rhs: RuleValue): RuleValue {
    return this.bin("Sub", rhs);
  }
  public mul(rhs: RuleValue): RuleValue {
    return this.bin("Mul", rhs);
  }
  public div(rhs: RuleValue): RuleValue {
    return this.bin("Div", rhs);
  }
  public min(rhs: RuleValue): RuleValue {
    return this.bin("Min", rhs);
  }
  public max(rhs: RuleValue): RuleValue {
    return this.bin("Max", rhs);
  }
  public neg(): RuleValue {
    return this.builder.op("Neg", [this]);
  }
  public abs(): RuleValue {
    return this.builder.op("Abs", [this]);
  }
  public clamp(lo: RuleValue, hi: RuleValue): RuleValue {
    return this.builder.op("Clamp", [this, lo, hi]);
  }
  public eq(rhs: RuleValue): RuleValue {
    return this.bin("CmpEq", rhs);
  }
  public ne(rhs: RuleValue): RuleValue {
    return this.bin("CmpNe", rhs);
  }
  public lt(rhs: RuleValue): RuleValue {
    return this.bin("CmpLt", rhs);
  }
  public le(rhs: RuleValue): RuleValue {
    return this.bin("CmpLe", rhs);
  }
  public gt(rhs: RuleValue): RuleValue {
    return this.bin("CmpGt", rhs);
  }
  public ge(rhs: RuleValue): RuleValue {
    return this.bin("CmpGe", rhs);
  }
  public and(rhs: RuleValue): RuleValue {
    return this.bin("And", rhs);
  }
  public or(rhs: RuleValue): RuleValue {
    return this.bin("Or", rhs);
  }
  public not(): RuleValue {
    return this.builder.op("Not", [this]);
  }
  /** `this ? ifTrue : ifFalse`（`this` 必须为 Bool） */
  public select(ifTrue: RuleValue, ifFalse: RuleValue): RuleValue {
    return this.builder.op("Select", [this, ifTrue, ifFalse]);
  }
  public toI32(): RuleValue {
    return this.builder.op("CastI32", [this]);
  }
  public toQ8_8(): RuleValue {
    return this.builder.op("CastQ8_8", [this]);
  }
  public toQ32_32(): RuleValue {
    return this.builder.op("CastQ32_32", [this]);
  }
}

/** `Kernel.define` 回调收到的构建器 */
export class KernelBuilder {
  private readonly nodes: GraphNode[] = [];
  private scope: Scope = { parent: undefined, unroll: 1 };

  /** @internal */
  public constructor(
    private readonly reads: ReadonlyMap<string, ValueType>,
    private readonly writes: ReadonlyMap<string, ValueType>,
  ) {}

  /** @internal */
  public graphNodes(): readonly GraphNode[] {
    return this.nodes;
  }

  /** @internal 循环是否全部闭合 */
  public isRootScope(): boolean {
    return this.scope.parent === undefined;
  }

  private visible(value: RuleValue): boolean {
    for (let s: Scope | undefined = this.scope; s !== undefined; s = s.parent) {
      if (s === value.scope) {
        return true;
      }
    }
    return false;
  }

  private push(node: GraphNode, scope: Scope = this.scope): RuleValue {
    this.nodes.push(node);
    return new RuleValue(this, this.nodes.length - 1, scope);
  }

  private argIds(args: readonly RuleValue[]): number[] {
    return args.map((a) => {
      if (a.builder !== this) {
        throw typeError(`值 v${a.id} 来自另一个 Kernel`);
      }
      if (!this.visible(a)) {
        throw typeError(`值 v${a.id} 定义在已结束的循环体内，只能经 loop 的携带值（Phi）带出`);
      }
      return a.id;
    });
  }

  /** @internal 通用指令发射 */
  public op(op: Opcode, args: readonly RuleValue[]): RuleValue {
    const ids = this.argIds(args);
    return this.push(ids.length === 0 ? { op } : { op, args: ids });
  }

  // ---- 常量 ----

  public bool(value: boolean): RuleValue {
    return this.push({ op: "ConstBool", imm: value ? "true" : "false" });
  }

  public i32(value: number): RuleValue {
    if (!Number.isInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff) {
      throw typeError(`ConstI32 超出 i32 范围：${value}`);
    }
    return this.push({ op: "ConstI32", imm: String(value) });
  }

  /** Q8.8 常量：传入 `Q8_8` 实例，或可被 1/256 精确表示的实数 */
  public q8(value: Q8_8 | number): RuleValue {
    const raw = typeof value === "number" ? value * 256 : value.raw;
    if (!Number.isInteger(raw) || raw < -0x8000 || raw > 0x7fff) {
      throw typeError(`ConstQ8_8 无法精确表示：${String(value)}`);
    }
    return this.push({ op: "ConstQ8_8", imm: String(raw) });
  }

  /** Q32.32 常量：传入 `Q32_32` 实例或整数 */
  public q32(value: Q32_32 | number): RuleValue {
    let raw: bigint;
    if (typeof value === "number") {
      if (!Number.isInteger(value)) {
        throw typeError(`ConstQ32_32 的 number 形式仅接受整数（小数请用 Q32_32 实例）：${value}`);
      }
      raw = Q32_32.fromInt(value).raw;
    } else {
      raw = value.raw;
    }
    return this.push({ op: "ConstQ32_32", imm: raw.toString() });
  }

  // ---- 组件 / 场 / 邻居 / 随机 ----

  private checkRead(component: string): void {
    if (!this.reads.has(component)) {
      throw typeError(`读取了未在 reads 中声明的组件 \`${component}\``);
    }
  }

  public load(component: string): RuleValue {
    this.checkRead(component);
    return this.push({ op: "LoadComponent", component });
  }

  public write(component: string, value: RuleValue): void {
    if (!this.writes.has(component) || component.startsWith(FIELD_WRITE_PREFIX)) {
      throw new RynvekError(RynvekErrorCode.RuleUndeclaredWriteAccess, `写入未声明的组件 \`${component}\``);
    }
    const [id] = this.argIds([value]);
    this.push({ op: "EmitComponentWrite", args: [id as number], component });
  }

  private channelOf(channel: string): FieldChannel {
    if (!(FIELD_CHANNEL_NAMES as readonly string[]).includes(channel)) {
      throw typeError(`未知场通道 \`${channel}\``);
    }
    return channel as FieldChannel;
  }

  public sampleField(channel: FieldChannel): RuleValue {
    return this.push({ op: "SampleField", channel: this.channelOf(channel) });
  }

  public deposit(channel: FieldChannel, value: RuleValue): void {
    const ch = this.channelOf(channel);
    if (!this.writes.has(`${FIELD_WRITE_PREFIX}${ch}`)) {
      throw new RynvekError(
        RynvekErrorCode.RuleUndeclaredWriteAccess,
        `向未声明的场通道 \`${ch}\` 沉积（需在 writes 中声明 \`${FIELD_WRITE_PREFIX}${ch}\`）`,
      );
    }
    const [id] = this.argIds([value]);
    this.push({ op: "EmitFieldDeposit", args: [id as number], channel: ch });
  }

  public neighborCount(): RuleValue {
    return this.push({ op: "NeighborCount" });
  }

  /** 第 `index` 个邻居的组件值；越界返回 0 */
  public loadNeighbor(component: string, index: RuleValue): RuleValue {
    this.checkRead(component);
    const [id] = this.argIds([index]);
    return this.push({ op: "LoadNeighborComponent", args: [id as number], component });
  }

  /** `[0, 1)` 的 Q8.8 确定性随机数（Philox，流号 `stream`） */
  public random(stream: number): RuleValue {
    if (!Number.isInteger(stream) || stream < 0 || stream > 0xffff) {
      throw typeError(`随机流号必须为 u16：${stream}`);
    }
    return this.push({ op: "RandomQ8_8", stream });
  }

  // ---- 有界循环 ----

  /**
   * 有界循环：`body(i, carried)` 返回下一迭代的携带值；返回循环结束后的携带值（Phi）。
   *
   * 循环体内定义的值不可在循环外使用，必须通过携带值带出。
   */
  public loop(
    maxIters: number,
    initial: readonly RuleValue[],
    body: (index: RuleValue, carried: readonly RuleValue[]) => readonly RuleValue[],
  ): RuleValue[] {
    if (!Number.isInteger(maxIters) || maxIters < 1 || maxIters > MAX_LOOP_ITERS) {
      throw new RynvekError(
        RynvekErrorCode.RuleLoopBoundExceeded,
        `BoundedForBegin max_iters=${maxIters} 超出 [1, ${MAX_LOOP_ITERS}]`,
      );
    }
    const unroll = this.scope.unroll * maxIters;
    if (unroll > MAX_NESTED_UNROLL) {
      throw new RynvekError(
        RynvekErrorCode.RuleLoopBoundExceeded,
        `嵌套循环总展开次数 ${unroll} > ${MAX_NESTED_UNROLL}`,
      );
    }
    const initIds = this.argIds(initial);
    const outer = this.scope;
    const inner: Scope = { parent: outer, unroll };
    const begin = this.push({ op: "BoundedForBegin", maxIters }, inner);
    // Phi 的值在循环后仍可见，故归属外层作用域；回边先占位，循环体构建完后回填
    const phiIdx: number[] = [];
    const phis = initIds.map((init) => {
      const v = this.push({ op: "Phi", args: [init, init] }, outer);
      phiIdx.push(v.id);
      return v;
    });
    this.scope = inner;
    const next = body(begin, phis);
    if (next.length !== phis.length) {
      throw typeError(`循环体返回 ${next.length} 个携带值，期望 ${phis.length} 个`);
    }
    const nextIds = this.argIds(next);
    phiIdx.forEach((p, i) => {
      const init = initIds[i] as number;
      this.nodes[p] = { op: "Phi", args: [init, nextIds[i] as number] };
    });
    this.push({ op: "BoundedForEnd" });
    this.scope = outer;
    return phis;
  }
}

/** 已定义的规则 */
export interface RuleKernel {
  readonly name: string;
  readonly graph: UntypedRuleGraph;
  /** 规范 JSON 文本，可直接交给 Rust `compile_rule_graph_json` */
  toJSON(): string;
}

function decls(record: Readonly<Record<string, ValueType>> | undefined): ComponentDecl[] {
  return Object.entries(record ?? {}).map(([component, ty]) => ({ component, ty }));
}

export const Kernel = {
  /** 定义一条 Tier-0 规则 */
  define(name: string, declaration: KernelDeclaration, body: (k: KernelBuilder) => void): RuleKernel {
    if (name.length === 0) {
      throw typeError("规则名不能为空");
    }
    const reads = decls(declaration.reads);
    const writes = decls(declaration.writes);
    for (const w of writes) {
      if (w.component.startsWith(FIELD_WRITE_PREFIX)) {
        const ch = w.component.slice(FIELD_WRITE_PREFIX.length);
        if (!(FIELD_CHANNEL_NAMES as readonly string[]).includes(ch) || w.ty !== "Q8_8") {
          throw typeError(`场写入声明 \`${w.component}\` 非法（通道须为 5 通道之一，类型须为 Q8_8）`);
        }
      }
    }
    const builder = new KernelBuilder(
      new Map(reads.map((d) => [d.component, d.ty])),
      new Map(writes.map((d) => [d.component, d.ty])),
    );
    body(builder);
    if (!builder.isRootScope()) {
      throw typeError("存在未闭合的循环");
    }
    const graph: UntypedRuleGraph = {
      format: RULE_GRAPH_FORMAT,
      name,
      reads,
      writes,
      nodes: [...builder.graphNodes()],
    };
    return { name, graph, toJSON: () => canonicalGraphJson(graph) };
  },
} as const;
