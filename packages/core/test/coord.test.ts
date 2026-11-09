// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";

import {
  CHUNK_SPAN_RAW,
  Q32_32,
  RynvekError,
  RynvekErrorCode,
  canonicalizePosition,
  isCanonicalPosition,
} from "../src/index.js";

describe("WorldPosition2D", () => {
  it("正好位于边界时进位到下一个 Chunk", () => {
    const pos = { chunkX: 0, chunkY: 0, localX: Q32_32.fromRaw(CHUNK_SPAN_RAW), localY: Q32_32.fromRaw(-CHUNK_SPAN_RAW) };
    const out = canonicalizePosition(pos);
    expect([out.chunkX, out.chunkY, out.localX.raw, out.localY.raw]).toEqual([1, -1, 0n, 0n]);
    expect(isCanonicalPosition(out)).toBe(true);
    expect(isCanonicalPosition(pos)).toBe(false);
  });

  it("Chunk 溢出 i32 时抛出统一错误码", () => {
    const pos = { chunkX: 2 ** 31 - 1, chunkY: 0, localX: Q32_32.fromRaw(CHUNK_SPAN_RAW), localY: Q32_32.ZERO };
    try {
      canonicalizePosition(pos);
      expect.unreachable("应当抛出异常");
    } catch (err) {
      expect(err).toBeInstanceOf(RynvekError);
      expect((err as RynvekError).code).toBe(RynvekErrorCode.CoordinateOutOfCanonicalRange);
    }
  });
});
