// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

//! 实时仿真流的 16 字节帧头（D202「16B 网络帧生命周期」）。
//!
//! 每个 WebTransport / WebSocket 二进制消息以一个 `WireFrameHeader` 开头，后接 `payload_len` 字节负载。
//!
//! | 偏移 | 长度 | 字段 | 说明 |
//! | :--- | :--- | :--- | :--- |
//! | 0 | 1 | `version` | 帧格式版本，当前为 [`WIRE_FORMAT_VERSION`] |
//! | 1 | 1 | `channel` | [`WireChannelId`]（`0..=5`） |
//! | 2 | 2 | `flags` | [`WireFrameFlags`] 位集，未定义位必须为 0 |
//! | 4 | 4 | `payload_len` | 负载字节数，上限 [`MAX_WIRE_PAYLOAD_LEN`] |
//! | 8 | 4 | `sequence` | 该通道内单调递增的帧序号（回绕） |
//! | 12 | 4 | `tick_low32` | 帧对应权威 Tick 的低 32 位 |

use core::fmt;

/// 帧头固定长度（字节）
pub const WIRE_FRAME_HEADER_LEN: usize = 16;

/// 当前帧格式版本
pub const WIRE_FORMAT_VERSION: u8 = 1;

/// 单帧负载上限：16 MiB（更大的数据必须走 CAS Blob 下载通道）
pub const MAX_WIRE_PAYLOAD_LEN: u32 = 16 * 1024 * 1024;

/// 实时流逻辑通道（与 Command / Query / Event / Stream 四大 API 基元对应，D383–D394）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WireChannelId {
    /// 会话控制信令：握手、心跳、断线重连（Protobuf v3 负载）
    Control = 0,
    /// 客户端 → 服务端的命令提交
    Command = 1,
    /// 请求-响应式只读查询
    Query = 2,
    /// 已提交的权威事件流
    Event = 3,
    /// 分层状态 Delta 复制流（16B 对齐列式二进制 + LZ4）
    StateStream = 4,
    /// 诊断与遥测指标
    Telemetry = 5,
}

impl WireChannelId {
    /// 全部通道（按数值升序）
    pub const ALL: [Self; 6] = [
        Self::Control,
        Self::Command,
        Self::Query,
        Self::Event,
        Self::StateStream,
        Self::Telemetry,
    ];

    /// 由线上字节解析通道
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|channel| *channel as u8 == value)
    }
}

/// 帧标志位集
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WireFrameFlags(u16);

impl WireFrameFlags {
    /// 无标志
    pub const NONE: Self = Self(0);
    /// 负载经 LZ4 Frame 压缩
    pub const COMPRESSED_LZ4: Self = Self(1 << 0);
    /// 负载为完整关键帧（而非相对上一帧的 Delta）
    pub const KEYFRAME: Self = Self(1 << 1);
    /// 全部已定义位的掩码
    pub const KNOWN_MASK: u16 = Self::COMPRESSED_LZ4.0 | Self::KEYFRAME.0;

    /// 由原始位构造；包含未定义位时返回 `None`
    pub const fn from_bits(bits: u16) -> Option<Self> {
        if bits & !Self::KNOWN_MASK == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// 原始位
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// 是否包含 `other` 的全部位
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// 合并两个标志集
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// 帧头编解码错误
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// 输入不足 16 字节
    Truncated { len: usize },
    /// 不支持的帧格式版本
    UnsupportedVersion(u8),
    /// 未定义的通道号
    UnknownChannel(u8),
    /// 标志字段包含未定义位
    ReservedFlagBits(u16),
    /// 负载长度超过上限
    PayloadTooLarge(u32),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { len } => write!(f, "wire frame header needs 16 bytes, got {len}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported wire format version {v}"),
            Self::UnknownChannel(c) => write!(f, "unknown wire channel id {c}"),
            Self::ReservedFlagBits(bits) => write!(f, "reserved wire flag bits set: {bits:#06x}"),
            Self::PayloadTooLarge(len) => write!(f, "wire payload of {len} bytes exceeds {MAX_WIRE_PAYLOAD_LEN}"),
        }
    }
}

impl std::error::Error for WireError {}

/// 16 字节实时流帧头
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WireFrameHeader {
    pub version: u8,
    pub channel: WireChannelId,
    pub flags: WireFrameFlags,
    pub payload_len: u32,
    pub sequence: u32,
    pub tick_low32: u32,
}

impl WireFrameHeader {
    /// 以当前格式版本构造帧头，负载长度超限时报错
    pub fn new(
        channel: WireChannelId,
        flags: WireFrameFlags,
        payload_len: u32,
        sequence: u32,
        tick: u64,
    ) -> Result<Self, WireError> {
        if payload_len > MAX_WIRE_PAYLOAD_LEN {
            return Err(WireError::PayloadTooLarge(payload_len));
        }
        Ok(Self {
            version: WIRE_FORMAT_VERSION,
            channel,
            flags,
            payload_len,
            sequence,
            tick_low32: tick as u32,
        })
    }

