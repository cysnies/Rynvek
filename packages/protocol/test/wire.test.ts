// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";

import {
  MAX_WIRE_PAYLOAD_LEN,
  WireChannelId,
  WireError,
  WireFrameFlag,
  createWireFrameHeader,
  decodeWireFrameHeader,
  encodeWireFrameHeader,
} from "../src/index.js";

/** 与 Rust `wire::tests::SAMPLE_BYTES` 完全相同的跨语言字节向量 */
const SAMPLE_BYTES = Uint8Array.from([
  0x01, 0x04, 0x03, 0x00, 0x00, 0x01, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x10, 0x0e, 0x00, 0x00,
]);

const sample = () =>
  createWireFrameHeader(WireChannelId.StateStream, WireFrameFlag.CompressedLz4 | WireFrameFlag.Keyframe, 256, 42, 3600n);

describe("WireFrameHeader", () => {
  it("编码为参考字节", () => {
    expect(encodeWireFrameHeader(sample())).toEqual(SAMPLE_BYTES);
  });

  it("解码忽略尾随负载且支持子视图", () => {
    const frame = new Uint8Array(32);
    frame.set(SAMPLE_BYTES, 8);
    expect(decodeWireFrameHeader(frame.subarray(8))).toEqual(sample());
  });

  it("Tick 截断为低 32 位", () => {
    expect(createWireFrameHeader(WireChannelId.Event, 0, 0, 0, 0x1_0000_0005n).tickLow32).toBe(5);
  });

  it("拒绝非法帧头", () => {
    const mutate = (index: number, value: number): Uint8Array => {
      const copy = SAMPLE_BYTES.slice();
      copy[index] = value;
      return copy;
    };
    expect(() => decodeWireFrameHeader(SAMPLE_BYTES.subarray(0, 15))).toThrow(WireError);
    expect(() => decodeWireFrameHeader(mutate(0, 2))).toThrow(/版本/);
    expect(() => decodeWireFrameHeader(mutate(1, 6))).toThrow(/通道/);
    expect(() => decodeWireFrameHeader(mutate(2, 0x04))).toThrow(/未定义位/);
    expect(() => createWireFrameHeader(WireChannelId.Command, 0, MAX_WIRE_PAYLOAD_LEN + 1, 0, 0n)).toThrow(WireError);
  });
});
