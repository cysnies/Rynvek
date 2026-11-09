// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `NEUR` 稀疏神经分块（GAP-09）：64-bit `spike_history_ring` + 可塑性偏移的零游程编码。
//!
//! M1 仅落地二进制容器与编解码；完整神经运行时在 M3（`rynvek-neural`）填充语义。

use rynvek_core::error::RynvekErrorCode;

/// 分块标签
pub const NEUR_TAG: [u8; 4] = *b"NEUR";

/// 单个脑实例的稀疏快照行
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeurNeuronRow {
    pub dense_index: u32,
    pub spike_history_ring: u64,
    /// 非零可塑性偏移 `(synapse_index, delta_q8)`，已按 synapse_index 升序
    pub weight_deltas: Vec<(u16, i16)>,
}

/// `NEUR` 分块
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NeurChunk {
    pub rows: Vec<NeurNeuronRow>,
}

impl NeurChunk {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&NEUR_TAG);
        out.extend_from_slice(&(self.rows.len() as u32).to_le_bytes());
        for row in &self.rows {
            out.extend_from_slice(&row.dense_index.to_le_bytes());
            out.extend_from_slice(&row.spike_history_ring.to_le_bytes());
            out.extend_from_slice(&(row.weight_deltas.len() as u16).to_le_bytes());
            // 零游程：只写非零项
            for (idx, delta) in &row.weight_deltas {
                out.extend_from_slice(&idx.to_le_bytes());
                out.extend_from_slice(&delta.to_le_bytes());
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() < 8 || bytes[0..4] != NEUR_TAG {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let row_count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let mut offset = 8;
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            if bytes.len() < offset + 4 + 8 + 2 {
                return Err(RynvekErrorCode::JournalTornWriteTruncated);
            }
            let dense_index = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            offset += 4;
            let spike_history_ring = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let delta_count = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
            offset += 2;
            if bytes.len() < offset + delta_count * 4 {
                return Err(RynvekErrorCode::JournalTornWriteTruncated);
            }
            let mut weight_deltas = Vec::with_capacity(delta_count);
            for _ in 0..delta_count {
                let idx = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
                let delta = i16::from_le_bytes(bytes[offset + 2..offset + 4].try_into().unwrap());
                offset += 4;
                weight_deltas.push((idx, delta));
            }
            rows.push(NeurNeuronRow {
                dense_index,
                spike_history_ring,
                weight_deltas,
            });
        }
        Ok(Self { rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neur_roundtrip_skips_zero_deltas() {
        let chunk = NeurChunk {
            rows: vec![NeurNeuronRow {
                dense_index: 3,
                spike_history_ring: 0b1011,
                weight_deltas: vec![(0, 5), (7, -2)],
            }],
        };
        let decoded = NeurChunk::decode(&chunk.encode()).unwrap();
        assert_eq!(decoded, chunk);
    }
}
