// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 其余三种神经元模型（`GatedMLP` / `Izhikevich` / `CTRNN`）的确定性 Q8.8 定点实现。
//!
//! 规范 05 只给出了 LIF 的冻结源码；这三种模型按经典公式做纯整数离散化（最佳实践，见 ERRATA E-009）：
//!
//! - 所有激活/状态均为 Q8.8（`i16` 原始值），中间量用 `i32`；
//! - 每个突触项先做 `(w * x) >> 8` 再求和，与 [`crate::storage`] 的三种布局保持位级一致；
//! - 统一 `16` 维感知输入与 `6` 维动作读出，与 LIF 核接口对齐（GAP-03 六维动作读出）；
//! - 支持完整的 `state_root()` 与 `NEUR` 稀疏神经分块序列化 / 恢复闭环，确保四种脑模型在快照恢复与状态根哈希中零死角覆盖。

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::fixed::Q8_8;
use rynvek_core::hash::Blake3Hash;
use rynvek_snapshot::{NeurChunk, NeurNeuronRow};

use crate::runtime::NeuronModelKind;
use crate::storage::{NeuralGraphStorage, SynapticMatVec};

/// 感知输入维度
pub const MODEL_INPUTS: usize = 16;
/// 动作读出维度
pub const MODEL_OUTPUTS: usize = 6;

#[inline]
fn syn_term(w: i16, x: i16) -> i32 {
    ((w as i32) * (x as i32)) >> 8
}

#[inline]
fn clamp_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// 硬 Sigmoid：`clamp(z/4 + 0.5, 0, 1)`（Q8.8）
#[inline]
pub fn hard_sigmoid_q8(z: i32) -> i32 {
    ((z >> 2) + 128).clamp(0, 256)
}

/// 硬 Tanh：`clamp(z, -1, 1)`（Q8.8）
#[inline]
pub fn hard_tanh_q8(z: i32) -> i32 {
    z.clamp(-256, 256)
}

/// 行主序 `rows × MODEL_INPUTS` 输入权重矩阵与感知向量相乘
fn input_projection(weights: &[i16], rows: usize, sensory: &[Q8_8; MODEL_INPUTS]) -> Vec<i32> {
    (0..rows)
        .map(|r| {
            weights[r * MODEL_INPUTS..(r + 1) * MODEL_INPUTS]
                .iter()
                .zip(sensory.iter())
                .map(|(&w, x)| syn_term(w, x.0))
                .sum()
        })
        .collect()
}

fn check_len(actual: usize, expected: usize) -> Result<(), RynvekErrorCode> {
    if actual == expected {
        Ok(())
    } else {
        Err(RynvekErrorCode::DimensionMismatch)
    }
}

// ---------------------------------------------------------------------------
// GatedMLP
// ---------------------------------------------------------------------------

/// 门控前馈网络：`h = hard_tanh(W1·x) ⊙ hard_sigmoid(Wg·x + b_g)`，`y = hard_tanh(W2·h)`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatedMlpBrain {
    pub n_hid: usize,
    /// `n_hid × 16`
    pub w_in: Vec<i16>,
    /// `n_hid × 16`
    pub w_gate: Vec<i16>,
    /// `n_hid`
    pub gate_bias: Vec<i16>,
    /// `6 × n_hid`
    pub w_out: Vec<i16>,
    /// 最近一次的隐藏激活（供观测与快照）
    pub hidden_q8: Vec<i16>,
}

impl GatedMlpBrain {
    pub fn new(
        n_hid: usize,
        w_in: Vec<i16>,
        w_gate: Vec<i16>,
        gate_bias: Vec<i16>,
        w_out: Vec<i16>,
    ) -> Result<Self, RynvekErrorCode> {
        check_len(w_in.len(), n_hid * MODEL_INPUTS)?;
        check_len(w_gate.len(), n_hid * MODEL_INPUTS)?;
        check_len(gate_bias.len(), n_hid)?;
        check_len(w_out.len(), MODEL_OUTPUTS * n_hid)?;
        Ok(Self {
            n_hid,
            w_in,
            w_gate,
            gate_bias,
            w_out,
            hidden_q8: vec![0; n_hid],
        })
    }