    /// 编码为 16 字节（Little-Endian）
    pub fn encode(&self) -> [u8; WIRE_FRAME_HEADER_LEN] {
        let mut out = [0u8; WIRE_FRAME_HEADER_LEN];
        out[0] = self.version;
        out[1] = self.channel as u8;
        out[2..4].copy_from_slice(&self.flags.bits().to_le_bytes());
        out[4..8].copy_from_slice(&self.payload_len.to_le_bytes());
        out[8..12].copy_from_slice(&self.sequence.to_le_bytes());
        out[12..16].copy_from_slice(&self.tick_low32.to_le_bytes());
        out
    }

    /// 从字节切片开头解码并校验帧头（多余字节视为负载，不在此处理）
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let header: &[u8; WIRE_FRAME_HEADER_LEN] = bytes
            .get(..WIRE_FRAME_HEADER_LEN)
            .and_then(|head| head.try_into().ok())
            .ok_or(WireError::Truncated { len: bytes.len() })?;

        let version = header[0];
        if version != WIRE_FORMAT_VERSION {
            return Err(WireError::UnsupportedVersion(version));
        }
        let channel = WireChannelId::from_u8(header[1]).ok_or(WireError::UnknownChannel(header[1]))?;
        let flag_bits = u16::from_le_bytes([header[2], header[3]]);
        let flags = WireFrameFlags::from_bits(flag_bits).ok_or(WireError::ReservedFlagBits(flag_bits))?;
        let payload_len = read_u32_le(header, 4);
        if payload_len > MAX_WIRE_PAYLOAD_LEN {
            return Err(WireError::PayloadTooLarge(payload_len));
        }
        Ok(Self {
            version,
            channel,
            flags,
            payload_len,
            sequence: read_u32_le(header, 8),
            tick_low32: read_u32_le(header, 12),
        })
    }
}

/// 读取 `offset` 处的 Little-Endian u32（调用方保证不越界）
fn read_u32_le(bytes: &[u8; WIRE_FRAME_HEADER_LEN], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跨语言字节向量：TypeScript `@rynvek/protocol` 的测试使用完全相同的期望字节
    const SAMPLE_BYTES: [u8; 16] = [
        0x01, 0x04, 0x03, 0x00, 0x00, 0x01, 0x00, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x10, 0x0E, 0x00, 0x00,
    ];

    fn sample() -> WireFrameHeader {
        let flags = WireFrameFlags::COMPRESSED_LZ4.union(WireFrameFlags::KEYFRAME);
        WireFrameHeader::new(WireChannelId::StateStream, flags, 256, 42, 3600).unwrap()
    }

    #[test]
    fn encodes_to_reference_bytes() {
        assert_eq!(sample().encode(), SAMPLE_BYTES);
    }

    #[test]
    fn decode_roundtrip_ignores_trailing_payload() {
        let mut frame = SAMPLE_BYTES.to_vec();
        frame.extend_from_slice(&[0xAB; 8]);
        assert_eq!(WireFrameHeader::decode(&frame), Ok(sample()));
    }

    #[test]
    fn tick_is_truncated_to_low_32_bits() {
        let header = WireFrameHeader::new(WireChannelId::Event, WireFrameFlags::NONE, 0, 0, 0x1_0000_0005).unwrap();
        assert_eq!(header.tick_low32, 5);
    }

    #[test]
    fn decode_rejects_malformed_headers() {
        assert_eq!(
            WireFrameHeader::decode(&SAMPLE_BYTES[..15]),
            Err(WireError::Truncated { len: 15 })
        );

        let mut bad = SAMPLE_BYTES;
        bad[0] = 2;
        assert_eq!(WireFrameHeader::decode(&bad), Err(WireError::UnsupportedVersion(2)));

        let mut bad = SAMPLE_BYTES;
        bad[1] = 6;
        assert_eq!(WireFrameHeader::decode(&bad), Err(WireError::UnknownChannel(6)));

        let mut bad = SAMPLE_BYTES;
        bad[2] = 0x04;
        assert_eq!(WireFrameHeader::decode(&bad), Err(WireError::ReservedFlagBits(0x0004)));

        let mut bad = SAMPLE_BYTES;
        bad[4..8].copy_from_slice(&(MAX_WIRE_PAYLOAD_LEN + 1).to_le_bytes());
        assert_eq!(
            WireFrameHeader::decode(&bad),
            Err(WireError::PayloadTooLarge(MAX_WIRE_PAYLOAD_LEN + 1))
        );
    }

    #[test]
    fn oversized_payload_is_rejected_on_construction() {
        let result = WireFrameHeader::new(WireChannelId::Command, WireFrameFlags::NONE, u32::MAX, 0, 0);
        assert_eq!(result, Err(WireError::PayloadTooLarge(u32::MAX)));
    }

    #[test]
    fn channel_ids_are_dense() {
        for (i, channel) in WireChannelId::ALL.iter().enumerate() {
            assert_eq!(*channel as usize, i);
            assert_eq!(WireChannelId::from_u8(i as u8), Some(*channel));
        }
    }
}
