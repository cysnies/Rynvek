// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 16 字节帧同步（D202「16B 网络帧生命周期」）。
//!
//! 帧 = [`WireFrameHeader`]（`rynvek-protocol` 冻结的 16 字节布局）+ `payload_len` 字节负载。
//!
//! - **发送**：[`FrameEncoder`] 为每个 [`WireChannelId`] 维护独立的单调（回绕）序号；负载不小于
//!   [`LZ4_MIN_PAYLOAD`] 且压缩后更短时以 LZ4 Frame 压缩并置 `COMPRESSED_LZ4` 位（规范 1.1：实时网络流用 LZ4 Frame）。
//! - **消息型传输**（WebSocket 二进制消息）：一条消息恰好一帧，[`decode_message`] 要求长度完全一致。
//! - **字节流传输**（WebTransport 双向流）：帧首尾相接，[`FrameDecoder`] 增量重组，可在任意字节处切分。
//! - **接收校验**：[`SequenceTracker`] 要求每通道序号严格 `+1`（可靠有序流不应出现缺口或重放）。
//! - **解压上限**：解压后长度不得超过 [`MAX_WIRE_PAYLOAD_LEN`]，防止压缩炸弹。

use std::io::{Read, Write};

use rynvek_protocol::wire::{MAX_WIRE_PAYLOAD_LEN, WIRE_FRAME_HEADER_LEN};
use rynvek_protocol::{WireChannelId, WireFrameFlags, WireFrameHeader};

use crate::error::NetError;

/// 达到该长度的负载才尝试压缩
pub const LZ4_MIN_PAYLOAD: usize = 256;

/// 一个完整的线上帧
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub header: WireFrameHeader,
    /// 线上负载（若 `COMPRESSED_LZ4` 置位则为压缩后字节）
    pub payload: Vec<u8>,
}

impl Frame {
    pub const fn channel(&self) -> WireChannelId {
        self.header.channel
    }

    pub const fn is_keyframe(&self) -> bool {
        self.header.flags.contains(WireFrameFlags::KEYFRAME)
    }

    /// 编码为 `16 + payload_len` 字节
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(WIRE_FRAME_HEADER_LEN + self.payload.len());
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.payload);
        out
    }

    /// 取得应用层负载（必要时解压）
    pub fn decoded_payload(&self) -> Result<Vec<u8>, NetError> {
        if !self.header.flags.contains(WireFrameFlags::COMPRESSED_LZ4) {
            return Ok(self.payload.clone());
        }
        let mut out = Vec::new();
        lz4_flex::frame::FrameDecoder::new(self.payload.as_slice())
            .take(MAX_WIRE_PAYLOAD_LEN as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| NetError::Decompress(e.to_string()))?;
        if out.len() > MAX_WIRE_PAYLOAD_LEN as usize {
            return Err(NetError::Decompress(format!(
                "解压后超过 {MAX_WIRE_PAYLOAD_LEN} 字节上限"
            )));
        }
        Ok(out)
    }
}

/// 发送端：分配序号并按需压缩
#[derive(Clone, Debug, Default)]
pub struct FrameEncoder {
    next_sequence: [u32; WireChannelId::ALL.len()],
    compress: bool,
}

impl FrameEncoder {
    /// `compress = true` 时对较大负载启用 LZ4
    pub fn new(compress: bool) -> Self {
        Self {
            next_sequence: [0; WireChannelId::ALL.len()],
            compress,
        }
    }

