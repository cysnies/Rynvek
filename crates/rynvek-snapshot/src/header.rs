// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rsnap` 64 字节文件头（规范 10.1，魔数 `RYSS`，D196）。
//!
//! | 偏移 | 长度 | 字段 |
//! | :--- | :--- | :--- |
//! | 0 | 4 | `magic` = `RYSS` |
//! | 4 | 2 | `format_version` |
//! | 6 | 2 | `flags`（bit0 = payload 经 Zstd） |
//! | 8 | 2 | `header_bytes` = 64 |
//! | 10 | 2 | `chunk_count` |
//! | 12 | 4 | `reserved0` |
//! | 16 | 16 | `world_id` |
//! | 32 | 8 | `tick` |
//! | 40 | 8 | `world_revision` |
//! | 48 | 8 | `payload_bytes`（压缩后载荷长度） |
//! | 56 | 8 | `reserved1` |
//!
//! 文件头之后紧跟 32 字节 `state_root_hash`，再跟压缩载荷。

use rynvek_core::error::RynvekErrorCode;

pub const SNAPSHOT_HEADER_LEN: usize = 64;
pub const SNAPSHOT_MAGIC: [u8; 4] = *b"RYSS";
pub const SNAPSHOT_VERSION: u16 = 1;
/// 规范十域世界快照；必须携带 `CMIT` 清单。
pub const CANONICAL_SNAPSHOT_VERSION: u16 = 2;
/// flags bit0：载荷为 Zstd 压缩
pub const SNAPSHOT_FLAG_ZSTD: u16 = 1 << 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotFileHeader {
    pub format_version: u16,
    pub flags: u16,
    pub chunk_count: u16,
    pub world_id: [u8; 16],
    pub tick: u64,
    pub world_revision: u64,
    pub payload_bytes: u64,
}

impl SnapshotFileHeader {
    pub fn new(
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        chunk_count: u16,
        payload_bytes: u64,
        zstd: bool,
    ) -> Self {
        Self {
            format_version: SNAPSHOT_VERSION,
            flags: if zstd { SNAPSHOT_FLAG_ZSTD } else { 0 },
            chunk_count,
            world_id,
            tick,
            world_revision,
            payload_bytes,
        }
    }

    pub fn encode(&self) -> [u8; SNAPSHOT_HEADER_LEN] {
        let mut out = [0u8; SNAPSHOT_HEADER_LEN];
        out[0..4].copy_from_slice(&SNAPSHOT_MAGIC);
        out[4..6].copy_from_slice(&self.format_version.to_le_bytes());
        out[6..8].copy_from_slice(&self.flags.to_le_bytes());
        out[8..10].copy_from_slice(&(SNAPSHOT_HEADER_LEN as u16).to_le_bytes());
        out[10..12].copy_from_slice(&self.chunk_count.to_le_bytes());
        out[16..32].copy_from_slice(&self.world_id);
        out[32..40].copy_from_slice(&self.tick.to_le_bytes());
        out[40..48].copy_from_slice(&self.world_revision.to_le_bytes());
        out[48..56].copy_from_slice(&self.payload_bytes.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() < SNAPSHOT_HEADER_LEN {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        if bytes[0..4] != SNAPSHOT_MAGIC {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let format_version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if ![SNAPSHOT_VERSION, CANONICAL_SNAPSHOT_VERSION].contains(&format_version) {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        Ok(Self {
            format_version,
            flags: u16::from_le_bytes([bytes[6], bytes[7]]),
            chunk_count: u16::from_le_bytes([bytes[10], bytes[11]]),
            world_id: bytes[16..32].try_into().unwrap(),
            tick: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            world_revision: u64::from_le_bytes(bytes[40..48].try_into().unwrap()),
            payload_bytes: u64::from_le_bytes(bytes[48..56].try_into().unwrap()),
        })
    }

    pub const fn is_zstd(self) -> bool {
        self.flags & SNAPSHOT_FLAG_ZSTD != 0
    }
}
