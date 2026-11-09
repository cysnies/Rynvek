// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `NEUR` 稀疏神经分块（GAP-09）：64-bit `spike_history_ring` + 可塑性偏移的零游程编码。
//!
//! M3 补齐语义（见 `docs/ERRATA.md` E-008）：每行对应一个隐藏神经元，额外保存膜电位、
//! 不应期与资格迹，使 `rynvek-neural` 能从快照位级一致地续跑。
//!
//! 二进制布局（Little-Endian）：
//!
//! ```text
//! 块头 16B : "NEUR" | version u16 (=1) | reserved u16 | body_len u32 | row_count u32
//! 行       : dense_index u32 | neuron_index u16 | model_kind u8 | refractory_ticks u8
//!            | spike_history_ring u64 | membrane_v_q8 i16
//!            | ZRLE(weight_deltas) | ZRLE(eligibility)
//! ZRLE     : total_len u16，随后重复 [zero_run u16 | literal_len u16 | literal i16 × literal_len]
//!            直到覆盖 total_len 个元素
//! ```
//!
//! 整个 `.rsnap` 负载随后再做 Zstd level 3 压缩（见 `writer.rs`）。

use rynvek_core::error::RynvekErrorCode;

/// 分块标签
pub const NEUR_TAG: [u8; 4] = *b"NEUR";
/// 当前 NEUR 分块格式版本
pub const NEUR_FORMAT_VERSION: u16 = 1;
/// 块头长度
const NEUR_HEADER_LEN: usize = 16;

/// 单个隐藏神经元的稀疏快照行
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NeurNeuronRow {
    /// 所属脑实例（实体）的紧凑下标
    pub dense_index: u32,
    /// 脑内神经元序号
    pub neuron_index: u16,
    /// `NeuronModelKind` 的 `repr(u8)` 值
    pub model_kind: u8,
    pub refractory_ticks: u8,
    pub spike_history_ring: u64,
    pub membrane_v_q8: i16,
    /// 后天可塑性偏移 δw（稠密向量，序列化时零游程编码）
    pub weight_deltas: Vec<i16>,
    /// STDP 资格迹（稠密向量，序列化时零游程编码）
    pub eligibility: Vec<i16>,
}

/// `NEUR` 分块
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NeurChunk {
    pub rows: Vec<NeurNeuronRow>,
}

