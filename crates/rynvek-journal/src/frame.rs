// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 变长帧：`FrameHeader(24B) + payload + blake3(24B头‖payload)`（规范 10.1 / 审计报告 §4.2）。
//!
//! 若文件在 payload 或 checksum 中途截断，读取器返回
//! [`RynvekErrorCode::JournalTornWriteTruncated`] 并停止重放（已读完的完整帧仍有效）。

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use serde::{Deserialize, Serialize};

use crate::manifest::CommitManifest;

/// 帧头长度（24B：2B kind + 2B flags + 4B payload_len + 8B tick + 8B revision）
pub const FRAME_HEADER_LEN: usize = 24;
/// 帧尾校验长度（32B BLAKE3）
pub const FRAME_CHECKSUM_LEN: usize = 32;
/// 单帧最大允许载荷长度（64 MiB）
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// 预写日志（WAL）帧类型（审计报告 §4.2，涵盖全部 7 种帧）
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    /// 载荷为完整 [`CommitManifest`]（Tick 提交清单）
    Commit = 1,
    /// 已被权威层接受并排入当前 Tick 的外部命令帧
    CommandAccepted = 2,
    /// 已调度的确定性事件帧
    ScheduledEvent = 3,
    /// 结构性实体变更（Spawn / Despawn / Archetype 迁移）帧
    StructuralMutation = 4,
    /// Studio / 沙盒世界补丁（WorldPatch）帧
    WorldPatch = 5,
    /// 快照锚点引用帧（记录最新落盘 `.rsnap` 的 Tick、Revision 与状态根）
    SnapshotReference = 6,
    /// 周期性状态检查点帧
    Checkpoint = 7,
}

impl FrameKind {
    /// 与 `Commit` 等价的语义别名
    pub const TickCommit: Self = Self::Commit;

    pub const fn from_u16(v: u16) -> Option<Self> {
        match v {
            1 => Some(Self::Commit),
            2 => Some(Self::CommandAccepted),
            3 => Some(Self::ScheduledEvent),
            4 => Some(Self::StructuralMutation),
            5 => Some(Self::WorldPatch),
            6 => Some(Self::SnapshotReference),
            7 => Some(Self::Checkpoint),
            _ => None,
        }
    }
}

/// 命令接受日志条目（`FrameKind::CommandAccepted`）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalCommandRecord {
    pub tick: u64,
    pub world_revision: u64,
    pub faction_id: u8,
    pub command_bytes: Vec<u8>,
}

/// 调度事件日志条目（`FrameKind::ScheduledEvent`）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEventRecord {
    pub tick: u64,
    pub world_revision: u64,
    pub event_kind: u16,
    pub event_bytes: Vec<u8>,
}

/// 结构变更日志条目（`FrameKind::StructuralMutation`）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalMutationRecord {
    pub tick: u64,
    pub world_revision: u64,
    pub op_kind: u8,
    pub entity_serial: u32,
    pub mutation_bytes: Vec<u8>,
}

/// 世界补丁日志条目（`FrameKind::WorldPatch`）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalPatchRecord {
    pub tick: u64,
    pub world_revision: u64,
    pub patch_bytes: Vec<u8>,
}

/// 快照锚点引用日志条目（`FrameKind::SnapshotReference`）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalSnapshotRefRecord {
    pub snapshot_tick: u64,
    pub snapshot_revision: u64,
    pub snapshot_state_root: [u8; 32],
}

/// 类型化 WAL 记录枚举
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalRecord {
    TickCommit(CommitManifest),
    CommandAccepted(JournalCommandRecord),
    ScheduledEvent(JournalEventRecord),
    StructuralMutation(JournalMutationRecord),
    WorldPatch(JournalPatchRecord),
    SnapshotReference(JournalSnapshotRefRecord),
    Checkpoint(CommitManifest),
}

