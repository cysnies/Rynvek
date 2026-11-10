// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rsnap` 读取器：有界解压 + 校验每个分块的 BLAKE3 + 核验分块计数 + 恢复 `EntityRegistry`、`NeurChunk` 与扩展分块。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use rynvek_ecs::EntityRegistry;

use crate::header::{SNAPSHOT_HEADER_LEN, SnapshotFileHeader};
use crate::neur::{NEUR_TAG, NeurChunk};
use crate::writer::{ENTS_TAG, SnapshotCustomChunk};

/// 解压后快照最大允许体积（256 MiB，防止恶意或损坏 Zstd 压缩炸弹耗尽内存）
pub const MAX_SNAPSHOT_DECOMPRESSED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct SnapshotReadResult {
    pub header: SnapshotFileHeader,
    pub state_root: [u8; 32],
    pub registry: EntityRegistry,
    pub neur: Option<NeurChunk>,
    /// 除 `ENTS` 与 `NEUR` 外的扩展领域分块（如 `CONF`、`COMP`、`FILD`、`ECON`、`EVOL`、`FOGW`、`RNGS`、`ROOT`）
    pub custom_chunks: Vec<SnapshotCustomChunk>,
}

impl SnapshotReadResult {
    /// 按 4 字节标签查找扩展分块载荷
    pub fn find_chunk(&self, tag: [u8; 4]) -> Option<&[u8]> {
        self.custom_chunks
            .iter()
            .find(|c| c.tag == tag)
            .map(|c| c.payload.as_slice())
    }
}

#[derive(Debug, Default)]
pub struct SnapshotReader;

