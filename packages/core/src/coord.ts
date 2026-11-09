// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 统一权威坐标：`Chunk(i32, i32) + Q32_32` 局部偏移（规范 02 §2.1），与 Rust `WorldPosition2D` 一致。
 */

import { CHUNK_SIZE_UNITS } from "./constants.js";
import { RynvekError, RynvekErrorCode } from "./error.js";
import { Q32_32 } from "./fixed.js";

/** 单个 Chunk 边长对应的 Q32.32 原始值（`256 << 32`） */
export const CHUNK_SPAN_RAW = BigInt(CHUNK_SIZE_UNITS) << 32n;

const I32_MIN = -(2n ** 31n);
const I32_MAX = 2n ** 31n - 1n;

/** 二维权威世界坐标 */
export interface WorldPosition2D {
  readonly chunkX: number;
  readonly chunkY: number;
  readonly localX: Q32_32;
  readonly localY: Q32_32;
}

/** 是否满足规范形式（不变量 `INV-05`：`0 <= local < 256 × 2^32`） */
export function isCanonicalPosition(pos: WorldPosition2D): boolean {
  return isCanonicalLocal(pos.localX) && isCanonicalLocal(pos.localY);
}

/**
 * 将越界的局部偏移进位/借位到相邻 Chunk（欧几里得除法）。
 *
 * @throws {RynvekError} Chunk 坐标超出 i32 时抛出 `CoordinateOutOfCanonicalRange`
 */
export function canonicalizePosition(pos: WorldPosition2D): WorldPosition2D {
  const [chunkX, localX] = carryAxis(pos.chunkX, pos.localX);
  const [chunkY, localY] = carryAxis(pos.chunkY, pos.localY);
  return { chunkX, chunkY, localX, localY };
}

function isCanonicalLocal(local: Q32_32): boolean {
  return local.raw >= 0n && local.raw < CHUNK_SPAN_RAW;
}

/** bigint 的欧几里得除法：余数恒为非负 */
function divRemEuclid(value: bigint, divisor: bigint): [bigint, bigint] {
  let quotient = value / divisor;
  let remainder = value % divisor;
  if (remainder < 0n) {
    quotient -= 1n;
    remainder += divisor;
  }
  return [quotient, remainder];
}

function carryAxis(chunk: number, local: Q32_32): [number, Q32_32] {
  const [carry, newLocal] = divRemEuclid(local.raw, CHUNK_SPAN_RAW);
  const newChunk = BigInt(chunk) + carry;
  if (newChunk < I32_MIN || newChunk > I32_MAX) {
    throw new RynvekError(RynvekErrorCode.CoordinateOutOfCanonicalRange, `chunk ${newChunk} 超出 i32`);
  }
  return [Number(newChunk), Q32_32.fromRaw(newLocal)];
}
