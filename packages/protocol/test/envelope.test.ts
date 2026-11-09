// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";

import {
  type EventKey,
  checkTickWindow,
  compareEventKeys,
  createIdempotencyKey,
  idempotencyKeyTimestampMs,
  isDigest256,
  isIdempotencyKey,
  isWireUuid,
} from "../src/index.js";

/** RFC 9562 附录 A.6 的 UUIDv7 示例（与 Rust 测试相同） */
const RFC9562_V7 = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";

describe("标识", () => {
  it("UUIDv7 校验与时间戳提取", () => {
    expect(isIdempotencyKey(RFC9562_V7)).toBe(true);
    expect(isIdempotencyKey("919108f7-52d1-4320-9bac-f847db4148a8")).toBe(false);
    expect(isWireUuid("919108f7-52d1-4320-9bac-f847db4148a8")).toBe(true);
    expect(idempotencyKeyTimestampMs(RFC9562_V7)).toBe(0x017f22e279b0n);
  });

  it("按 RFC 9562 生成 UUIDv7", () => {
    const random = Uint8Array.from([0x0c, 0xc3, 0x18, 0xc4, 0xdc, 0x0c, 0x0c, 0x07, 0x39, 0x8f]);
    expect(createIdempotencyKey(0x017f22e279b0n, random)).toBe(RFC9562_V7);
    expect(() => createIdempotencyKey(1n << 48n, random)).toThrow(RangeError);
  });

  it("摘要文本校验", () => {
    expect(isDigest256("ab".repeat(32))).toBe(true);
    expect(isDigest256("ab")).toBe(false);
  });
});

describe("信封", () => {
  const window = { maxPastTicks: 2, maxFutureTicks: 30 };

  it("Tick 时间窗边界包含", () => {
    expect(checkTickWindow({ target_tick: 98 }, 100, window)).toBe("Ok");
    expect(checkTickWindow({ target_tick: 130 }, 100, window)).toBe("Ok");
    expect(checkTickWindow({ target_tick: 97 }, 100, window)).toBe("Expired");
    expect(checkTickWindow({ target_tick: 131 }, 100, window)).toBe("TooFarInFuture");
  });

  it("事件键按因果全序排序（与 Rust 测试相同的用例）", () => {
    const base: EventKey = { tick: 10, phase_id: 9, emitter_serial: 5, sequence: 0 };
    const keys: EventKey[] = [
      { ...base, sequence: 1 },
      { ...base, tick: 9 },
      { ...base, phase_id: 15 },
      base,
      { ...base, emitter_serial: 1 },
    ];
    expect(keys.sort(compareEventKeys)).toEqual([
      { ...base, tick: 9 },
      { ...base, emitter_serial: 1 },
      base,
      { ...base, sequence: 1 },
      { ...base, phase_id: 15 },
    ]);
  });
});
