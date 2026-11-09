// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 跨语言 Golden Test Vectors（规范 12 §12.3）：TypeScript 端必须与 Rust 端产生逐位相同的结果。
 * GV-004 属于神经运行时（`rynvek-neural` / M3），不在 `@rynvek/core` 的职责范围内。
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { Q32_32, type U32x2, type U32x4, canonicalizePosition, philox4x32_10 } from "../src/index.js";

interface GoldenVector {
  readonly id: string;
  readonly inputs: Record<string, unknown>;
  readonly expected: Record<string, unknown>;
}

const BASELINE = fileURLToPath(new URL("../../../tests/golden_vectors/v1_baseline.json", import.meta.url));
const suite = JSON.parse(readFileSync(BASELINE, "utf8")) as { suite: string; vectors: GoldenVector[] };

function vector(id: string): GoldenVector {
  const found = suite.vectors.find((v) => v.id === id);
  if (found === undefined) {
    throw new Error(`基线中缺少向量 ${id}`);
  }
  return found;
}

/** 64 位整数在基线中以十进制字符串存储 */
function rawField(record: Record<string, unknown>, key: string): bigint {
  const value = record[key];
  if (typeof value !== "string") {
    throw new TypeError(`${key} 必须是十进制字符串`);
  }
  return BigInt(value);
}

describe(`Golden Vectors · ${suite.suite}`, () => {
  it("GV-001-Q32_32_ARITHMETIC", () => {
    const { inputs, expected } = vector("GV-001-Q32_32_ARITHMETIC");
    const a = Q32_32.fromRaw(rawField(inputs, "a_raw_i64"));
    const b = Q32_32.fromRaw(rawField(inputs, "b_raw_i64"));
    const target = Q32_32.fromRaw(rawField(inputs, "sqrt_target_raw_i64"));
    expect(a.mul(b).raw).toBe(rawField(expected, "mul_raw_i64"));
    expect(a.divSafe(b, Q32_32.ZERO).raw).toBe(rawField(expected, "div_raw_i64"));
    expect(target.sqrtDeterministic().raw).toBe(rawField(expected, "sqrt_raw_i64"));
  });

  it("GV-002-WORLD_POS_CANONICALIZE", () => {
    const { inputs, expected } = vector("GV-002-WORLD_POS_CANONICALIZE");
    const out = canonicalizePosition({
      chunkX: inputs["chunk_x"] as number,
      chunkY: inputs["chunk_y"] as number,
      localX: Q32_32.fromRaw(rawField(inputs, "local_x_raw")),
      localY: Q32_32.fromRaw(rawField(inputs, "local_y_raw")),
    });
    expect(out.chunkX).toBe(expected["chunk_x"]);
    expect(out.chunkY).toBe(expected["chunk_y"]);
    expect(out.localX.raw).toBe(rawField(expected, "local_x_raw"));
    expect(out.localY.raw).toBe(rawField(expected, "local_y_raw"));
  });

  it("GV-003-PHILOX4X32_10_ORIGIN", () => {
    const { inputs, expected } = vector("GV-003-PHILOX4X32_10_ORIGIN");
    const out = philox4x32_10(inputs["ctr_u32x4"] as U32x4, inputs["key_u32x2"] as U32x2);
    expect(out).toEqual(expected["out_u32x4"]);
  });
});
