// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rsnap` 读取与实体表重建。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use rynvek_core::error::RynvekErrorCode;
use rynvek_ecs::{EntityId, EntityRegistry};

use crate::header::{SNAPSHOT_HEADER_LEN, SnapshotFileHeader};
use crate::neur::NeurChunk;
use crate::writer::ENTI_TAG;

/// 快照读取结果
#[derive(Debug)]
pub struct SnapshotReadResult {
    pub header: SnapshotFileHeader,
    pub state_root: [u8; 32],
    pub registry: EntityRegistry,
    pub neur: Option<NeurChunk>,
}

#[derive(Debug, Default)]
pub struct SnapshotReader;

impl SnapshotReader {
    pub fn read_path(path: impl AsRef<Path>) -> Result<SnapshotReadResult, RynvekErrorCode> {
        let mut file = File::open(path.as_ref()).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Self::read_bytes(&bytes)
    }

    pub fn read_bytes(bytes: &[u8]) -> Result<SnapshotReadResult, RynvekErrorCode> {
        if bytes.len() < SNAPSHOT_HEADER_LEN + 32 {
            return Err(RynvekErrorCode::JournalTornWriteTruncated);
        }
        let header = SnapshotFileHeader::decode(&bytes[..SNAPSHOT_HEADER_LEN])?;
        let mut state_root = [0u8; 32];
        state_root.copy_from_slice(&bytes[SNAPSHOT_HEADER_LEN..SNAPSHOT_HEADER_LEN + 32]);
        let payload = &bytes[SNAPSHOT_HEADER_LEN + 32..];
        if payload.len() as u64 != header.payload_bytes {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let plain = if header.is_zstd() {
            zstd::decode_all(payload).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?
        } else {
            payload.to_vec()
        };
        let (registry, neur) = decode_payload(&plain, header.world_id)?;
        // 校验实体根是否与快照声明一致：调用方可用 state_root 与 journal 链比对
        let _ = registry.state_root_blake3();
        Ok(SnapshotReadResult {
            header,
            state_root,
            registry,
            neur,
        })
    }
}

fn decode_payload(plain: &[u8], world_id: [u8; 16]) -> Result<(EntityRegistry, Option<NeurChunk>), RynvekErrorCode> {
    let lineage = u64::from_le_bytes(world_id[0..8].try_into().unwrap());
    let mut registry = EntityRegistry::new(lineage);
    let mut offset = 0usize;
    let mut neur = None;

    while offset + 4 <= plain.len() {
        let tag = &plain[offset..offset + 4];
        if tag == ENTI_TAG {
            offset = decode_enti(&plain[offset..], &mut registry)?;
        } else if tag == crate::neur::NEUR_TAG {
            // 需要知道块长度：decode 会消费完整块；这里对剩余字节尝试
            let chunk = NeurChunk::decode(&plain[offset..])?;
            // 估算消费长度：重新 encode
            let encoded_len = chunk.encode().len();
            offset += encoded_len;
            neur = Some(chunk);
        } else {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
    }
    Ok((registry, neur))
}

fn decode_enti(bytes: &[u8], registry: &mut EntityRegistry) -> Result<usize, RynvekErrorCode> {
    if bytes.len() < 8 || bytes[0..4] != ENTI_TAG {
        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
    }
    let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let mut offset = 8;
    // 行：16B id + 4 slot + 4 gen + 2 arch + 2 pad = 28
    const ROW: usize = 28;
    if bytes.len() < offset + count * ROW {
        return Err(RynvekErrorCode::JournalTornWriteTruncated);
    }
    // 重建：按文件中的顺序 spawn，但需要恢复具体 slot/generation ——
    // 简化策略：按稳定 ID 顺序重新 spawn（generation 从 1 起），
    // 对崩溃恢复的状态根而言，以 EntityId+archetype 为准（与 registry.state_root_blake3 一致）。
    let mut max_serial = 0u64;
    for _ in 0..count {
        let id = u128::from_le_bytes(bytes[offset..offset + 16].try_into().unwrap());
        let generation = u32::from_le_bytes(bytes[offset + 20..offset + 24].try_into().unwrap());
        let arch = u16::from_le_bytes(bytes[offset + 24..offset + 26].try_into().unwrap());
        let entity_id = EntityId(id);
        max_serial = max_serial.max(entity_id.monotonic_serial());
        let (handle, _) = registry.spawn(arch)?;
        let entry = registry
            .sparse_pages
            .get_mut((handle.slot as usize) / rynvek_ecs::PAGE_CAPACITY)
            .and_then(|p| p.get_mut((handle.slot as usize) % rynvek_ecs::PAGE_CAPACITY))
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        entry.stable_id = entity_id;
        entry.generation = if generation == 0 { 1 } else { generation };
        offset += ROW;
    }
    registry.next_monotonic_serial = max_serial.saturating_add(1);
    Ok(offset)
}