/// 零游程编码
fn encode_zrle(values: &[i16], out: &mut Vec<u8>) {
    debug_assert!(values.len() <= u16::MAX as usize);
    out.extend_from_slice(&(values.len() as u16).to_le_bytes());
    let mut i = 0;
    while i < values.len() {
        let zero_start = i;
        while i < values.len() && values[i] == 0 && i - zero_start < u16::MAX as usize {
            i += 1;
        }
        let zero_run = i - zero_start;
        let lit_start = i;
        while i < values.len() && values[i] != 0 && i - lit_start < u16::MAX as usize {
            i += 1;
        }
        out.extend_from_slice(&(zero_run as u16).to_le_bytes());
        out.extend_from_slice(&((i - lit_start) as u16).to_le_bytes());
        for v in &values[lit_start..i] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}

/// 有界读取游标：所有越界一律视为撕裂截断
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], RynvekErrorCode> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(RynvekErrorCode::JournalTornWriteTruncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(RynvekErrorCode::JournalTornWriteTruncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, RynvekErrorCode> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, RynvekErrorCode> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap_or([0; 2])))
    }

    fn i16(&mut self) -> Result<i16, RynvekErrorCode> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap_or([0; 2])))
    }

    fn u32(&mut self) -> Result<u32, RynvekErrorCode> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap_or([0; 4])))
    }

    fn u64(&mut self) -> Result<u64, RynvekErrorCode> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap_or([0; 8])))
    }

    fn zrle(&mut self) -> Result<Vec<i16>, RynvekErrorCode> {
        let total = self.u16()? as usize;
        let mut values = Vec::with_capacity(total);
        while values.len() < total {
            let zero_run = self.u16()? as usize;
            let literal_len = self.u16()? as usize;
            if zero_run == 0 && literal_len == 0 {
                // 空游程会导致死循环：判定为损坏
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
            if values.len() + zero_run + literal_len > total {
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
            values.resize(values.len() + zero_run, 0);
            for _ in 0..literal_len {
                values.push(self.i16()?);
            }
        }
        Ok(values)
    }
}

impl NeurChunk {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for row in &self.rows {
            body.extend_from_slice(&row.dense_index.to_le_bytes());
            body.extend_from_slice(&row.neuron_index.to_le_bytes());
            body.push(row.model_kind);
            body.push(row.refractory_ticks);
            body.extend_from_slice(&row.spike_history_ring.to_le_bytes());
            body.extend_from_slice(&row.membrane_v_q8.to_le_bytes());
            encode_zrle(&row.weight_deltas, &mut body);
            encode_zrle(&row.eligibility, &mut body);
        }
        let mut out = Vec::with_capacity(NEUR_HEADER_LEN + body.len());
        out.extend_from_slice(&NEUR_TAG);
        out.extend_from_slice(&NEUR_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.rows.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// 解码字节流前缀中的一个 NEUR 块，返回 `(块, 消费字节数)`
    pub fn decode_prefix(bytes: &[u8]) -> Result<(Self, usize), RynvekErrorCode> {
        if bytes.len() < NEUR_HEADER_LEN || bytes[0..4] != NEUR_TAG {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let mut header = Cursor { bytes, pos: 4 };
        if header.u16()? != NEUR_FORMAT_VERSION {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let _reserved = header.u16()?;
        let body_len = header.u32()? as usize;
        let row_count = header.u32()? as usize;
        let total = NEUR_HEADER_LEN + body_len;
        let body = bytes
            .get(NEUR_HEADER_LEN..total)
            .ok_or(RynvekErrorCode::JournalTornWriteTruncated)?;

        let mut cur = Cursor { bytes: body, pos: 0 };
        let mut rows = Vec::with_capacity(row_count.min(body_len / 16 + 1));
        for _ in 0..row_count {
            let dense_index = cur.u32()?;
            let neuron_index = cur.u16()?;
            let model_kind = cur.u8()?;
            let refractory_ticks = cur.u8()?;
            let spike_history_ring = cur.u64()?;
            let membrane_v_q8 = cur.i16()?;
            let weight_deltas = cur.zrle()?;
            let eligibility = cur.zrle()?;
            rows.push(NeurNeuronRow {
                dense_index,
                neuron_index,
                model_kind,
                refractory_ticks,
                spike_history_ring,
                membrane_v_q8,
                weight_deltas,
                eligibility,
            });
        }
        if cur.pos != body.len() {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok((Self { rows }, total))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        Self::decode_prefix(bytes).map(|(chunk, _)| chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> NeurNeuronRow {
        NeurNeuronRow {
            dense_index: 3,
            neuron_index: 7,
            model_kind: 1,
            refractory_ticks: 2,
            spike_history_ring: 0b1011,
            membrane_v_q8: -12,
            weight_deltas: vec![0, 0, 0, 5, -2, 0, 0, 9, 0, 0],
            eligibility: vec![0; 16],
        }
    }

    #[test]
    fn neur_roundtrip_with_zero_run_length() {
        let chunk = NeurChunk {
            rows: vec![sample_row(), NeurNeuronRow::default()],
        };
        let bytes = chunk.encode();
        let (decoded, used) = NeurChunk::decode_prefix(&bytes).unwrap();
        assert_eq!(decoded, chunk);
        assert_eq!(used, bytes.len());
    }

    #[test]
    fn zero_run_compresses_all_zero_vectors() {
        let mut out = Vec::new();
        encode_zrle(&[0i16; 32], &mut out);
        // total_len + 一个游程头（zero_run=32, literal_len=0）
        assert_eq!(out.len(), 2 + 4);
    }

    #[test]
    fn truncated_chunk_is_rejected() {
        let bytes = NeurChunk {
            rows: vec![sample_row()],
        }
        .encode();
        assert_eq!(
            NeurChunk::decode(&bytes[..bytes.len() - 1]),
            Err(RynvekErrorCode::JournalTornWriteTruncated)
        );
    }
}