    pub fn step(&mut self, sensory: &[Q8_8; MODEL_INPUTS]) -> [Q8_8; MODEL_OUTPUTS] {
        let pre = input_projection(&self.w_in, self.n_hid, sensory);
        let gate = input_projection(&self.w_gate, self.n_hid, sensory);
        for j in 0..self.n_hid {
            let g = hard_sigmoid_q8(gate[j] + self.gate_bias[j] as i32);
            self.hidden_q8[j] = ((hard_tanh_q8(pre[j]) * g) >> 8) as i16;
        }
        let mut out = [Q8_8::ZERO; MODEL_OUTPUTS];
        for (k, o) in out.iter_mut().enumerate() {
            let acc: i32 = self.w_out[k * self.n_hid..(k + 1) * self.n_hid]
                .iter()
                .zip(&self.hidden_q8)
                .map(|(&w, &h)| syn_term(w, h))
                .sum();
            *o = Q8_8(hard_tanh_q8(acc) as i16);
        }
        out
    }

    /// 计算 GatedMLP 脑实例的确定性状态根
    pub fn state_root(&self) -> Blake3Hash {
        Blake3Hash::of(&self.to_neur_chunk(0).encode())
    }

    /// 导出为 NEUR 快照行（每个隐藏单元一行，隐藏激活存入 `membrane_v_q8`）
    pub fn to_neur_rows(&self, dense_index: u32) -> Vec<NeurNeuronRow> {
        (0..self.n_hid)
            .map(|h| NeurNeuronRow {
                dense_index,
                neuron_index: h as u16,
                model_kind: NeuronModelKind::GatedMLP as u8,
                refractory_ticks: 0,
                spike_history_ring: 0,
                membrane_v_q8: self.hidden_q8[h],
                weight_deltas: Vec::new(),
                eligibility: Vec::new(),
            })
            .collect()
    }

    /// 导出为独立的 NEUR 分块
    pub fn to_neur_chunk(&self, dense_index: u32) -> NeurChunk {
        NeurChunk {
            rows: self.to_neur_rows(dense_index),
        }
    }