impl SnapshotReader {
    pub fn read_path(path: impl AsRef<Path>) -> Result<SnapshotReadResult, RynvekErrorCode> {
        let f = File::open(path.as_ref()).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let mut bytes = Vec::new();
        f.take((MAX_SNAPSHOT_DECOMPRESSED_BYTES + SNAPSHOT_HEADER_LEN + 64) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        if bytes.len() > MAX_SNAPSHOT_DECOMPRESSED_BYTES + SNAPSHOT_HEADER_LEN + 32 {
            return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
        }
        Self::read_bytes(&bytes)
    }

    pub fn read_bytes(bytes: &[u8]) -> Result<SnapshotReadResult, RynvekErrorCode> {
        if bytes.len() < SNAPSHOT_HEADER_LEN + 32 {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let header = SnapshotFileHeader::decode(&bytes[..SNAPSHOT_HEADER_LEN])?;
        let mut state_root = [0u8; 32];
        state_root.copy_from_slice(&bytes[SNAPSHOT_HEADER_LEN..SNAPSHOT_HEADER_LEN + 32]);
        let payload_start = SNAPSHOT_HEADER_LEN + 32;
        let payload_end = payload_start
            .checked_add(header.payload_bytes as usize)
            .ok_or(RynvekErrorCode::JournalTornWriteTruncated)?;
        if bytes.len() < payload_end {
            return Err(RynvekErrorCode::JournalTornWriteTruncated);
        }
        let compressed = &bytes[payload_start..payload_end];
        let uncompressed = if header.is_zstd() {
            let mut decoder = zstd::stream::read::Decoder::new(compressed)
                .map_err(|_| RynvekErrorCode::SnapshotChunkChecksumFailed)?;
            let mut out = Vec::new();
            decoder
                .by_ref()
                .take((MAX_SNAPSHOT_DECOMPRESSED_BYTES + 1) as u64)
                .read_to_end(&mut out)
                .map_err(|_| RynvekErrorCode::SnapshotChunkChecksumFailed)?;
            if out.len() > MAX_SNAPSHOT_DECOMPRESSED_BYTES {
                return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
            }
            out
        } else {
            if compressed.len() > MAX_SNAPSHOT_DECOMPRESSED_BYTES {
                return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
            }
            compressed.to_vec()
        };

        let mut offset = 0usize;
        let mut parsed_chunks = 0u16;
        let mut registry: Option<EntityRegistry> = None;
        let mut neur: Option<NeurChunk> = None;
        let mut custom_chunks: Vec<SnapshotCustomChunk> = Vec::new();

        while offset < uncompressed.len() {
            if uncompressed.len() - offset < 40 {
                return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
            }
            let tag: [u8; 4] = uncompressed[offset..offset + 4].try_into().unwrap();
            let len = u32::from_le_bytes(uncompressed[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let expected_hash: [u8; 32] = uncompressed[offset + 8..offset + 40].try_into().unwrap();
            let body_start = offset + 40;
            let body_end = body_start
                .checked_add(len)
                .ok_or(RynvekErrorCode::SnapshotChunkChecksumFailed)?;
            if uncompressed.len() < body_end {
                return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
            }
            let body = &uncompressed[body_start..body_end];
            if Blake3Hash::of(body).as_bytes() != &expected_hash {
                return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
            }
            if tag == ENTS_TAG {
                registry = Some(EntityRegistry::decode_snapshot_bytes(body)?);
            } else if tag == NEUR_TAG {
                neur = Some(NeurChunk::decode(body)?);
            } else {
                custom_chunks.push(SnapshotCustomChunk {
                    tag,
                    payload: body.to_vec(),
                });
            }
            parsed_chunks = parsed_chunks
                .checked_add(1)
                .ok_or(RynvekErrorCode::SnapshotChunkChecksumFailed)?;
            offset = body_end;
        }

        if parsed_chunks != header.chunk_count {
            return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
        }

        let registry = registry.ok_or(RynvekErrorCode::SnapshotChunkChecksumFailed)?;
        Ok(SnapshotReadResult {
            header,
            state_root,
            registry,
            neur,
            custom_chunks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neur::NeurNeuronRow;
    use crate::writer::{ CONF_TAG, SnapshotWriter };
    use tempfile::TempDir;

    #[test]
    fn snapshot_write_and_read_roundtrip_with_custom_chunks() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("world.rsnap");
        let mut reg = EntityRegistry::new(7);
        let (h0, _) = reg.spawn(1).unwrap();
        let (_h1, _) = reg.spawn(2).unwrap();
        reg.despawn(h0, 12).unwrap();
        reg.recycle_tombstones(4);

        let neur = NeurChunk {
            rows: vec![NeurNeuronRow {
                dense_index: 0,
                neuron_index: 3,
                model_kind: 1,
                refractory_ticks: 1,
                spike_history_ring: 0b101,
                membrane_v_q8: 42,
                weight_deltas: vec![0, 4, 0, -2],
                eligibility: vec![0; 8],
            }],
        };

        let extra = vec![SnapshotCustomChunk {
            tag: CONF_TAG,
            payload: b"duel_seed_42".to_vec(),
        }];

        let out = SnapshotWriter::write_path_with_chunks(&path, [9; 16], 16, 4, [0; 32], &reg, Some(&neur), &extra)
            .unwrap();
        let read = SnapshotReader::read_path(&path).unwrap();
        assert_eq!(read.header.tick, 16);
        assert_eq!(read.header.chunk_count, 3);
        assert_eq!(read.state_root, out.state_root);
        assert_eq!(read.registry.state_root_blake3(), reg.state_root_blake3());
        assert_eq!(read.neur.as_ref(), Some(&neur));
        assert_eq!(read.find_chunk(CONF_TAG), Some(b"duel_seed_42".as_slice()));
    }

    #[test]
    fn corrupted_chunk_payload_or_count_is_rejected() {
        let reg = EntityRegistry::new(1);
        let (mut bytes, _) = SnapshotWriter::write_bytes([1; 16], 1, 1, [0; 32], &reg, None).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(SnapshotReader::read_bytes(&bytes).is_err());

        // 篡改 header.chunk_count 触发计数校验失败
        let (mut bytes2, _) = SnapshotWriter::write_bytes([1; 16], 1, 1, [0; 32], &reg, None).unwrap();
        bytes2[10] = 99;
        assert_eq!(
            SnapshotReader::read_bytes(&bytes2),
            Err(RynvekErrorCode::SnapshotChunkChecksumFailed)
        );
    }
}
