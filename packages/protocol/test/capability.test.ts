// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";

import {
  CAPABILITY_SCOPES_KNOWN_MASK,
  CapabilityTokenError,
  type CapabilityToken,
  capabilityClaimsBytes,
  capabilityGrants,
  decodeCapabilityToken,
  encodeCapabilityToken,
  scopeSetOf,
  scopesIn,
} from "../src/index.js";

/** 与 Rust `capability::tests::SAMPLE_CLAIMS_HEX` 完全相同的跨语言声明区 */
const SAMPLE_CLAIMS_HEX = "01000700efbeaddee903000000000000240000000000000000d2496b00000000";

const hex = (bytes: Uint8Array): string => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

const sample = (): CapabilityToken => ({
  version: 1,
  signerKeyId: 7,
  nonce: 0xdeadbeef,
  principalId: 1001n,
  scopes: scopeSetOf(["world.read", "world.connect"]),
  expiresAtUnixS: 1_800_000_000n,
  signature: new Uint8Array(64).fill(0x5a),
});

describe("CapabilityToken", () => {
  it("声明区与 Rust 参考字节一致", () => {
    expect(hex(capabilityClaimsBytes(sample()))).toBe(SAMPLE_CLAIMS_HEX);
  });

  it("编解码往返", () => {
    const bytes = encodeCapabilityToken(sample());
    expect(bytes.length).toBe(96);
    expect(decodeCapabilityToken(bytes)).toEqual(sample());
  });

  it("拒绝非法令牌", () => {
    const good = encodeCapabilityToken(sample());
    const mutate = (index: number, value: number): Uint8Array => {
      const copy = good.slice();
      copy[index] = value;
      return copy;
    };
    expect(() => decodeCapabilityToken(good.subarray(0, 95))).toThrow(CapabilityTokenError);
    expect(() => decodeCapabilityToken(mutate(0, 9))).toThrow(/版本/);
    expect(() => decodeCapabilityToken(mutate(1, 1))).toThrow(/保留字节/);
    expect(() => decodeCapabilityToken(mutate(17, 0x80))).toThrow(/未定义位/);
  });

  it("授权判断同时检查权限与过期时间", () => {
    const token = sample();
    expect(capabilityGrants(token, "world.read", 1_799_999_999n)).toBe(true);
    expect(capabilityGrants(token, "world.read", 1_800_000_000n)).toBe(false);
    expect(capabilityGrants(token, "hub.publish", 0n)).toBe(false);
  });

  it("权限位序与 Rust CapabilityScope 一致", () => {
    expect(CAPABILITY_SCOPES_KNOWN_MASK).toBe(0x7ffn);
    expect(scopesIn(scopeSetOf(["lab.read", "world.list"]))).toEqual(["world.list", "lab.read"]);
    expect(scopeSetOf(["account.owner"])).toBe(1n << 10n);
  });
});