    /// 构造下一帧
    pub fn encode(
        &mut self,
        channel: WireChannelId,
        tick: u64,
        payload: &[u8],
        keyframe: bool,
    ) -> Result<Frame, NetError> {
        let mut flags = if keyframe {
            WireFrameFlags::KEYFRAME
        } else {
            WireFrameFlags::NONE
        };
        let mut body = payload.to_vec();
        if self.compress && payload.len() >= LZ4_MIN_PAYLOAD {
            let compressed = lz4_compress(payload)?;
            if compressed.len() < payload.len() {
                body = compressed;
                flags = flags.union(WireFrameFlags::COMPRESSED_LZ4);
            }
        }
        let len = u32::try_from(body.len()).map_err(|_| NetError::Payload("负载超过 u32".into()))?;
        let sequence = self.next_sequence[channel as usize];
        let header = WireFrameHeader::new(channel, flags, len, sequence, tick)?;
        self.next_sequence[channel as usize] = sequence.wrapping_add(1);
        Ok(Frame { header, payload: body })
    }
}

fn lz4_compress(payload: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut enc = lz4_flex::frame::FrameEncoder::new(Vec::with_capacity(payload.len() / 2));
    enc.write_all(payload)
        .map_err(|e| NetError::Decompress(e.to_string()))?;
    enc.finish().map_err(|e| NetError::Decompress(e.to_string()))
}

/// 消息型传输：一条消息必须恰好是一帧
pub fn decode_message(bytes: &[u8]) -> Result<Frame, NetError> {
    let header = WireFrameHeader::decode(bytes)?;
    let actual = bytes.len() - WIRE_FRAME_HEADER_LEN;
    if actual != header.payload_len as usize {
        return Err(NetError::LengthMismatch {
            declared: header.payload_len,
            actual,
        });
    }
    Ok(Frame {
        header,
        payload: bytes[WIRE_FRAME_HEADER_LEN..].to_vec(),
    })
}

/// 字节流传输的增量帧重组器
#[derive(Clone, Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加收到的字节
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// 取出下一完整帧；数据不足返回 `Ok(None)`。帧头一旦可读即校验，非法时立即报错（不等待负载）。
    pub fn next_frame(&mut self) -> Result<Option<Frame>, NetError> {
        if self.buffer.len() < WIRE_FRAME_HEADER_LEN {
            return Ok(None);
        }
        let header = WireFrameHeader::decode(&self.buffer)?;
        let total = WIRE_FRAME_HEADER_LEN + header.payload_len as usize;
        if self.buffer.len() < total {
            return Ok(None);
        }
        let payload = self.buffer[WIRE_FRAME_HEADER_LEN..total].to_vec();
        self.buffer.drain(..total);
        Ok(Some(Frame { header, payload }))
    }

    /// 缓冲中尚未成帧的字节数
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

/// 接收端序号校验：每个通道从 0 开始、严格 `+1`（回绕）
#[derive(Clone, Debug, Default)]
pub struct SequenceTracker {
    expected: [u32; WireChannelId::ALL.len()],
}

impl SequenceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accept(&mut self, header: &WireFrameHeader) -> Result<(), NetError> {
        let c = header.channel as usize;
        if header.sequence != self.expected[c] {
            return Err(NetError::Sequence {
                channel: header.channel as u8,
                expected: self.expected[c],
                got: header.sequence,
            });
        }
        self.expected[c] = self.expected[c].wrapping_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_payloads_are_never_compressed() {
        let mut enc = FrameEncoder::new(true);
        let f = enc.encode(WireChannelId::Command, 5, b"{\"x\":1}", false).unwrap();
        assert_eq!(f.header.flags, WireFrameFlags::NONE);
        assert_eq!(f.to_bytes().len(), 16 + 7);
        assert_eq!(f.decoded_payload().unwrap(), b"{\"x\":1}");
    }

    #[test]
    fn large_compressible_payloads_use_lz4_frame() {
        let mut enc = FrameEncoder::new(true);
        let payload = vec![7u8; 10_000];
        let f = enc.encode(WireChannelId::StateStream, 9, &payload, true).unwrap();
        assert!(f.header.flags.contains(WireFrameFlags::COMPRESSED_LZ4));
        assert!(f.is_keyframe());
        assert!(f.payload.len() < 200);
        // LZ4 Frame 魔数 0x184D2204（LE）
        assert_eq!(&f.payload[..4], &[0x04, 0x22, 0x4D, 0x18]);
        assert_eq!(f.decoded_payload().unwrap(), payload);
    }

