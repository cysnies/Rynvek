// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 实时仿真流的 16 字节帧头，与 Rust `rynvek_protocol::wire` 逐字节一致（Little-Endian）。
 *
 * | 偏移 | 长度 | 字段 |
 * | :--- | :--- | :--- |
 * | 0 | 1 | `version` |
 * | 1 | 1 | `channel` |
 * | 2 | 2 | `flags` |
 * | 4 | 4 | `payloadLen` |
 * | 8 | 4 | `sequence` |
 * | 12 | 4 | `tickLow32` |
 */

/** 帧头固定长度（字节） */
export const WIRE_FRAME_HEADER_LEN = 16;
/** 当前帧格式版本 */
export const WIRE_FORMAT_VERSION = 1;
/** 单帧负载上限：16 MiB */
export const MAX_WIRE_PAYLOAD_LEN = 16 * 1024 * 1024;

/** 实时流逻辑通道 */
export const WireChannelId = {
  /** 会话控制信令：握手、心跳、断线重连 */
  Control: 0,
  /** 客户端 → 服务端的命令提交 */
  Command: 1,
  /** 请求-响应式只读查询 */
  Query: 2,
  /** 已提交的权威事件流 */
  Event: 3,
  /** 分层状态 Delta 复制流 */
  StateStream: 4,
  /** 诊断与遥测指标 */
  Telemetry: 5,
} as const;

export type WireChannelId = (typeof WireChannelId)[keyof typeof WireChannelId];

const CHANNEL_IDS: ReadonlySet<number> = new Set(Object.values(WireChannelId));

/** 帧标志位 */
export const WireFrameFlag = {
  None: 0,
  /** 负载经 LZ4 Frame 压缩 */
  CompressedLz4: 1 << 0,
  /** 负载为完整关键帧 */
  Keyframe: 1 << 1,
} as const;

/** 全部已定义标志位的掩码 */
export const WIRE_FLAGS_KNOWN_MASK = WireFrameFlag.CompressedLz4 | WireFrameFlag.Keyframe;

/** 解码后的帧头 */
export interface WireFrameHeader {
  readonly version: number;
  readonly channel: WireChannelId;
  readonly flags: number;
  readonly payloadLen: number;
  readonly sequence: number;
  readonly tickLow32: number;
}

/** 帧头编解码错误 */
export class WireError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "WireError";
  }
}

/** 以当前格式版本构造帧头（`tick` 截断为低 32 位） */
export function createWireFrameHeader(
  channel: WireChannelId,
  flags: number,
  payloadLen: number,
  sequence: number,
  tick: bigint,
): WireFrameHeader {
  validateFlags(flags);
  validatePayloadLen(payloadLen);
  return {
    version: WIRE_FORMAT_VERSION,
    channel,
    flags,
    payloadLen,
    sequence: sequence >>> 0,
    tickLow32: Number(BigInt.asUintN(32, tick)),
  };
}

/** 编码为 16 字节 */
export function encodeWireFrameHeader(header: WireFrameHeader): Uint8Array {
  const bytes = new Uint8Array(WIRE_FRAME_HEADER_LEN);
  const view = new DataView(bytes.buffer);
  view.setUint8(0, header.version);
  view.setUint8(1, header.channel);
  view.setUint16(2, header.flags, true);
  view.setUint32(4, header.payloadLen, true);
  view.setUint32(8, header.sequence, true);
  view.setUint32(12, header.tickLow32, true);
  return bytes;
}

/** 从字节开头解码并校验帧头（多余字节视为负载） */
export function decodeWireFrameHeader(bytes: Uint8Array): WireFrameHeader {
  if (bytes.length < WIRE_FRAME_HEADER_LEN) {
    throw new WireError(`帧头需要 16 字节，实际 ${bytes.length}`);
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, WIRE_FRAME_HEADER_LEN);
  const version = view.getUint8(0);
  if (version !== WIRE_FORMAT_VERSION) {
    throw new WireError(`不支持的帧格式版本 ${version}`);
  }
  const channel = view.getUint8(1);
  if (!CHANNEL_IDS.has(channel)) {
    throw new WireError(`未定义的通道号 ${channel}`);
  }
  const flags = view.getUint16(2, true);
  validateFlags(flags);
  const payloadLen = view.getUint32(4, true);
  validatePayloadLen(payloadLen);
  return {
    version,
    channel: channel as WireChannelId,
    flags,
    payloadLen,
    sequence: view.getUint32(8, true),
    tickLow32: view.getUint32(12, true),
  };
}

function validateFlags(flags: number): void {
  if ((flags & ~WIRE_FLAGS_KNOWN_MASK) !== 0) {
    throw new WireError(`标志字段包含未定义位 0x${flags.toString(16)}`);
  }
}

function validatePayloadLen(payloadLen: number): void {
  if (!Number.isInteger(payloadLen) || payloadLen < 0 || payloadLen > MAX_WIRE_PAYLOAD_LEN) {
    throw new WireError(`负载长度 ${payloadLen} 超出上限 ${MAX_WIRE_PAYLOAD_LEN}`);
  }
}
