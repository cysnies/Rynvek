// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rsnap` 写入器：将 `EntityRegistry`、可选 `NeurChunk` 与扩展世界分块（`CONF` / `COMP` / `FILD` / `ECON` / `EVOL` / `FOGW` / `RNGS` / `ROOT`）打包为 Zstd 压缩载荷并支持原子落盘。
//!
//! 压缩前载荷布局：
//! - 块 0：`ENTS`（4B tag + 4B len + 32B blake3 + EntityRegistry 快照字节）
//! - 块 1（可选）：`NEUR`（4B tag + 4B len + 32B blake3 + `NeurChunk::encode()`）
//! - 块 2..N（可选）：扩展领域分块（4B tag + 4B len + 32B blake3 + 分块字节）

use std::fs::File;
use std::io::Write;
use std::path::Path;

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use rynvek_ecs::EntityRegistry;
use rynvek_journal::{CommitManifest, CommitSubRoots};

use crate::header::{SNAPSHOT_HEADER_LEN, SnapshotFileHeader};
use crate::neur::{NEUR_TAG, NeurChunk};

pub const ENTI_TAG: [u8; 4] = *b"ENTI";
pub const ENTS_TAG: [u8; 4] = *b"ENTI";
pub const CONF_TAG: [u8; 4] = *b"CONF";
pub const COMP_TAG: [u8; 4] = *b"COMP";
pub const FILD_TAG: [u8; 4] = *b"FILD";
pub const ECON_TAG: [u8; 4] = *b"ECON";
pub const EVOL_TAG: [u8; 4] = *b"EVOL";
pub const FOGW_TAG: [u8; 4] = *b"FOGW";
pub const RNGS_TAG: [u8; 4] = *b"RNGS";
pub const CMIT_TAG: [u8; 4] = *b"CMIT";
pub const MAPT_TAG: [u8; 4] = *b"MAPT";
pub const ROOT_TAG: [u8; 4] = *b"ROOT";

/// Zstd 默认压缩级别（规范 10.1：level 3）
pub const SNAPSHOT_ZSTD_LEVEL: i32 = 3;

/// 扩展自定义分块（供上层世界保存 `CONF` / `COMP` / `FILD` / `ECON` / `EVOL` / `FOGW` / `RNGS` / `ROOT` 等分块）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotCustomChunk {
    pub tag: [u8; 4],
    pub payload: Vec<u8>,
}

/// 写入完成后的元数据回执
#[derive(Clone, Debug)]
pub struct SnapshotWriteOutput {
    pub header: SnapshotFileHeader,
    pub state_root: [u8; 32],
    pub commit: CommitManifest,
    pub total_bytes: usize,
}

/// 快照写入器
#[derive(Debug, Default)]
pub struct SnapshotWriter;

pub(crate) fn write_chunk(out: &mut Vec<u8>, tag: [u8; 4], body: &[u8]) {
    let hash = Blake3Hash::of(body);
    out.extend_from_slice(&tag);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(hash.as_bytes());
    out.extend_from_slice(body);
}

/// 根据实体根、可选神经分块与可选扩展分块确定性计算快照 `CommitSubRoots`
pub fn compute_snapshot_sub_roots(
    entity_root: [u8; 32],
    neur: Option<&NeurChunk>,
    extra_chunks: &[SnapshotCustomChunk],
) -> CommitSubRoots {
    let neural_root = neur
        .map(|n| *Blake3Hash::of(&n.encode()).as_bytes())
        .unwrap_or([0u8; 32]);
    let find_hash = |tag: [u8; 4], default_label: &[u8]| -> [u8; 32] {
        extra_chunks
            .iter()
            .find(|c| c.tag == tag)
            .map(|c| *Blake3Hash::of(&c.payload).as_bytes())
            .unwrap_or_else(|| *Blake3Hash::of(default_label).as_bytes())
    };
    CommitSubRoots {
        entity_root_hash: entity_root,
        field_root_hash: find_hash(FILD_TAG, b"field-empty"),
        neural_root_hash: neural_root,
        economy_root_hash: find_hash(ECON_TAG, b"economy-empty"),
        rng_root_hash: find_hash(RNGS_TAG, b"rng-empty"),
    }
}

impl SnapshotWriter {
    /// 序列化为完整 `.rsnap` 字节流（基础接口：含 `ENTS` 与可选 `NEUR`）
    pub fn write_bytes(
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        parent_state_root: [u8; 32],
        registry: &EntityRegistry,
        neur: Option<&NeurChunk>,
    ) -> Result<(Vec<u8>, SnapshotWriteOutput), RynvekErrorCode> {
        Self::write_bytes_with_chunks(world_id, tick, world_revision, parent_state_root, registry, neur, &[])
    }

