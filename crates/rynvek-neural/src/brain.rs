// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! LIF 脑实例：封装规范 05 的 SoA 状态切片，并与 `NEUR` 快照分块双向映射。
//!
//! 权重布局与 `step_lif_brain_deterministic` 一致：
//! `[0, n_hid*16)` 为输入→隐藏行（每神经元 16 个），`[n_hid*16, n_hid*22)` 为隐藏→输出（每神经元 6 个）。

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::fixed::Q8_8;
use rynvek_core::hash::Blake3Hash;
use rynvek_snapshot::{NeurChunk, NeurNeuronRow};

use crate::runtime::{NeuronModelKind, step_lif_brain_deterministic};

/// 感知输入维度
pub const LIF_INPUTS: usize = 16;
/// 动作读出维度
pub const LIF_OUTPUTS: usize = 6;

/// 单个 LIF 脑实例的全部可变状态
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifBrain {
    pub membrane_v_q8: Vec<Q8_8>,
    pub refractory_ticks: Vec<u8>,
    pub spike_ring: Vec<u64>,
    /// 先天权重（基因组表达，快照中不保存，由 Genome 重建）
    pub weights_base_q8: Vec<i16>,
    /// 后天可塑性偏移 δw
    pub weights_delta_q8: Vec<i16>,
    pub eligibility_q8: Vec<i16>,
}

impl LifBrain {
    pub fn new(n_hid: usize, weights_base_q8: Vec<i16>) -> Result<Self, RynvekErrorCode> {
        if weights_base_q8.len() != n_hid * (LIF_INPUTS + LIF_OUTPUTS) {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        Ok(Self {
            membrane_v_q8: vec![Q8_8::ZERO; n_hid],
            refractory_ticks: vec![0; n_hid],
            spike_ring: vec![0; n_hid],
            weights_delta_q8: vec![0; weights_base_q8.len()],
            weights_base_q8,
            eligibility_q8: vec![0; n_hid * LIF_INPUTS],
        })
    }

    pub fn n_hid(&self) -> usize {
        self.membrane_v_q8.len()
    }

    /// 推进一个神经子步
    pub fn step(
        &mut self,
        sensory: &[Q8_8; LIF_INPUTS],
        modulators_q8: [i16; 4],
        plasticity: bool,
    ) -> [Q8_8; LIF_OUTPUTS] {
        step_lif_brain_deterministic(
            sensory,
            &mut self.membrane_v_q8,
            &mut self.refractory_ticks,
            &mut self.spike_ring,
            &self.weights_base_q8,
            &mut self.weights_delta_q8,
            &mut self.eligibility_q8,
            modulators_q8,
            plasticity,
        )
    }

    /// 神经状态根（参与 `TickCommitManifest::neural_root_hash`）
    pub fn state_root(&self) -> Blake3Hash {
        Blake3Hash::of(&self.to_neur_chunk(0).encode())
    }

    /// 导出为 NEUR 行（每个隐藏神经元一行）
    pub fn to_neur_rows(&self, dense_index: u32) -> Vec<NeurNeuronRow> {
        let n = self.n_hid();
        (0..n)
            .map(|h| {
                let mut deltas = Vec::with_capacity(LIF_INPUTS + LIF_OUTPUTS);
                deltas.extend_from_slice(&self.weights_delta_q8[h * LIF_INPUTS..(h + 1) * LIF_INPUTS]);
                let out_off = n * LIF_INPUTS + h * LIF_OUTPUTS;
                deltas.extend_from_slice(&self.weights_delta_q8[out_off..out_off + LIF_OUTPUTS]);
                NeurNeuronRow {
                    dense_index,
                    neuron_index: h as u16,
                    model_kind: NeuronModelKind::LIF as u8,
                    refractory_ticks: self.refractory_ticks[h],
                    spike_history_ring: self.spike_ring[h],
                    membrane_v_q8: self.membrane_v_q8[h].0,
                    weight_deltas: deltas,
                    eligibility: self.eligibility_q8[h * LIF_INPUTS..(h + 1) * LIF_INPUTS].to_vec(),
                }
            })
            .collect()
    }

    pub fn to_neur_chunk(&self, dense_index: u32) -> NeurChunk {
        NeurChunk {
            rows: self.to_neur_rows(dense_index),
        }
    }

    /// 从 NEUR 行恢复后天状态（先天权重保持不变）；只处理 `dense_index` 匹配的行
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk, dense_index: u32) -> Result<(), RynvekErrorCode> {
        let n = self.n_hid();
        let mut seen = vec![false; n];
        for row in chunk.rows.iter().filter(|r| r.dense_index == dense_index) {
            let h = row.neuron_index as usize;
            if h >= n
                || row.model_kind != NeuronModelKind::LIF as u8
                || row.weight_deltas.len() != LIF_INPUTS + LIF_OUTPUTS
                || row.eligibility.len() != LIF_INPUTS
            {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
            self.membrane_v_q8[h] = Q8_8(row.membrane_v_q8);
            self.refractory_ticks[h] = row.refractory_ticks;
            self.spike_ring[h] = row.spike_history_ring;
            self.weights_delta_q8[h * LIF_INPUTS..(h + 1) * LIF_INPUTS]
                .copy_from_slice(&row.weight_deltas[..LIF_INPUTS]);
            let out_off = n * LIF_INPUTS + h * LIF_OUTPUTS;
            self.weights_delta_q8[out_off..out_off + LIF_OUTPUTS].copy_from_slice(&row.weight_deltas[LIF_INPUTS..]);
            self.eligibility_q8[h * LIF_INPUTS..(h + 1) * LIF_INPUTS].copy_from_slice(&row.eligibility);
            seen[h] = true;
        }
        if seen.iter().all(|&s| s) {
            Ok(())
        } else {
            Err(RynvekErrorCode::DimensionMismatch)
        }
    }
}