    #[test]
    fn incompressible_payload_stays_raw() {
        let mut enc = FrameEncoder::new(true);
        // BLAKE3 XOF 输出近似均匀随机，LZ4 无法压缩
        let mut payload = vec![0u8; 1024];
        blake3::Hasher::new().finalize_xof().fill(&mut payload);
        let f = enc.encode(WireChannelId::Event, 0, &payload, false).unwrap();
        assert!(!f.header.flags.contains(WireFrameFlags::COMPRESSED_LZ4));
    }

    #[test]
    fn sequences_are_per_channel() {
        let mut enc = FrameEncoder::new(false);
        let a = enc.encode(WireChannelId::Event, 0, b"a", false).unwrap();
        let b = enc.encode(WireChannelId::Event, 0, b"b", false).unwrap();
        let c = enc.encode(WireChannelId::Control, 0, b"c", false).unwrap();
        assert_eq!((a.header.sequence, b.header.sequence, c.header.sequence), (0, 1, 0));
        let mut t = SequenceTracker::new();
        t.accept(&a.header).unwrap();
        t.accept(&c.header).unwrap();
        assert!(matches!(
            t.accept(&a.header),
            Err(NetError::Sequence {
                expected: 1,
                got: 0,
                ..
            })
        ));
        t.accept(&b.header).unwrap();
    }

    #[test]
    fn message_decoding_requires_exact_length() {
        let mut enc = FrameEncoder::new(false);
        let bytes = enc.encode(WireChannelId::Query, 3, b"hello", false).unwrap().to_bytes();
        assert_eq!(decode_message(&bytes).unwrap().payload, b"hello");
        assert!(matches!(
            decode_message(&bytes[..20]),
            Err(NetError::LengthMismatch { declared: 5, actual: 4 })
        ));
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode_message(&longer).is_err());
        assert!(matches!(decode_message(&bytes[..10]), Err(NetError::Header(_))));
    }

    #[test]
    fn stream_reassembly_at_every_split_point() {
        let mut enc = FrameEncoder::new(true);
        let frames: Vec<Frame> = vec![
            enc.encode(WireChannelId::Control, 1, b"hello", false).unwrap(),
            enc.encode(WireChannelId::StateStream, 2, &vec![1u8; 4096], true)
                .unwrap(),
            enc.encode(WireChannelId::Event, 3, b"", false).unwrap(),
        ];
        let stream: Vec<u8> = frames.iter().flat_map(Frame::to_bytes).collect();
        for split in 0..=stream.len() {
            let mut dec = FrameDecoder::new();
            let mut out = Vec::new();
            for part in [&stream[..split], &stream[split..]] {
                dec.push(part);
                while let Some(f) = dec.next_frame().unwrap() {
                    out.push(f);
                }
            }
            assert_eq!(out, frames, "在第 {split} 字节切分");
            assert_eq!(dec.buffered(), 0);
        }
    }

    #[test]
    fn stream_decoder_rejects_bad_header_before_payload_arrives() {
        let mut dec = FrameDecoder::new();
        let mut bad = [0u8; 16];
        bad[0] = 9;
        dec.push(&bad);
        assert!(dec.next_frame().is_err());
    }

    #[test]
    fn compression_bomb_is_rejected() {
        // 17 MiB 的零压缩后很小，但解压会超过 16 MiB 上限
        let huge = vec![0u8; MAX_WIRE_PAYLOAD_LEN as usize + 1024];
        let compressed = lz4_compress(&huge).unwrap();
        let header = WireFrameHeader::new(
            WireChannelId::StateStream,
            WireFrameFlags::COMPRESSED_LZ4,
            compressed.len() as u32,
            0,
            0,
        )
        .unwrap();
        let f = Frame {
            header,
            payload: compressed,
        };
        assert!(matches!(f.decoded_payload(), Err(NetError::Decompress(_))));
    }
}