impl JournalRecord {
    pub const fn kind(&self) -> FrameKind {
        match self {
            Self::TickCommit(_) => FrameKind::Commit,
            Self::CommandAccepted(_) => FrameKind::CommandAccepted,
            Self::ScheduledEvent(_) => FrameKind::ScheduledEvent,
            Self::StructuralMutation(_) => FrameKind::StructuralMutation,
            Self::WorldPatch(_) => FrameKind::WorldPatch,
            Self::SnapshotReference(_) => FrameKind::SnapshotReference,
            Self::Checkpoint(_) => FrameKind::Checkpoint,
        }
    }

    pub const fn tick(&self) -> u64 {
        match self {
            Self::TickCommit(m) | Self::Checkpoint(m) => m.tick,
            Self::CommandAccepted(r) => r.tick,
            Self::ScheduledEvent(r) => r.tick,
            Self::StructuralMutation(r) => r.tick,
            Self::WorldPatch(r) => r.tick,
            Self::SnapshotReference(r) => r.snapshot_tick,
        }
    }

    pub const fn world_revision(&self) -> u64 {
        match self {
            Self::TickCommit(m) | Self::Checkpoint(m) => m.world_revision,
            Self::CommandAccepted(r) => r.world_revision,
            Self::ScheduledEvent(r) => r.world_revision,
            Self::StructuralMutation(r) => r.world_revision,
            Self::WorldPatch(r) => r.world_revision,
            Self::SnapshotReference(r) => r.snapshot_revision,
        }
    }
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
        let kind = FrameKind::from_u16(kind_raw).ok_or(RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let payload_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if payload_len as usize > MAX_FRAME_PAYLOAD_BYTES {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        Ok(Self {
            kind,
            flags: u16::from_le_bytes([bytes[2], bytes[3]]),
            payload_len,
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
        Self::from_parts(FrameKind::Commit, manifest.tick, manifest.world_revision, payload)
    }

    pub fn from_parts(kind: FrameKind, tick: u64, revision: u64, payload: Vec<u8>) -> Self {
        let header = JournalFrameHeader {
            kind,
            flags: 0,
            payload_len: payload.len() as u32,
            tick,
            revision,
        };
        let checksum = checksum_of(&header, &payload);
        Self {
            header,
            payload,
            checksum,
        }
    }

    pub fn from_record(record: &JournalRecord) -> Self {
        let payload = match record {
            JournalRecord::TickCommit(m) | JournalRecord::Checkpoint(m) => m.encode().to_vec(),
            JournalRecord::CommandAccepted(r) => {
                let mut b = Vec::with_capacity(1 + 4 + r.command_bytes.len());
                b.push(r.faction_id);
                b.extend_from_slice(&(r.command_bytes.len() as u32).to_le_bytes());
                b.extend_from_slice(&r.command_bytes);
                b
            }
            JournalRecord::ScheduledEvent(r) => {
                let mut b = Vec::with_capacity(2 + 4 + r.event_bytes.len());
                b.extend_from_slice(&r.event_kind.to_le_bytes());
                b.extend_from_slice(&(r.event_bytes.len() as u32).to_le_bytes());
                b.extend_from_slice(&r.event_bytes);
                b
            }
            JournalRecord::StructuralMutation(r) => {
                let mut b = Vec::with_capacity(1 + 4 + 4 + r.mutation_bytes.len());
                b.push(r.op_kind);
                b.extend_from_slice(&r.entity_serial.to_le_bytes());
                b.extend_from_slice(&(r.mutation_bytes.len() as u32).to_le_bytes());
                b.extend_from_slice(&r.mutation_bytes);
                b
            }
            JournalRecord::WorldPatch(r) => {
                let mut b = Vec::with_capacity(4 + r.patch_bytes.len());
                b.extend_from_slice(&(r.patch_bytes.len() as u32).to_le_bytes());
                b.extend_from_slice(&r.patch_bytes);
                b
            }
            JournalRecord::SnapshotReference(r) => {
                let mut b = Vec::with_capacity(48);
                b.extend_from_slice(&r.snapshot_tick.to_le_bytes());
                b.extend_from_slice(&r.snapshot_revision.to_le_bytes());
                b.extend_from_slice(&r.snapshot_state_root);
                b
            }
        };
        Self::from_parts(record.kind(), record.tick(), record.world_revision(), payload)
    }

    /// 解析为提交清单
    pub fn to_commit(&self) -> Result<CommitManifest, RynvekErrorCode> {
        if !matches!(self.header.kind, FrameKind::Commit | FrameKind::Checkpoint) {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        CommitManifest::decode(&self.payload)
    }

    /// 解析为类型化 WAL 记录
    pub fn to_record(&self) -> Result<JournalRecord, RynvekErrorCode> {
        let p = &self.payload;
        match self.header.kind {
            FrameKind::Commit => Ok(JournalRecord::TickCommit(CommitManifest::decode(p)?)),
            FrameKind::Checkpoint => Ok(JournalRecord::Checkpoint(CommitManifest::decode(p)?)),
            FrameKind::CommandAccepted => {
                if p.len() < 5 {
                    return Err(RynvekErrorCode::JournalTornWriteTruncated);
                }
                let faction_id = p[0];
                let len = u32::from_le_bytes(p[1..5].try_into().unwrap()) as usize;
                if p.len() != 5 + len {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                Ok(JournalRecord::CommandAccepted(JournalCommandRecord {
                    tick: self.header.tick,
                    world_revision: self.header.revision,
                    faction_id,
                    command_bytes: p[5..].to_vec(),
                }))
            }
            FrameKind::ScheduledEvent => {
                if p.len() < 6 {
                    return Err(RynvekErrorCode::JournalTornWriteTruncated);
                }
                let event_kind = u16::from_le_bytes([p[0], p[1]]);
                let len = u32::from_le_bytes(p[2..6].try_into().unwrap()) as usize;
                if p.len() != 6 + len {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                Ok(JournalRecord::ScheduledEvent(JournalEventRecord {
                    tick: self.header.tick,
                    world_revision: self.header.revision,
                    event_kind,
                    event_bytes: p[6..].to_vec(),
                }))
            }
            FrameKind::StructuralMutation => {
                if p.len() < 9 {
                    return Err(RynvekErrorCode::JournalTornWriteTruncated);
                }
                let op_kind = p[0];
                let entity_serial = u32::from_le_bytes(p[1..5].try_into().unwrap());
                let len = u32::from_le_bytes(p[5..9].try_into().unwrap()) as usize;
                if p.len() != 9 + len {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                Ok(JournalRecord::StructuralMutation(JournalMutationRecord {
                    tick: self.header.tick,
                    world_revision: self.header.revision,
                    op_kind,
                    entity_serial,
                    mutation_bytes: p[9..].to_vec(),
                }))
            }
            FrameKind::WorldPatch => {
                if p.len() < 4 {
                    return Err(RynvekErrorCode::JournalTornWriteTruncated);
                }
                let len = u32::from_le_bytes(p[0..4].try_into().unwrap()) as usize;
                if p.len() != 4 + len {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                Ok(JournalRecord::WorldPatch(JournalPatchRecord {
                    tick: self.header.tick,
                    world_revision: self.header.revision,
                    patch_bytes: p[4..].to_vec(),
                }))
            }
            FrameKind::SnapshotReference => {
                if p.len() != 48 {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                let snapshot_tick = u64::from_le_bytes(p[0..8].try_into().unwrap());
                let snapshot_revision = u64::from_le_bytes(p[8..16].try_into().unwrap());
                let snapshot_state_root: [u8; 32] = p[16..48].try_into().unwrap();
                Ok(JournalRecord::SnapshotReference(JournalSnapshotRefRecord {
                    snapshot_tick,
                    snapshot_revision,
                    snapshot_state_root,
                }))
            }
        }
    }

    /// 编码为字节（头 + 载荷 + 校验）
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + self.payload.len() + FRAME_CHECKSUM_LEN);
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&self.checksum);
        out
    }

    /// 与 `encode` 等价的辅助方法
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode()
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