    /// 序列化为包含扩展领域分块的完整 `.rsnap` 字节流
    pub fn write_bytes_with_chunks(
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        parent_state_root: [u8; 32],
        registry: &EntityRegistry,
        neur: Option<&NeurChunk>,
        extra_chunks: &[SnapshotCustomChunk],
    ) -> Result<(Vec<u8>, SnapshotWriteOutput), RynvekErrorCode> {
        let mut tags = std::collections::BTreeSet::from([ENTS_TAG, *b"ENTS", NEUR_TAG]);
        for chunk in extra_chunks {
            if !tags.insert(chunk.tag) {
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
        }
        let ents_bytes = registry.encode_snapshot_bytes();
        let entity_root = registry.state_root_blake3();

        let mut uncompressed = Vec::with_capacity(ents_bytes.len() + 256);
        write_chunk(&mut uncompressed, ENTS_TAG, &ents_bytes);
        let mut chunk_count = 1u16;

        if let Some(n) = neur {
            let n_bytes = n.encode();
            write_chunk(&mut uncompressed, NEUR_TAG, &n_bytes);
            chunk_count = chunk_count
                .checked_add(1)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?;
        }

        for extra in extra_chunks {
            write_chunk(&mut uncompressed, extra.tag, &extra.payload);
            chunk_count = chunk_count
                .checked_add(1)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?;
        }

        let sub = compute_snapshot_sub_roots(entity_root, neur, extra_chunks);
        let commit = if let Some(chunk) = extra_chunks.iter().find(|c| c.tag == CMIT_TAG) {
            let commit = CommitManifest::decode(&chunk.payload)?;
            if commit.canonical_domains.is_none()
                || commit.world_id != world_id
                || commit.tick != tick
                || commit.world_revision != world_revision
                || commit.parent_commit_hash != parent_state_root
            {
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
            commit
        } else {
            CommitManifest::new(world_id, tick, world_revision, parent_state_root, sub)
        };

        let compressed = zstd::bulk::compress(&uncompressed, SNAPSHOT_ZSTD_LEVEL)
            .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        let mut header = SnapshotFileHeader::new(
            world_id,
            tick,
            world_revision,
            chunk_count,
            compressed.len() as u64,
            true,
        );

        if commit.canonical_domains.is_some() {
            header.format_version = crate::header::CANONICAL_SNAPSHOT_VERSION;
        }
        let mut file_bytes = Vec::with_capacity(SNAPSHOT_HEADER_LEN + 32 + compressed.len());
        file_bytes.extend_from_slice(&header.encode());
        file_bytes.extend_from_slice(&commit.state_root_hash);
        file_bytes.extend_from_slice(&compressed);

        let total_bytes = file_bytes.len();
        Ok((
            file_bytes,
            SnapshotWriteOutput {
                header,
                state_root: commit.state_root_hash,
                commit,
                total_bytes,
            },
        ))
    }

    /// 原子写入磁盘文件（先写临时文件 `.tmp` + `sync_all()`，再 `rename` 覆盖目标路径）
    pub fn write_path(
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        parent_state_root: [u8; 32],
        registry: &EntityRegistry,
        neur: Option<&NeurChunk>,
    ) -> Result<SnapshotWriteOutput, RynvekErrorCode> {
        Self::write_path_with_chunks(
            path,
            world_id,
            tick,
            world_revision,
            parent_state_root,
            registry,
            neur,
            &[],
        )
    }

    /// 原子写入带扩展领域分块的 `.rsnap` 快照文件
    #[allow(clippy::too_many_arguments)]
    pub fn write_path_with_chunks(
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        parent_state_root: [u8; 32],
        registry: &EntityRegistry,
        neur: Option<&NeurChunk>,
        extra_chunks: &[SnapshotCustomChunk],
    ) -> Result<SnapshotWriteOutput, RynvekErrorCode> {
        let target = path.as_ref();
        let (bytes, out) = Self::write_bytes_with_chunks(
            world_id,
            tick,
            world_revision,
            parent_state_root,
            registry,
            neur,
            extra_chunks,
        )?;
        let tmp_path = target.with_extension("rsnap.tmp");
        let mut f = File::create(&tmp_path).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        f.write_all(&bytes)
            .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        f.sync_all().map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        drop(f);
        std::fs::rename(&tmp_path, target).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        Ok(out)
    }
}
