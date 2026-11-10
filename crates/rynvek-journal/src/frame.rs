// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 帧布局（规范 10.1 / 审计报告 §4.2）：
//!
//! | 偏移 | 长度 | 字段 |
//! | :--- | :--- | :--- |
//! | 0 | 4 | `frame_magic` = `RYFR` |
//! | 4 | 2 | `frame_kind`（1..=7 对应完整 WAL 帧种类） |
//! | 6 | 2 | `reserved` |
//! | 8 | 8 | `tick` |
//! | 16 | 8 | `world_revision` |
//! | 24 | 8 | `payload_len` |
//! | 32 | N | `payload`（帧体） |
//! | 32+N | 32 | `frame_blake3`（对前 32+N 字节的 BLAKE3） |

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use serde::{Deserialize, Serialize};

use crate::manifest::CommitManifest;

pub const FRAME_MAGIC: [u8; 4] = *b"RYFR";
pub const FRAME_HEADER_LEN: usize = 32;
pub const FRAME_CHECKSUM_LEN: usize = 32;
/// 单帧最大允许载荷长度（64 MiB，防止畸形长度字段触发巨额内存分配）
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// 预写日志（WAL）帧种类（审计报告 §4.2）
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    /// Tick 提交清单帧（含全套子根与最终状态根）
    TickCommit = 1,
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
    pub const fn from_u16(v: u16) -> Option<Self> {
        match v {
            1 => Some(Self::TickCommit),
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
    /// 确定性小端二进制或规范化 JSON 命令字节
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
    /// 1 = Spawn, 2 = Despawn, 3 = ComponentPatch
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
            Self::TickCommit(_) => FrameKind::TickCommit,
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
    pub tick: u64,
    pub world_revision: u64,
    pub payload_len: u64,
}

impl JournalFrameHeader {
    pub fn encode(&self) -> [u8; FRAME_HEADER_LEN] {
        let mut out = [0u8; FRAME_HEADER_LEN];
        out[0..4].copy_from_slice(&FRAME_MAGIC);
        out[4..6].copy_from_slice(&(self.kind as u16).to_le_bytes());
        out[8..16].copy_from_slice(&self.tick.to_le_bytes());
        out[16..24].copy_from_slice(&self.world_revision.to_le_bytes());
        out[24..32].copy_from_slice(&self.payload_len.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() < FRAME_HEADER_LEN {
            return Err(RynvekErrorCode::JournalTornWriteTruncated);
        }
        if bytes[0..4] != FRAME_MAGIC {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let kind_raw = u16::from_le_bytes([bytes[4], bytes[5]]);
        let kind = FrameKind::from_u16(kind_raw).ok_or(RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let payload_len = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        if payload_len as usize > MAX_FRAME_PAYLOAD_BYTES {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        Ok(Self {
            kind,
            tick: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            world_revision: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            payload_len,
        })
    }
}

/// 一条完整的 Journal 帧
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalFrame {
    pub header: JournalFrameHeader,
    pub payload: Vec<u8>,
    pub checksum: [u8; 32],
}

impl JournalFrame {
    pub fn from_commit(commit: &CommitManifest) -> Self {
        let payload = commit.encode().to_vec();
        Self::from_parts(FrameKind::TickCommit, commit.tick, commit.world_revision, payload)
    }

    pub fn from_parts(kind: FrameKind, tick: u64, world_revision: u64, payload: Vec<u8>) -> Self {
        let header = JournalFrameHeader {
            kind,
            tick,
            world_revision,
            payload_len: payload.len() as u64,
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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + self.payload.len() + FRAME_CHECKSUM_LEN);
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&self.checksum);
        out
    }

    pub fn to_commit(&self) -> Result<CommitManifest, RynvekErrorCode> {
        if !matches!(self.header.kind, FrameKind::TickCommit | FrameKind::Checkpoint) {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        CommitManifest::decode(&self.payload)
    }

    pub fn to_record(&self) -> Result<JournalRecord, RynvekErrorCode> {
        let p = &self.payload;
        match self.header.kind {
            FrameKind::TickCommit => Ok(JournalRecord::TickCommit(CommitManifest::decode(p)?)),
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
                    world_revision: self.header.world_revision,
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
                    world_revision: self.header.world_revision,
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
                    world_revision: self.header.world_revision,
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
                    world_revision: self.header.world_revision,
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

    pub fn verify_checksum(&self) -> Result<(), RynvekErrorCode> {
        let expected = checksum_of(&self.header, &self.payload);
        if expected != self.checksum {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok(())
    }
}

pub(crate) fn checksum_of(header: &JournalFrameHeader, payload: &[u8]) -> [u8; 32] {
    let hdr = header.encode();
    Blake3Hash::of_concatenated(&[&hdr, payload]).0
}
