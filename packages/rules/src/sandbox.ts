// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * Tier-1 规则：QuickJS-NG WASM 确定性沙箱（GAP-06；规范 01 §1.1：单 Tick `500,000 Gas`、堆 `64 MiB`）。
 *
 * 与 Rust `rynvek-rules::sandbox`（rquickjs 内嵌 QuickJS-NG）采用同一套确定性约定，
 * 两个宿主对同一脚本与输入给出逐字节相同的输出 JSON：
 *
 * 1. **每 Tick 新建 Context**：QuickJS-NG 的中断计数器挂在 Context 上，新建即复位，
 *    Gas 计量逐次完全相同，且杜绝跨 Tick 的隐藏状态；
 * 2. **最小内建集**：不注册 `Date` / `Promise` / `WeakRef` / `Proxy` 等不确定或可观测 GC 的内建；
 *    `Math.random` 被替换为抛错函数，确定性随机数改由宿主注入的 `Rynvek.philox4x32_10` 提供；
 * 3. **严格模式 + 纯函数接口**：脚本须定义全局 `tick(input)`，输入/输出均经 JSON 序列化，不暴露任何 I/O。
 *
 * Gas 计量（ERRATA E-011）：QuickJS-NG 每 `10,000` 次中断计数递减（函数调用与循环回边）回调一次中断处理器，
 * Gas = 回调次数 × 10,000；超过预算即中断（不可被脚本 `try/catch` 捕获）并报 `SandboxGasBudgetExhausted (0x5004)`；
 * 堆上限由 `JS_SetMemoryLimit` 强制，超限报 `SandboxMemoryLimitExceeded (0x5005)`。
 *
 * 注意：WASM 变体（`@jitl/quickjs-ng-wasmfile-release-sync`）与 Rust 侧 rquickjs 内嵌的 QuickJS-NG 小版本不同，
 * 中断回调的精确次数可能有细微差异；跨宿主只保证**输出**一致，Gas 在同一宿主内逐次确定。
 */

import variantModule from "@jitl/quickjs-ng-wasmfile-release-sync";
import { RynvekError, RynvekErrorCode, philox4x32_10 } from "@rynvek/core";
import {
  newQuickJSWASMModuleFromVariant,
  type Intrinsics,
  type QuickJSContext,
  type QuickJSHandle,
  type QuickJSRuntime,
  type QuickJSSyncVariant,
  type QuickJSWASMModule,
} from "quickjs-emscripten-core";

/** 单 Tick Gas 预算 */
export const TIER1_TICK_GAS_BUDGET = 500_000;
/** 堆上限 */
export const TIER1_HEAP_LIMIT_BYTES = 64 * 1024 * 1024;
/** 每次中断回调折算的 Gas */
export const GAS_PER_INTERRUPT_POLL = 10_000;
/** 栈上限 */
export const TIER1_STACK_LIMIT_BYTES = 512 * 1024;

export interface QuickJsSandboxConfig {
  readonly gasBudget: number;
  readonly heapLimitBytes: number;
  readonly stackLimitBytes: number;
}

export const DEFAULT_SANDBOX_CONFIG: QuickJsSandboxConfig = Object.freeze({
  gasBudget: TIER1_TICK_GAS_BUDGET,
  heapLimitBytes: TIER1_HEAP_LIMIT_BYTES,
  stackLimitBytes: TIER1_STACK_LIMIT_BYTES,
});

export interface SandboxTickOutput {
  /** `tick(input)` 返回值的 JSON */
  readonly outputJson: string;
  readonly gasUsed: number;
}

/** 只允许确定性的内建对象（与 Rust 侧 `DeterministicIntrinsics` 一致） */
const DETERMINISTIC_INTRINSICS: Intrinsics = {
  BaseObjects: true,
  Eval: true,
  JSON: true,
  MapSet: true,
  TypedArrays: true,
  BigInt: true,
  RegExp: true,
  RegExpCompiler: true,
};

/** 确定性加固前导：在用户脚本之前执行（与 Rust 侧逐字一致） */
const HARDEN_PRELUDE = `
"use strict";
Object.defineProperty(Math, "random", {
    value: function () { throw new Error("Math.random 在确定性沙箱中被禁用，请使用 Rynvek.philox4x32_10"); },
    writable: false,
    configurable: false,
});
`;

/** 规则脚本一律以严格模式求值（与 rquickjs `EvalOptions::default()` 的 `strict: true` 一致），写只读属性会抛错而非静默失败 */
const STRICT_GLOBAL = { type: "global", strict: true } as const;

// 该变体包的 "types" 指向 CJS 形态的 .d.ts：NodeNext 下默认导入在类型上是整个模块对象，
// 而运行时 ESM 入口（index.mjs）的默认导出才是变体本身；两种形态都兼容地取出变体
const variant: QuickJSSyncVariant =
  (variantModule as unknown as { default?: QuickJSSyncVariant }).default ??
  (variantModule as unknown as QuickJSSyncVariant);

let modulePromise: Promise<QuickJSWASMModule> | undefined;

/** 加载（并缓存）QuickJS-NG WASM 模块 */
export function loadQuickJsNg(): Promise<QuickJSWASMModule> {
  modulePromise ??= newQuickJSWASMModuleFromVariant(variant);
  return modulePromise;
}

function u32At(list: unknown, i: number): number {
  if (!Array.isArray(list)) {
    return 0;
  }
  const v: unknown = list[i];
  return typeof v === "number" ? v >>> 0 : 0;
}

/** QuickJS-NG Tier-1 规则沙箱 */
export class QuickJsSandbox {
  private polls = 0;

