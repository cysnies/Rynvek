// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rsnap` 写入：实体 ENTI 块 + 可选 NEUR 块，整体 Zstd level 3 压缩。

use std::fs::File;
use std::io::Write;
use std::path::Path;

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use rynvek_ecs::EntityRegistry;
use rynvek_journal::{CommitManifest, CommitSubRoots};

use crate::header::SnapshotFileHeader;
use crate::neur::NeurChunk;

/// 未压缩分块标签：实体表
pub const ENTI_TAG: [u8; 4] = *b"ENTI";

/// 快照写入器
#[derive(Debug, Default)]
pub struct SnapshotWriter;

/// 写入结果
#[derive(Clone, Debug)]
pub struct SnapshotWriteOutput {
    pub header: SnapshotFileHeader,
    pub state_root: [u8; 32],
    pub commit: CommitManifest,
}

impl SnapshotWriter {
    /// 将实体注册表（及可选 NEUR）写成 `.rsnap`
    pub fn write_path(
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        parent_commit_hash: [u8; 32],
        registry: &EntityRegistry,
        neur: Option<&NeurChunk>,
    ) -> Result<SnapshotWriteOutput, RynvekErrorCode> {
        let entity_root = registry.state_root_blake3();
        let mut plain = Vec::new();
        plain.extend_from_slice(&encode_enti_chunk(registry));
        let mut chunk_count = 1u16;
        if let Some(neur) = neur {
            plain.extend_from_slice(&neur.encode());
            chunk_count += 1;
        }

        let compressed =
            zstd::encode_all(plain.as_slice(), 3).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;

        let commit = CommitManifest::new(
            world_id,
            tick,
            world_revision,
            parent_commit_hash,
            CommitSubRoots {
                entity_root_hash: entity_root,
                field_root_hash: Blake3Hash::of(b"field-empty").0,
                neural_root_hash: neur.map(|n| Blake3Hash::of(&n.encode()).0).unwrap_or([0; 32]),
                economy_root_hash: Blake3Hash::of(b"economy-empty").0,
                rng_root_hash: Blake3Hash::of(b"rng-empty").0,
            },
        );

        let header = SnapshotFileHeader::new(
            world_id,
            tick,
            world_revision,
            chunk_count,
            compressed.len() as u64,
            true,
        );

        let mut file = File::create(path.as_ref()).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        file.write_all(&header.encode())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.write_all(&commit.state_root_hash)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.write_all(&compressed)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.flush().map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;

        Ok(SnapshotWriteOutput {
            header,
            state_root: commit.state_root_hash,
            commit,
        })
    }
}

fn encode_enti_chunk(registry: &EntityRegistry) -> Vec<u8> {
    let mut rows: Vec<_> = registry
        .iter_alive()
        .map(|(_, handle, id)| {
            let arch = registry
                .sparse_pages
                .get((handle.slot as usize) / rynvek_ecs::PAGE_CAPACITY)
                .and_then(|p| p.get((handle.slot as usize) % rynvek_ecs::PAGE_CAPACITY))
                .map(|e| e.archetype_id)
                .unwrap_or(0);
            (id.0, handle.slot, handle.generation, arch)
        })
        .collect();
    rows.sort_by_key(|(id, _, _, _)| *id);

    let mut out = Vec::new();
    out.extend_from_slice(&ENTI_TAG);
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    for (id, slot, generation, arch) in rows {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&slot.to_le_bytes());
        out.extend_from_slice(&generation.to_le_bytes());
        out.extend_from_slice(&arch.to_le_bytes());
        out.extend_from_slice(&[0u8; 2]); // padding 对齐到 28 字节行
    }
    out
}
