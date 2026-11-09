// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { RynvekError, RynvekErrorCode, philox4x32_10 } from "@rynvek/core";
import { describe, expect, it } from "vitest";

import { DEFAULT_SANDBOX_CONFIG, GAS_PER_INTERRUPT_POLL, QuickJsSandbox } from "../src/index.js";
import { SHARED_FIXTURE_DIR } from "./fixtures.js";

const fixture = (name: string) => readFileSync(join(SHARED_FIXTURE_DIR, name), "utf8");

async function codeOfAsync(f: () => Promise<unknown>): Promise<number | undefined> {
  try {
    await f();
  } catch (e) {
    if (e instanceof RynvekError) {
      return e.code;
    }
    throw e;
  }
  return undefined;
}

function codeOf(f: () => unknown): number | undefined {
  try {
    f();
  } catch (e) {
    if (e instanceof RynvekError) {
      return e.code;
    }
    throw e;
  }
  return undefined;
}

describe("QuickJS-NG WASM 确定性沙箱", () => {
  it("共享 Tier-1 夹具输出与 Rust 宿主逐字节一致", async () => {
    const sb = await QuickJsSandbox.create(fixture("tier1_metabolism.js"));
    const out = sb.runTick(fixture("tier1_metabolism.input.json").trim());
    expect(out.outputJson).toBe(fixture("tier1_metabolism.expected.json").trim());
    const parsed = JSON.parse(out.outputJson) as { entities: { jitter: number }[] };
    [10, 11, 12].forEach((serial, i) => {
      expect(parsed.entities[i]!.jitter).toBe(philox4x32_10([serial, 7, 42, 0], [24301, 0])[0] >>> 24);
    });
    sb.dispose();
  });

  it("Gas 计量逐次确定，且随工作量增长", async () => {
    const sb = await QuickJsSandbox.create(
      "function tick(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 7) | 0; } return s; }",
    );
    const a = sb.runTick("100000");
    expect(sb.runTick("100000")).toEqual(a);
    expect(a.gasUsed).toBeGreaterThan(0);
    expect(a.gasUsed % GAS_PER_INTERRUPT_POLL).toBe(0);
    expect(sb.runTick("200000").gasUsed).toBeGreaterThan(a.gasUsed);
    sb.dispose();
  });

  it("死循环耗尽 Gas → 0x5004，且 try/catch 无法吞掉中断", async () => {
    const sb = await QuickJsSandbox.create("function tick() { for (;;) {} }");
    expect(codeOf(() => sb.runTick("null"))).toBe(RynvekErrorCode.SandboxGasBudgetExhausted);
    expect(codeOf(() => sb.runTick("null"))).toBe(RynvekErrorCode.SandboxGasBudgetExhausted);
    sb.dispose();
    const sb2 = await QuickJsSandbox.create("function tick() { try { for (;;) {} } catch (e) { return 1; } }");
    expect(codeOf(() => sb2.runTick("null"))).toBe(RynvekErrorCode.SandboxGasBudgetExhausted);
    sb2.dispose();
  });

  it("堆超限 → 0x5005", async () => {
    const sb = await QuickJsSandbox.create(
      "function tick() { return 'x'.repeat(1 << 24).length; }",
      { ...DEFAULT_SANDBOX_CONFIG, heapLimitBytes: 8 * 1024 * 1024 },
    );
    expect(codeOf(() => sb.runTick("null"))).toBe(RynvekErrorCode.SandboxMemoryLimitExceeded);
    sb.dispose();
  });

  it("不确定的内建不可用，Math.random 不可被改写", async () => {
    const sb = await QuickJsSandbox.create(
      "function tick() { let r = 'ok'; try { Math.random(); r = 'random'; } catch (e) {} " +
        "return [r, typeof Date, typeof Promise, typeof WeakRef, typeof performance, typeof Rynvek.philox4x32_10]; }",
    );
    expect(sb.runTick("null").outputJson).toBe('["ok","undefined","undefined","undefined","undefined","function"]');
    sb.dispose();
    const sb2 = await QuickJsSandbox.create(
      "function tick() { try { Math.random = () => 0.5; } catch (e) { return 'frozen'; } return 'mutable'; }",
    );
    expect(sb2.runTick("null").outputJson).toBe('"frozen"');
    sb2.dispose();
  });

  it("缺少 tick、语法错误与运行时异常 → 0x5001", async () => {
    expect(await codeOfAsync(() => QuickJsSandbox.create("var x = 1;"))).toBe(
      RynvekErrorCode.RuleStaticTypeCheckFailed,
    );
    expect(await codeOfAsync(() => QuickJsSandbox.create("function tick( {"))).toBe(
      RynvekErrorCode.RuleStaticTypeCheckFailed,
    );
    const sb = await QuickJsSandbox.create("function tick() { throw new TypeError('boom'); }");
    expect(() => sb.runTick("null")).toThrow(/boom/);
    expect(codeOf(() => sb.runTick("null"))).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    sb.dispose();
  });

  it("Tick 之间不共享全局状态", async () => {
    const sb = await QuickJsSandbox.create("var counter = 0; function tick() { counter += 1; return counter; }");
    expect(sb.runTick("null").outputJson).toBe("1");
    expect(sb.runTick("null").outputJson).toBe("1");
    sb.dispose();
  });
});
