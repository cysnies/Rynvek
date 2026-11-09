// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { describe, expect, it } from "vitest";

import { Kernel, OPCODES, RULE_GRAPH_FORMAT, type KernelBuilder } from "../src/index.js";
import { SHARED_FIXTURE_DIR, foragerMetabolism } from "./fixtures.js";

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

function define(body: (k: KernelBuilder) => void, writes: Record<string, "Q8_8" | "I32"> = {}) {
  return Kernel.define("t", { reads: { "a.v": "Q8_8" }, writes }, body);
}

describe("指令集", () => {
  it("恰好 36 条且互不重复，顺序与 Rust 操作码一致", () => {
    expect(OPCODES).toHaveLength(36);
    expect(new Set(OPCODES).size).toBe(36);
    expect(OPCODES[0]).toBe("ConstBool");
    expect(OPCODES[26]).toBe("LoadComponent");
    expect(OPCODES[35]).toBe("Phi");
  });
});

describe("Kernel.define", () => {
  it("生成与共享夹具逐字节一致的规范 JSON", () => {
    const json = foragerMetabolism().toJSON();
    const path = join(SHARED_FIXTURE_DIR, "forager_metabolism.graph.json");
    if (process.env["RYNVEK_UPDATE_FIXTURES"] === "1") {
      writeFileSync(path, json);
    }
    expect(json).toBe(readFileSync(path, "utf8"));
  });

  it("记录 SSA 节点、Phi 回边与读写集", () => {
    const rule = Kernel.define(
      "sum",
      { reads: { "a.v": "Q8_8" }, writes: { "a.v": "Q8_8" } },
      (k) => {
        const [acc] = k.loop(4, [k.q8(0)], (i, [a]) => [a!.add(k.loadNeighbor("a.v", i))]);
        k.write("a.v", acc!);
      },
    );
    expect(rule.graph.format).toBe(RULE_GRAPH_FORMAT);
    expect(rule.graph.nodes).toEqual([
      { op: "ConstQ8_8", imm: "0" },
      { op: "BoundedForBegin", maxIters: 4 },
      { op: "Phi", args: [0, 4] },
      { op: "LoadNeighborComponent", args: [1], component: "a.v" },
      { op: "Add", args: [2, 3] },
      { op: "BoundedForEnd" },
      { op: "EmitComponentWrite", args: [2], component: "a.v" },
    ]);
    expect(rule.graph.reads).toEqual([{ component: "a.v", ty: "Q8_8" }]);
  });

  it("常量以十进制字符串承载原始值", () => {
    const rule = define((k) => {
      k.q8(-1.5);
      k.q32(3);
      k.i32(-7);
      k.bool(true);
    });
    expect(rule.graph.nodes.map((n) => n.imm)).toEqual(["-384", (3n << 32n).toString(), "-7", "true"]);
  });
});

describe("本端提前检查", () => {
  it("写入未声明组件 / 场通道 → 0x5002", () => {
    expect(codeOf(() => define((k) => k.write("a.v", k.load("a.v"))))).toBe(
      RynvekErrorCode.RuleUndeclaredWriteAccess,
    );
    expect(codeOf(() => define((k) => k.deposit("hazard", k.q8(1)), { "field.nutrient": "Q8_8" }))).toBe(
      RynvekErrorCode.RuleUndeclaredWriteAccess,
    );
  });

  it("循环上界 > 64 或嵌套总展开 > 256 → 0x5003", () => {
    expect(codeOf(() => define((k) => k.loop(65, [], () => [])))).toBe(RynvekErrorCode.RuleLoopBoundExceeded);
    expect(codeOf(() => define((k) => k.loop(0, [], () => [])))).toBe(RynvekErrorCode.RuleLoopBoundExceeded);
    expect(codeOf(() => define((k) => k.loop(16, [], () => (k.loop(32, [], () => []), []))))).toBe(
      RynvekErrorCode.RuleLoopBoundExceeded,
    );
    expect(codeOf(() => define((k) => k.loop(16, [], () => (k.loop(16, [], () => []), []))))).toBeUndefined();
  });

  it("越出循环作用域、跨 Kernel 使用值、非法常量与通道 → 0x5001", () => {
    expect(
      codeOf(() =>
        define((k) => {
          let leaked = k.q8(0);
          k.loop(2, [], () => {
            leaked = k.q8(1);
            return [];
          });
          leaked.neg();
        }),
      ),
    ).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    let foreign: ReturnType<KernelBuilder["q8"]> | undefined;
    define((k) => {
      foreign = k.q8(1);
    });
    expect(codeOf(() => define((k) => k.q8(1).add(foreign!)))).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    expect(codeOf(() => define((k) => k.load("b.undeclared")))).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    expect(codeOf(() => define((k) => k.q8(0.001)))).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    expect(codeOf(() => define((k) => k.q8(200)))).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    // @ts-expect-error 未知通道
    expect(codeOf(() => define((k) => k.sampleField("plasma")))).toBe(RynvekErrorCode.RuleStaticTypeCheckFailed);
    expect(codeOf(() => define((k) => k.loop(2, [k.q8(0)], () => [])))).toBe(
      RynvekErrorCode.RuleStaticTypeCheckFailed,
    );
  });
});
