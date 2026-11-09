// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rjournal` 64 字节文件头（规范 10.1，魔数 `RYJL`）。
//!
//! | 偏移 | 长度 | 字段 |
//! | :--- | :--- | :--- |
//! | 0 | 4 | `magic` = `RYJL` |
//! | 4 | 2 | `format_version` |
//! | 6 | 2 | `flags` |
//! | 8 | 2 | `header_bytes` = 64 |
//! | 10 | 2 | `reserved0` |
//! | 12 | 4 | `frame_count`（写入完成时回填；崩溃时可能偏大，读取以校验为准） |
//! | 16 | 16 | `world_id` |
//! | 32 | 8 | `base_tick`（本段覆盖的起始 Tick） |
//! | 40 | 8 | `base_revision` |
//! | 48 | 8 | `snapshot_tick`（锚定快照 Tick；无则 0） |
//! | 56 | 8 | `reserved1` |

use rynvek_core::error::RynvekErrorCode;

/// 文件头长度
pub const JOURNAL_HEADER_LEN: usize = 64;
/// 魔数 `RYJL`
pub const JOURNAL_MAGIC: [u8; 4] = *b"RYJL";
/// 当前格式版本
pub const JOURNAL_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalFileHeader {
    pub format_version: u16,
    pub flags: u16,
    pub frame_count: u32,
    pub world_id: [u8; 16],
    pub base_tick: u64,
    pub base_revision: u64,
    pub snapshot_tick: u64,
}

impl JournalFileHeader {
    pub fn new(world_id: [u8; 16], base_tick: u64, base_revision: u64, snapshot_tick: u64) -> Self {
        Self {
            format_version: JOURNAL_VERSION,
            flags: 0,
            frame_count: 0,
            world_id,
            base_tick,
            base_revision,
            snapshot_tick,
        }
    }

    pub fn encode(&self) -> [u8; JOURNAL_HEADER_LEN] {
        let mut out = [0u8; JOURNAL_HEADER_LEN];
        out[0..4].copy_from_slice(&JOURNAL_MAGIC);
        out[4..6].copy_from_slice(&self.format_version.to_le_bytes());
        out[6..8].copy_from_slice(&self.flags.to_le_bytes());
        out[8..10].copy_from_slice(&(JOURNAL_HEADER_LEN as u16).to_le_bytes());
        out[12..16].copy_from_slice(&self.frame_count.to_le_bytes());
        out[16..32].copy_from_slice(&self.world_id);
        out[32..40].copy_from_slice(&self.base_tick.to_le_bytes());
        out[40..48].copy_from_slice(&self.base_revision.to_le_bytes());
        out[48..56].copy_from_slice(&self.snapshot_tick.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() < JOURNAL_HEADER_LEN {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        if bytes[0..4] != JOURNAL_MAGIC {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let format_version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if format_version != JOURNAL_VERSION {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let header_bytes = u16::from_le_bytes([bytes[8], bytes[9]]);
        if header_bytes as usize != JOURNAL_HEADER_LEN {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let mut world_id = [0u8; 16];
        world_id.copy_from_slice(&bytes[16..32]);
        Ok(Self {
            format_version,
            flags: u16::from_le_bytes([bytes[6], bytes[7]]),
            frame_count: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            world_id,
            base_tick: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            base_revision: u64::from_le_bytes(bytes[40..48].try_into().unwrap()),
            snapshot_tick: u64::from_le_bytes(bytes[48..56].try_into().unwrap()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = JournalFileHeader::new([9; 16], 10, 2, 0);
        assert_eq!(JournalFileHeader::decode(&h.encode()).unwrap(), h);
        let mut bad = h.encode();
        bad[0] = b'X';
        assert!(JournalFileHeader::decode(&bad).is_err());
    }
}