    /// 从 NEUR 分块恢复隐藏激活状态
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk, dense_index: u32) -> Result<(), RynvekErrorCode> {
        let mut seen = vec![false; self.n_hid];
        for row in chunk.rows.iter().filter(|r| r.dense_index == dense_index) {
            let h = row.neuron_index as usize;
            if h >= self.n_hid
                || row.model_kind != NeuronModelKind::GatedMLP as u8
                || !row.weight_deltas.is_empty()
                || !row.eligibility.is_empty()
            {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
            self.hidden_q8[h] = row.membrane_v_q8;
            seen[h] = true;
        }
        if seen.iter().all(|&s| s) {
            Ok(())
        } else {
            Err(RynvekErrorCode::DimensionMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// Izhikevich
// ---------------------------------------------------------------------------

/// Izhikevich 参数（Q8.8；`v` 以 mV 为单位）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IzhikevichParams {
    pub a_q8: i32,
    pub b_q8: i32,
    pub c_q8: i32,
    pub d_q8: i32,
}

impl IzhikevichParams {
    /// 经典 Regular Spiking：a=0.02, b=0.2, c=-65 mV, d=8
    pub const REGULAR_SPIKING: Self = Self {
        a_q8: 5,
        b_q8: 51,
        c_q8: -65 * 256,
        d_q8: 8 * 256,
    };
}

/// 发放阈值 30 mV
pub const IZH_PEAK_Q8: i32 = 30 * 256;
/// 突触电流增益（感知/循环项 Q8.8 → mV）
pub const IZH_CURRENT_GAIN: i32 = 16;

/// 循环脉冲耦合的 Izhikevich 网络
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IzhikevichBrain {
    pub n: usize,
    pub params: IzhikevichParams,
    pub v_q8: Vec<i16>,
    pub u_q8: Vec<i16>,
    pub spike_ring: Vec<u64>,
    /// `n × 16`
    pub w_in: Vec<i16>,
    /// 循环突触（三种布局之一）
    pub w_rec: NeuralGraphStorage,
    /// `6 × n`
    pub w_out: Vec<i16>,
}

impl IzhikevichBrain {
    pub fn new(
        params: IzhikevichParams,
        w_in: Vec<i16>,
        w_rec: NeuralGraphStorage,
        w_out: Vec<i16>,
    ) -> Result<Self, RynvekErrorCode> {
        let n = w_rec.neuron_count();
        check_len(w_in.len(), n * MODEL_INPUTS)?;
        check_len(w_out.len(), MODEL_OUTPUTS * n)?;
        let v0 = params.c_q8;
        let u0 = (params.b_q8 * v0) >> 8;
        Ok(Self {
            n,
            params,
            v_q8: vec![clamp_i16(v0); n],
            u_q8: vec![clamp_i16(u0); n],
            spike_ring: vec![0; n],
            w_in,
            w_rec,
            w_out,
        })
    }

    pub fn step(&mut self, sensory: &[Q8_8; MODEL_INPUTS]) -> [Q8_8; MODEL_OUTPUTS] {
        let p = self.params;
        let mut current = input_projection(&self.w_in, self.n, sensory);
        // 上一 Tick 的脉冲作为循环输入（发放 = 1.0）
        let prev_spikes: Vec<i16> = self
            .spike_ring
            .iter()
            .map(|r| if r & 1 == 1 { 256 } else { 0 })
            .collect();
        self.w_rec.matvec_accumulate_q8(&prev_spikes, &mut current);

        let mut out_acc = [0i32; MODEL_OUTPUTS];
        for (j, &cur) in current.iter().enumerate() {
            let i_syn = cur * IZH_CURRENT_GAIN;
            let mut v = self.v_q8[j] as i32;
            let mut u = self.u_q8[j] as i32;
            // 与 Izhikevich (2003) 参考实现一致：v 以两个 0.5 ms 半步积分
            for _ in 0..2 {
                let dv = ((((v * v) >> 8) * 10) >> 8) + 5 * v + 140 * 256 - u + i_syn;
                v = clamp_i16(v + (dv >> 1)) as i32;
            }
            u += (p.a_q8 * (((p.b_q8 * v) >> 8) - u)) >> 8;

            let spiked = v >= IZH_PEAK_Q8;
            self.spike_ring[j] = (self.spike_ring[j] << 1) | spiked as u64;
            if spiked {
                v = p.c_q8;
                u += p.d_q8;
                for (k, acc) in out_acc.iter_mut().enumerate() {
                    *acc += self.w_out[k * self.n + j] as i32;
                }
            }
            self.v_q8[j] = clamp_i16(v);
            self.u_q8[j] = clamp_i16(u);
        }
        out_acc.map(|a| Q8_8((a >> 2).clamp(-256, 256) as i16))
    }

    /// 计算 Izhikevich 脑实例的确定性状态根（涵盖 `v_q8`、`u_q8` 与 `spike_ring`）
    pub fn state_root(&self) -> Blake3Hash {
        Blake3Hash::of(&self.to_neur_chunk(0).encode())
    }

    /// 导出为 NEUR 快照行（`membrane_v_q8` 存 `v_q8`，`eligibility[0]` 存恢复变量 `u_q8`，`spike_history_ring` 存脉冲环）
    pub fn to_neur_rows(&self, dense_index: u32) -> Vec<NeurNeuronRow> {
        (0..self.n)
            .map(|h| NeurNeuronRow {
                dense_index,
                neuron_index: h as u16,
                model_kind: NeuronModelKind::Izhikevich as u8,
                refractory_ticks: 0,
                spike_history_ring: self.spike_ring[h],
                membrane_v_q8: self.v_q8[h],
                weight_deltas: Vec::new(),
                eligibility: vec![self.u_q8[h]],
            })
            .collect()
    }

    /// 导出为独立的 NEUR 分块
    pub fn to_neur_chunk(&self, dense_index: u32) -> NeurChunk {
        NeurChunk {
            rows: self.to_neur_rows(dense_index),
        }
    }

    /// 从 NEUR 分块无损恢复 `v_q8`、`u_q8` 与 `spike_ring`
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk, dense_index: u32) -> Result<(), RynvekErrorCode> {
        let mut seen = vec![false; self.n];
        for row in chunk.rows.iter().filter(|r| r.dense_index == dense_index) {
            let h = row.neuron_index as usize;
            if h >= self.n
                || row.model_kind != NeuronModelKind::Izhikevich as u8
                || !row.weight_deltas.is_empty()
                || row.eligibility.len() != 1
            {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
            self.v_q8[h] = row.membrane_v_q8;
            self.u_q8[h] = row.eligibility[0];
            self.spike_ring[h] = row.spike_history_ring;
            seen[h] = true;
        }
        if seen.iter().all(|&s| s) {
            Ok(())
        } else {
            Err(RynvekErrorCode::DimensionMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// CTRNN
// ---------------------------------------------------------------------------

/// CTRNN 状态钳制上界（±8.0）
pub const CTRNN_STATE_LIMIT_Q8: i32 = 8 * 256;

/// 连续时间循环神经网络：`τ·dy/dt = -y + W·σ(y + θ) + I`，前向欧拉 `Δt = 1 Tick`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtrnnBrain {
    pub n: usize,
    pub y_q8: Vec<i16>,
    /// `1/τ`（Q8.8），τ ∈ [1, 256] Tick
    pub inv_tau_q8: Vec<i16>,
    pub bias_q8: Vec<i16>,
    /// `n × 16`
    pub w_in: Vec<i16>,
    pub w_rec: NeuralGraphStorage,
    /// `6 × n`
    pub w_out: Vec<i16>,
}

impl CtrnnBrain {
    pub fn new(
        tau_ticks: &[u16],
        bias_q8: Vec<i16>,
        w_in: Vec<i16>,
        w_rec: NeuralGraphStorage,
        w_out: Vec<i16>,
    ) -> Result<Self, RynvekErrorCode> {
        let n = w_rec.neuron_count();
        check_len(tau_ticks.len(), n)?;
        check_len(bias_q8.len(), n)?;
        check_len(w_in.len(), n * MODEL_INPUTS)?;
        check_len(w_out.len(), MODEL_OUTPUTS * n)?;
        if tau_ticks.iter().any(|&t| t == 0 || t > 256) {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        Ok(Self {
            n,
            y_q8: vec![0; n],
            inv_tau_q8: tau_ticks.iter().map(|&t| (256 / t as i32) as i16).collect(),
            bias_q8,
            w_in,
            w_rec,
            w_out,
        })
    }

    fn activations(&self) -> Vec<i16> {
        self.y_q8
            .iter()
            .zip(&self.bias_q8)
            .map(|(&y, &b)| hard_sigmoid_q8(y as i32 + b as i32) as i16)
            .collect()
    }

    pub fn step(&mut self, sensory: &[Q8_8; MODEL_INPUTS]) -> [Q8_8; MODEL_OUTPUTS] {
        let sigma = self.activations();
        let mut drive = input_projection(&self.w_in, self.n, sensory);
        self.w_rec.matvec_accumulate_q8(&sigma, &mut drive);
        for (j, &d) in drive.iter().enumerate() {
            let y = self.y_q8[j] as i32;
            let dy = -y + d;
            let next = y + ((dy * self.inv_tau_q8[j] as i32) >> 8);
            self.y_q8[j] = next.clamp(-CTRNN_STATE_LIMIT_Q8, CTRNN_STATE_LIMIT_Q8) as i16;
        }
        let sigma = self.activations();
        let mut out = [Q8_8::ZERO; MODEL_OUTPUTS];
        for (k, o) in out.iter_mut().enumerate() {
            let acc: i32 = self.w_out[k * self.n..(k + 1) * self.n]
                .iter()
                .zip(&sigma)
                .map(|(&w, &s)| syn_term(w, s))
                .sum();
            *o = Q8_8(hard_tanh_q8(acc) as i16);
        }
        out
    }

    /// 计算 CTRNN 脑实例的确定性状态根（涵盖全部神经元连续状态 `y_q8`）
    pub fn state_root(&self) -> Blake3Hash {
        Blake3Hash::of(&self.to_neur_chunk(0).encode())
    }

    /// 导出为 NEUR 快照行（`membrane_v_q8` 存连续状态 `y_q8`）
    pub fn to_neur_rows(&self, dense_index: u32) -> Vec<NeurNeuronRow> {
        (0..self.n)
            .map(|h| NeurNeuronRow {
                dense_index,
                neuron_index: h as u16,
                model_kind: NeuronModelKind::CTRNN as u8,
                refractory_ticks: 0,
                spike_history_ring: 0,
                membrane_v_q8: self.y_q8[h],
                weight_deltas: Vec::new(),
                eligibility: Vec::new(),
            })
            .collect()
    }

    /// 导出为独立的 NEUR 分块
    pub fn to_neur_chunk(&self, dense_index: u32) -> NeurChunk {
        NeurChunk {
            rows: self.to_neur_rows(dense_index),
        }
    }

    /// 从 NEUR 分块恢复连续状态 `y_q8`
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk, dense_index: u32) -> Result<(), RynvekErrorCode> {
        let mut seen = vec![false; self.n];
        for row in chunk.rows.iter().filter(|r| r.dense_index == dense_index) {
            let h = row.neuron_index as usize;
            if h >= self.n
                || row.model_kind != NeuronModelKind::CTRNN as u8
                || !row.weight_deltas.is_empty()
                || !row.eligibility.is_empty()
            {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
            self.y_q8[h] = row.membrane_v_q8;
            seen[h] = true;
        }
        if seen.iter().all(|&s| s) {
            Ok(())
        } else {
            Err(RynvekErrorCode::DimensionMismatch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::TopologyStorageClass;

    fn sensory_const(v: i16) -> [Q8_8; MODEL_INPUTS] {
        [Q8_8(v); MODEL_INPUTS]
    }

    fn ring_matrix(n: usize, w: i16) -> Vec<i16> {
        let mut m = vec![0i16; n * n];
        for post in 0..n {
            m[post * n + (post + 1) % n] = w;
        }
        m
    }

    #[test]
    fn gated_mlp_matches_hand_computed_value() {
        // 1 个隐藏单元：pre = 0.5（x=1.0 × w=0.5），gate = σ(0) = 0.5 → h = 0.25；y0 = h × 2.0 = 0.5
        let mut w_in = vec![0i16; 16];
        w_in[0] = 128;
        let mut brain = GatedMlpBrain::new(1, w_in, vec![0; 16], vec![0], vec![512, 0, 0, 0, 0, 0]).unwrap();
        let mut x = [Q8_8::ZERO; 16];
        x[0] = Q8_8::ONE;
        let y = brain.step(&x);
        assert_eq!(brain.hidden_q8[0], 64);
        assert_eq!(y[0], Q8_8(128));
    }

    #[test]
    fn izhikevich_spikes_under_drive_and_rests_without() {
        let n = 8;
        let build = || {
            let w_rec = NeuralGraphStorage::build(TopologyStorageClass::DenseTiledMatrix, n, &vec![0; n * n]).unwrap();
            IzhikevichBrain::new(
                IzhikevichParams::REGULAR_SPIKING,
                vec![64; n * 16],
                w_rec,
                vec![32; 6 * n],
            )
            .unwrap()
        };
        let mut silent = build();
        let mut driven = build();
        let mut silent_spikes = 0;
        let mut driven_spikes = 0;
        for _ in 0..300 {
            silent.step(&sensory_const(0));
            driven.step(&sensory_const(256));
            silent_spikes += silent.spike_ring.iter().filter(|r| *r & 1 == 1).count();
            driven_spikes += driven.spike_ring.iter().filter(|r| *r & 1 == 1).count();
        }
        assert_eq!(silent_spikes, 0, "零输入不应发放");
        assert!(driven_spikes > 0, "强驱动应产生规则发放");
    }

    #[test]
    fn ctrnn_is_layout_invariant_and_deterministic() {
        let n = 24;
        let rec = ring_matrix(n, 200);
        let tau: Vec<u16> = (0..n).map(|j| 4 + (j as u16 % 9)).collect();
        let run = |class| {
            let w_rec = NeuralGraphStorage::build(class, n, &rec).unwrap();
            let mut brain = CtrnnBrain::new(&tau, vec![-64; n], vec![40; n * 16], w_rec, vec![16; 6 * n]).unwrap();
            let mut trace = Vec::new();
            for t in 0..200i16 {
                trace.push(brain.step(&sensory_const((t % 17) * 15 - 128)));
            }
            (trace, brain.y_q8)
        };
        let dense = run(TopologyStorageClass::DenseTiledMatrix);
        assert_eq!(dense, run(TopologyStorageClass::BlockSparseCSR));
        assert_eq!(dense, run(TopologyStorageClass::PagedDynamicAdjacency));
    }

    #[test]
    fn neur_snapshot_roundtrip_preserves_all_three_models() {
        let n = 8;
        let rec = ring_matrix(n, 160);
        let w_rec = NeuralGraphStorage::build(TopologyStorageClass::DenseTiledMatrix, n, &rec).unwrap();
        let mut mlp = GatedMlpBrain::new(n, vec![32; n * 16], vec![16; n * 16], vec![64; n], vec![24; 6 * n]).unwrap();
        let mut izh = IzhikevichBrain::new(
            IzhikevichParams::REGULAR_SPIKING,
            vec![64; n * 16],
            w_rec.clone(),
            vec![32; 6 * n],
        )
        .unwrap();
        let tau: Vec<u16> = (0..n).map(|j| 4 + j as u16).collect();
        let mut ctrnn = CtrnnBrain::new(&tau, vec![-32; n], vec![40; n * 16], w_rec.clone(), vec![16; 6 * n]).unwrap();

        for t in 0..25i16 {
            let s = sensory_const(t * 9 - 80);
            mlp.step(&s);
            izh.step(&s);
            ctrnn.step(&s);
        }

        let mut rows = Vec::new();
        rows.extend(mlp.to_neur_rows(10));
        rows.extend(izh.to_neur_rows(20));
        rows.extend(ctrnn.to_neur_rows(30));
        let encoded = NeurChunk { rows }.encode();
        let decoded = NeurChunk::decode(&encoded).unwrap();

        let mut mlp_restored =
            GatedMlpBrain::new(n, vec![32; n * 16], vec![16; n * 16], vec![64; n], vec![24; 6 * n]).unwrap();
        let mut izh_restored = IzhikevichBrain::new(
            IzhikevichParams::REGULAR_SPIKING,
            vec![64; n * 16],
            w_rec.clone(),
            vec![32; 6 * n],
        )
        .unwrap();
        let mut ctrnn_restored = CtrnnBrain::new(&tau, vec![-32; n], vec![40; n * 16], w_rec, vec![16; 6 * n]).unwrap();

        mlp_restored.restore_from_neur(&decoded, 10).unwrap();
        izh_restored.restore_from_neur(&decoded, 20).unwrap();
        ctrnn_restored.restore_from_neur(&decoded, 30).unwrap();

        assert_eq!(mlp_restored.state_root(), mlp.state_root());
        assert_eq!(izh_restored.state_root(), izh.state_root());
        assert_eq!(ctrnn_restored.state_root(), ctrnn.state_root());

        for t in 25..50i16 {
            let s = sensory_const(t * 7 - 100);
            assert_eq!(mlp_restored.step(&s), mlp.step(&s));
            assert_eq!(izh_restored.step(&s), izh.step(&s));
            assert_eq!(ctrnn_restored.step(&s), ctrnn.step(&s));
        }
    }

    #[test]
    fn constructors_reject_dimension_mismatch() {
        assert_eq!(
            GatedMlpBrain::new(2, vec![0; 16], vec![0; 32], vec![0; 2], vec![0; 12]),
            Err(RynvekErrorCode::DimensionMismatch)
        );
    }
}