  private constructor(
    private readonly runtime: QuickJSRuntime,
    private readonly source: string,
    public readonly config: QuickJsSandboxConfig,
  ) {}

  /** 加载规则脚本；脚本须定义全局函数 `tick(input)` */
  public static async create(
    source: string,
    config: QuickJsSandboxConfig = DEFAULT_SANDBOX_CONFIG,
  ): Promise<QuickJsSandbox> {
    const qjs = await loadQuickJsNg();
    const runtime = qjs.newRuntime();
    runtime.setMemoryLimit(config.heapLimitBytes);
    runtime.setMaxStackSize(config.stackLimitBytes);
    const sandbox = new QuickJsSandbox(runtime, source, config);
    const maxPolls = Math.floor(config.gasBudget / GAS_PER_INTERRUPT_POLL);
    runtime.setInterruptHandler(() => {
      sandbox.polls += 1;
      return sandbox.polls > maxPolls;
    });
    try {
      sandbox.withFreshContext((vm) => {
        const tick = vm.getProp(vm.global, "tick");
        const kind = vm.typeof(tick);
        tick.dispose();
        if (kind !== "function") {
          throw new RynvekError(RynvekErrorCode.RuleStaticTypeCheckFailed, "规则脚本未定义全局函数 tick(input)");
        }
      });
    } catch (e) {
      runtime.dispose();
      throw e;
    }
    return sandbox;
  }

  /** 释放 WASM 运行时 */
  public dispose(): void {
    this.runtime.dispose();
  }

  /** 以 JSON 输入执行一次 `tick(input)` */
  public runTick(inputJson: string): SandboxTickOutput {
    const outputJson = this.withFreshContext((vm) => {
      const json = vm.getProp(vm.global, "JSON");
      const parse = vm.getProp(json, "parse");
      const stringify = vm.getProp(json, "stringify");
      const tick = vm.getProp(vm.global, "tick");
      const inputStr = vm.newString(inputJson);
      const handles: QuickJSHandle[] = [json, parse, stringify, tick, inputStr];
      try {
        const input = this.unwrap(vm, vm.callFunction(parse, vm.undefined, inputStr));
        handles.push(input);
        const output = this.unwrap(vm, vm.callFunction(tick, vm.undefined, input));
        handles.push(output);
        const text = this.unwrap(vm, vm.callFunction(stringify, vm.undefined, output));
        handles.push(text);
        return vm.typeof(text) === "string" ? vm.getString(text) : "null";
      } finally {
        for (const h of handles) {
          h.dispose();
        }
      }
    });
    return { outputJson, gasUsed: this.polls * GAS_PER_INTERRUPT_POLL };
  }

  /** 新建确定性 Context，执行加固前导与用户脚本后调用 `f`；结束后丢弃 Context */
  private withFreshContext<T>(f: (vm: QuickJSContext) => T): T {
    this.polls = 0;
    let vm: QuickJSContext;
    try {
      vm = this.runtime.newContext({ intrinsics: DETERMINISTIC_INTRINSICS });
    } catch (e) {
      throw this.classify(String(e));
    }
    try {
      this.unwrap(vm, vm.evalCode(HARDEN_PRELUDE, "rynvek-prelude.js", STRICT_GLOBAL)).dispose();
      this.installHostApi(vm);
      this.unwrap(vm, vm.evalCode(this.source, "rule.js", STRICT_GLOBAL)).dispose();
      return f(vm);
    } finally {
      vm.dispose();
    }
  }

  /** 注入 `Rynvek` 宿主对象（仅确定性纯函数） */
  private installHostApi(vm: QuickJSContext): void {
    const api = vm.newObject();
    const philox = vm.newFunction("philox4x32_10", (ctrH, keyH) => {
      const ctr: unknown = ctrH === undefined ? undefined : vm.dump(ctrH);
      const key: unknown = keyH === undefined ? undefined : vm.dump(keyH);
      const out = philox4x32_10(
        [u32At(ctr, 0), u32At(ctr, 1), u32At(ctr, 2), u32At(ctr, 3)],
        [u32At(key, 0), u32At(key, 1)],
      );
      const arr = vm.newArray();
      out.forEach((word, i) => {
        const n = vm.newNumber(word);
        vm.setProp(arr, i, n);
        n.dispose();
      });
      return arr;
    });
    vm.setProp(api, "philox4x32_10", philox);
    vm.setProp(vm.global, "Rynvek", api);
    philox.dispose();
    api.dispose();
  }

  private unwrap(
    vm: QuickJSContext,
    result: { error: QuickJSHandle; value?: undefined } | { error?: undefined; value: QuickJSHandle },
  ): QuickJSHandle {
    if (result.error === undefined) {
      return result.value;
    }
    let message: string;
    try {
      const err: unknown = vm.dump(result.error);
      message =
        typeof err === "object" && err !== null && "message" in err ? String(err.message) : String(err);
    } catch {
      message = "未知 JS 异常";
    }
    result.error.dispose();
    throw this.classify(message);
  }

  /** 把 QuickJS 异常归类为规范错误码 */
  private classify(message: string): RynvekError {
    if (this.polls * GAS_PER_INTERRUPT_POLL > this.config.gasBudget) {
      return new RynvekError(
        RynvekErrorCode.SandboxGasBudgetExhausted,
        `单 Tick Gas 预算 ${this.config.gasBudget} 已耗尽`,
      );
    }
    if (message.includes("out of memory") || message.includes("Allocation")) {
      return new RynvekError(RynvekErrorCode.SandboxMemoryLimitExceeded, message);
    }
    return new RynvekError(RynvekErrorCode.RuleStaticTypeCheckFailed, `规则脚本异常：${message}`);
  }
}
