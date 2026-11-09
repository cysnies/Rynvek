// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 协议标识的文本表示，与 Rust `rynvek_protocol::ids` 一致：
 * UUID 为小写连字符形式，32 字节摘要为 64 位小写十六进制。
 */

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const DIGEST256_PATTERN = /^[0-9a-f]{64}$/i;
const UUID_V7_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

/** UUIDv7 时间戳字段的位宽（毫秒） */
const UUID_V7_TIMESTAMP_BITS = 48n;

/** 是否为合法 UUID 文本 */
export function isWireUuid(text: string): boolean {
  return UUID_PATTERN.test(text);
}

/** 是否为合法 UUIDv7（RFC 9562 变体）文本，命令幂等键必须满足 */
export function isIdempotencyKey(text: string): boolean {
  return UUID_V7_PATTERN.test(text);
}

/** 是否为合法 32 字节摘要文本 */
export function isDigest256(text: string): boolean {
  return DIGEST256_PATTERN.test(text);
}

/** 提取 UUIDv7 中嵌入的 Unix 毫秒时间戳 */
export function idempotencyKeyTimestampMs(key: string): bigint {
  if (!isIdempotencyKey(key)) {
    throw new TypeError(`不是 UUIDv7：${key}`);
  }
  return BigInt(`0x${key.slice(0, 8)}${key.slice(9, 13)}`);
}

/**
 * 生成 UUIDv7 幂等键。
 *
 * @param unixMs 嵌入的 Unix 毫秒时间戳
 * @param random10 至少 10 字节随机数（调用方注入，便于测试与确定性重放）
 */
export function createIdempotencyKey(unixMs: bigint, random10: Uint8Array): string {
  if (unixMs < 0n || unixMs >= 1n << UUID_V7_TIMESTAMP_BITS) {
    throw new RangeError(`UUIDv7 时间戳超出 48 位：${unixMs}`);
  }
  if (random10.length < 10) {
    throw new RangeError("UUIDv7 需要至少 10 字节随机数");
  }
  const bytes = new Uint8Array(16);
  for (let i = 0; i < 6; i++) {
    bytes[i] = Number((unixMs >> BigInt(8 * (5 - i))) & 0xffn);
  }
  bytes.set(random10.subarray(0, 10), 6);
  bytes[6] = 0x70 | ((bytes[6] ?? 0) & 0x0f); // 版本 7
  bytes[8] = 0x80 | ((bytes[8] ?? 0) & 0x3f); // RFC 9562 变体
  return formatUuid(bytes);
}

/** 把 16 字节格式化为小写连字符 UUID 文本 */
export function formatUuid(bytes: Uint8Array): string {
  if (bytes.length !== 16) {
    throw new RangeError("UUID 必须为 16 字节");
  }
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
