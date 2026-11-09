// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";

import { generateU32x4, mulHiLoU32, packStreamAndPhase, philox4x32_10, randomKeyCounter, randomKeyKey } from "../src/index.js";

describe("Philox4x32-10 · Random123 官方 Known-Answer 向量", () => {
  it("全零", () => {
    expect(philox4x32_10([0, 0, 0, 0], [0, 0])).toEqual([0x6627e8d5, 0xe169c58d, 0xbc57ac4c, 0x9b00dbd8]);
  });

  it("全 1", () => {
    const ones = 0xffffffff;
    expect(philox4x32_10([ones, ones, ones, ones], [ones, ones])).toEqual([0x408f276d, 0x41c83b0e, 0xa20bc7c6, 0x6d5451fd]);
  });

  it("圆周率位", () => {
    const out = philox4x32_10([0x243f6a88, 0x85a308d3, 0x13198a2e, 0x03707344], [0xa4093822, 0x299f31d0]);
    expect(out).toEqual([0xd16cfe09, 0x94fdcceb, 0x5001e420, 0x24126ea1]);
  });
});

describe("mulHiLoU32", () => {
  it("与 64 位精确乘积一致", () => {
    const samples: [number, number][] = [
      [0xffffffff, 0xffffffff],
      [0xd2511f53, 0x12345678],
      [0xcd9e8d57, 0x80000000],
      [1, 0],
    ];
    for (const [a, b] of samples) {
      const product = BigInt(a) * BigInt(b);
      expect(mulHiLoU32(a, b)).toEqual([Number(product & 0xffffffffn), Number(product >> 32n)]);
    }
  });
});

describe("RandomKey", () => {
  it("字段映射与 Rust 实现一致", () => {
    const rk = {
      worldSeed: 0x0123_4567_89ab_cdefn,
      tick: 0x10n,
      entitySerial: 0xaaaa_bbbb_cccc_ddddn,
      streamAndPhase: packStreamAndPhase(1, 7),
      subCounter: 3,
    };
    expect(randomKeyCounter(rk)).toEqual([3, 0x0001_0007, 0xcccc_dddd, 0xaaaa_bbbb]);
    expect(randomKeyKey(rk)).toEqual([0x89ab_cdff, 0x0123_4567]);
    expect(generateU32x4(rk)).toEqual(philox4x32_10(randomKeyCounter(rk), randomKeyKey(rk)));
  });
});
