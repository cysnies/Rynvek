// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";

import { Q32_32, Q8_8 } from "../src/index.js";

const q = (raw: bigint): Q32_32 => Q32_32.fromRaw(raw);

describe("Q32_32", () => {
  it("常量精确", () => {
    expect(Q32_32.ONE.equals(Q32_32.fromInt(1))).toBe(true);
    expect(Q32_32.NEG_ONE.equals(Q32_32.fromInt(-1))).toBe(true);
    expect(Q32_32.DT_30HZ.raw).toBe((1n << 32n) / 30n);
  });

  it("fromMilli 与 toIntFloor", () => {
    expect(Q32_32.fromMilli(1000n).equals(Q32_32.ONE)).toBe(true);
    expect(Q32_32.fromMilli(-1500n).toIntFloor()).toBe(-2);
    expect(Q32_32.fromMilli(2999n).toIntFloor()).toBe(2);
  });

  it("乘法、除法与零除回退", () => {
    expect(q(10_737_418_240n).mul(q(6_442_450_944n)).raw).toBe(16_106_127_360n);
    expect(q(10_737_418_240n).divSafe(q(6_442_450_944n), Q32_32.ZERO).raw).toBe(7_158_278_826n);
    expect(Q32_32.ONE.divSafe(Q32_32.ZERO, Q32_32.NEG_ONE)).toBe(Q32_32.NEG_ONE);
    expect(Q32_32.NEG_ONE.mul(q(10_737_418_240n)).raw).toBe(-10_737_418_240n);
  });

  it("平方根为真实平方根的下取整", () => {
    expect(Q32_32.fromInt(9).sqrtDeterministic().equals(Q32_32.fromInt(3))).toBe(true);
    expect(Q32_32.NEG_ONE.sqrtDeterministic()).toBe(Q32_32.ZERO);
    for (const raw of [1n, 2n, 3n, 12_345n, 1n << 20n, 987_654_321_012n, (1n << 63n) - 1n]) {
      const res = q(raw).sqrtDeterministic().raw;
      const val = raw << 32n;
      expect(res * res <= val).toBe(true);
      expect((res + 1n) * (res + 1n) > val).toBe(true);
    }
  });

  it("饱和加减与范围校验", () => {
    const max = q((1n << 63n) - 1n);
    expect(max.saturatingAdd(Q32_32.ONE).equals(max)).toBe(true);
    expect(q(-(1n << 63n)).saturatingSub(Q32_32.ONE).raw).toBe(-(1n << 63n));
    expect(() => Q32_32.fromRaw(1n << 63n)).toThrow(RangeError);
    expect(() => Q32_32.fromInt(2 ** 31)).toThrow(RangeError);
  });
});

describe("Q8_8", () => {
  it("乘法饱和与 hardTanh 钳制", () => {
    expect(Q8_8.ONE.mul(Q8_8.NEG_ONE).equals(Q8_8.NEG_ONE)).toBe(true);
    expect(Q8_8.fromRaw(128).mul(Q8_8.fromRaw(128)).raw).toBe(64);
    expect(Q8_8.fromRaw(0x7fff).mul(Q8_8.fromRaw(0x7fff)).raw).toBe(0x7fff);
    expect(Q8_8.fromRaw(-0x8000).mul(Q8_8.fromRaw(0x7fff)).raw).toBe(-0x8000);
    expect(Q8_8.fromRaw(-3).mul(Q8_8.fromRaw(1)).raw).toBe(-1); // 算术右移向负无穷取整
    expect(Q8_8.fromRaw(1000).hardTanh().raw).toBe(256);
    expect(Q8_8.fromRaw(-1000).hardTanh().raw).toBe(-256);
    expect(() => Q8_8.fromRaw(40_000)).toThrow(RangeError);
  });
});
