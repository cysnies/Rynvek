// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 变长帧：`FrameHeader(24B) + payload + blake3(24B头‖payload)`。
//!
//! 若文件在 payload 或 checksum 中途截断，读取器返回
//! [`RynvekErrorCode::JournalTornWriteTruncated`] 并停止重放（已读完的完整帧仍有效）。

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

use crate::manifest::CommitManifest;

/// 帧头长度
pub const FRAME_HEADER_LEN: usize = 24;
/// 帧尾校验长度
pub const FRAME_CHECKSUM_LEN: usize = 32;

/// 帧类型
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// 载荷为完整 [`CommitManifest`]
    Commit = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalFrameHeader {
    pub kind: FrameKind,
    pub flags: u16,
    pub payload_len: u32,
    pub tick: u64,
    pub revision: u64,
}

impl JournalFrameHeader {
    pub fn encode(&self) -> [u8; FRAME_HEADER_LEN] {
        let mut out = [0u8; FRAME_HEADER_LEN];
        out[0..2].copy_from_slice(&(self.kind as u16).to_le_bytes());
        out[2..4].copy_from_slice(&self.flags.to_le_bytes());
        out[4..8].copy_from_slice(&self.payload_len.to_le_bytes());
        out[8..16].copy_from_slice(&self.tick.to_le_bytes());
        out[16..24].copy_from_slice(&self.revision.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() < FRAME_HEADER_LEN {
            return Err(RynvekErrorCode::JournalTornWriteTruncated);
        }
        let kind_raw = u16::from_le_bytes([bytes[0], bytes[1]]);
        let kind = match kind_raw {
            1 => FrameKind::Commit,
            _ => return Err(RynvekErrorCode::InvalidFileMagicOrVersion),
        };
        Ok(Self {
            kind,
            flags: u16::from_le_bytes([bytes[2], bytes[3]]),
            payload_len: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            tick: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            revision: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        })
    }
}

/// 完整 journal 帧
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalFrame {
    pub header: JournalFrameHeader,
    pub payload: Vec<u8>,
    pub checksum: [u8; 32],
}

impl JournalFrame {
    /// 由提交清单构造帧
    pub fn from_commit(manifest: &CommitManifest) -> Self {
        let payload = manifest.encode().to_vec();
        let header = JournalFrameHeader {
            kind: FrameKind::Commit,
            flags: 0,
            payload_len: payload.len() as u32,
            tick: manifest.tick,
            revision: manifest.world_revision,
        };
        let checksum = checksum_of(&header, &payload);
        Self {
            header,
            payload,
            checksum,
        }
    }

    /// 解析为提交清单
    pub fn to_commit(&self) -> Result<CommitManifest, RynvekErrorCode> {
        if self.header.kind != FrameKind::Commit {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        CommitManifest::decode(&self.payload)
    }

    /// 编码为字节（头 + 载荷 + 校验）
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + self.payload.len() + FRAME_CHECKSUM_LEN);
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&self.checksum);
        out
    }

    /// 校验 checksum 是否匹配
    pub fn verify_checksum(&self) -> Result<(), RynvekErrorCode> {
        let expected = checksum_of(&self.header, &self.payload);
        if expected != self.checksum {
            Err(RynvekErrorCode::Blake3ChecksumMismatch)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn checksum_of(header: &JournalFrameHeader, payload: &[u8]) -> [u8; 32] {
    let head = header.encode();
    Blake3Hash::of_concatenated(&[&head, payload]).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::CommitManifest;

    #[test]
    fn frame_roundtrip() {
        let m = CommitManifest::new(
            [1; 16],
            3,
            1,
            [0; 32],
            crate::manifest::CommitSubRoots {
                entity_root_hash: [9; 32],
                ..Default::default()
            },
        );
        let frame = JournalFrame::from_commit(&m);
        frame.verify_checksum().unwrap();
        assert_eq!(frame.to_commit().unwrap(), m);
        let bytes = frame.encode();
        let header = JournalFrameHeader::decode(&bytes).unwrap();
        assert_eq!(header.tick, 3);
    }
}
