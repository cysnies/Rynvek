// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 字节工具：BLAKE3 摘要与十六进制编码。

import { blake3 } from "@noble/hashes/blake3.js";

/** BLAKE3-256 摘要 */
export function blake3Digest(...parts: readonly Uint8Array[]): Uint8Array {
  const h = blake3.create();
  for (const p of parts) h.update(p);
  return h.digest();
}

/** 小写十六进制 */
export function toHex(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}

export function fromHex(hex: string): Uint8Array {
  if (!/^(?:[0-9a-f]{2})*$/.test(hex)) throw new RangeError("非法十六进制串");
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

/** 常量时间比较（避免摘要比对的时序侧信道） */
export function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  return diff === 0;
}
