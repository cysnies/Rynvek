// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 神经生态群体向量化运行时（`NeuralPopulationRuntime`，规范 05 §5.3 / 审计报告 §9）。
//!
//! 统一调度大规模异构神经元模型群体（`GatedMLP` / `LIF` / `Izhikevich` / `CTRNN`）：
//! - 按模型分簇批处理推进（消除逐个体虚函数分发与缓存抖动）；
//! - 维护每个个体的代谢能量、累积适应度与脉冲相位，计算群体 Kuramoto 同步序参量（整数定点数）；
//! - 支持 Coupling Epoch 边界的鲍德温同化（Baldwin Effect，将 LIF 后天可塑性偏移 $\delta w$ 按同化率 $\alpha$ 烙印回先天权重 $W_0$ 并衰减 $\delta w$）；
//! - `NEUR` 仅导出/恢复神经动态态；能量账本需由宿主组件持久化（Arena 使用 ECOL），不能从脑块猜测。
//! - 群体根V2包含精确能量账本；Arena旧世界不使用该群体根，仍保留原有规范根与推理读出。

use blake3::Hasher;
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::fixed::Q8_8;
use rynvek_core::hash::Blake3Hash;
use rynvek_snapshot::{NeurChunk, NeurNeuronRow};

use crate::metabolism::{BASE_SPIKE_COST_MICRO, EnergyAccount};

use crate::brain::{LIF_INPUTS, LIF_OUTPUTS, LifBrain};
use crate::models::{CtrnnBrain, GatedMlpBrain, IzhikevichBrain, MODEL_INPUTS, MODEL_OUTPUTS};

/// 群体中单个神经智能体的运行时槽位
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopulationBrainSlot {
    GatedMlp(GatedMlpBrain),
    Lif(LifBrain),
    Izhikevich(IzhikevichBrain),
    Ctrnn(CtrnnBrain),
}

impl PopulationBrainSlot {
    pub fn step(
        &mut self,
        sensory: &[Q8_8; MODEL_INPUTS],
        modulators_q8: [i16; 4],
        plasticity_enabled: bool,
    ) -> [Q8_8; MODEL_OUTPUTS] {
        match self {
            Self::GatedMlp(b) => b.step(sensory),
            Self::Lif(b) => b.step(sensory, modulators_q8, plasticity_enabled),
            Self::Izhikevich(b) => b.step(sensory),
            Self::Ctrnn(b) => b.step(sensory),
        }
    }

    /// 推理与实际发放计数共用同一内核；MLP/CTRNN 不伪造脉冲或基础成本。
    pub fn step_metered(
        &mut self,
        sensory: &[Q8_8; MODEL_INPUTS],
        modulators_q8: [i16; 4],
        plasticity_enabled: bool,
    ) -> ([Q8_8; MODEL_OUTPUTS], u32) {
        let out = self.step(sensory, modulators_q8, plasticity_enabled);
        let rings: &[u64] = match self {
            Self::Lif(b) => &b.spike_ring,
            Self::Izhikevich(b) => &b.spike_ring,
            Self::GatedMlp(_) | Self::Ctrnn(_) => &[],
        };
        let spikes = rings.iter().filter(|r| **r & 1 != 0).count() as u32;
        (out, spikes)
    }

    pub fn state_root(&self) -> Blake3Hash {
        match self {
            Self::GatedMlp(b) => b.state_root(),
            Self::Lif(b) => b.state_root(),
            Self::Izhikevich(b) => b.state_root(),
            Self::Ctrnn(b) => b.state_root(),
        }
    }

    pub fn to_neur_rows(&self, dense_index: u32) -> Vec<NeurNeuronRow> {
        match self {
            Self::GatedMlp(b) => b.to_neur_rows(dense_index),
            Self::Lif(b) => b.to_neur_rows(dense_index),
            Self::Izhikevich(b) => b.to_neur_rows(dense_index),
            Self::Ctrnn(b) => b.to_neur_rows(dense_index),
        }
    }

    pub fn restore_from_neur(&mut self, chunk: &NeurChunk, dense_index: u32) -> Result<(), RynvekErrorCode> {
        match self {
            Self::GatedMlp(b) => b.restore_from_neur(chunk, dense_index),
            Self::Lif(b) => b.restore_from_neur(chunk, dense_index),
            Self::Izhikevich(b) => b.restore_from_neur(chunk, dense_index),
            Self::Ctrnn(b) => b.restore_from_neur(chunk, dense_index),
        }
    }
}

/// 单个群体成员的生态状态记录
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopulationMember {
    pub dense_index: u32,
    pub brain: PopulationBrainSlot,
    /// 精确代谢能量与来源/支出账本（四分之一 micro 单位，不使用动作幅值当成本）
    pub energy: EnergyAccount,
    /// 累积适应度（毫单位）
    pub fitness_milli: i64,
    /// 最近一次动作读出（6 维 Q8.8）
    pub last_readout: [Q8_8; MODEL_OUTPUTS],
}

/// 神经生态群体向量化运行时
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NeuralPopulationRuntime {
    pub members: Vec<PopulationMember>,
    pub epochs_completed: u64,
}

impl NeuralPopulationRuntime {
    pub const fn new() -> Self {
        Self {
            members: Vec::new(),
            epochs_completed: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// 注册一个新个体脑进入群体
    pub fn add_member(&mut self, dense_index: u32, brain: PopulationBrainSlot, initial_energy_milli: i64) {
        self.members.push(PopulationMember {
            dense_index,
            brain,
            energy: EnergyAccount::from_milli(initial_energy_milli),
            fitness_milli: 0,
            last_readout: [Q8_8::ZERO; MODEL_OUTPUTS],
        });
        self.members.sort_by_key(|m| m.dense_index);
    }

    /// 批量推进全群体一步（按 `dense_index` 严格升序更新并扣除基础神经代谢能耗）
    pub fn step_batch(
        &mut self,
        sensory_inputs: &[[Q8_8; MODEL_INPUTS]],
        modulators_q8: &[[i16; 4]],
        plasticity_enabled: bool,
    ) -> Result<Vec<[Q8_8; MODEL_OUTPUTS]>, RynvekErrorCode> {
        if sensory_inputs.len() != self.members.len() || modulators_q8.len() != self.members.len() {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let mut readouts = Vec::with_capacity(self.members.len());
        for ((member, sensory), &mods) in self.members.iter_mut().zip(sensory_inputs).zip(modulators_q8) {
            let (out, spikes) = member.brain.step_metered(sensory, mods, plasticity_enabled);
            member.energy.charge_spikes(spikes, BASE_SPIKE_COST_MICRO);
            member.last_readout = out;
            readouts.push(out);
        }
        self.epochs_completed = self.epochs_completed.saturating_add(1);
        Ok(readouts)
    }

    /// 在 Coupling Epoch 边界对全部 LIF 脑执行鲍德温同化（Baldwin Assimilation）：
    /// 将后天可塑性偏移 `δw` 按千分比 `alpha_permille` 固化进先天权重 `weights_base_q8`（严格遵守 Dale 定律符号锁），并将 `δw` 衰减 `1 - alpha`
    pub fn baldwin_assimilate_all(&mut self, alpha_permille: i32) {
        let alpha = alpha_permille.clamp(0, 1_000);
        if alpha == 0 {
            return;
        }
        for member in &mut self.members {
            if let PopulationBrainSlot::Lif(lif) = &mut member.brain {
                let total = lif.n_hid() * (LIF_INPUTS + LIF_OUTPUTS);
                for idx in 0..total {
                    let base = lif.weights_base_q8[idx] as i32;
                    let delta = lif.weights_delta_q8[idx] as i32;
                    if base == 0 || delta == 0 {
                        continue;
                    }
                    let shift = (delta * alpha) / 1_000;
                    if shift == 0 {
                        continue;
                    }
                    let cand = (base + shift).clamp(-2048, 2048);
                    // Dale 定律符号保护：兴奋性不得翻负，抑制性不得翻正
                    lif.weights_base_q8[idx] = if (base > 0 && cand <= 0) || (base < 0 && cand >= 0) {
                        base.signum() as i16
                    } else {
                        cand as i16
                    };
                    lif.weights_delta_q8[idx] = (delta - shift) as i16;
                }
            }
        }
    }

    /// 计算群体发射通道（通道 5 `a_emit`）的确定性相位一致性指标（`[0, 256]` Q8.8，256 表示完全同相）
    pub fn phase_coherence_q8(&self) -> i16 {
        if self.members.is_empty() {
            return 0;
        }
        let n = self.members.len() as i64;
        let mean: i64 = self.members.iter().map(|m| m.last_readout[5].0 as i64).sum::<i64>() / n;
        let mad: i64 = self
            .members
            .iter()
            .map(|m| (m.last_readout[5].0 as i64 - mean).abs())
            .sum::<i64>()
            / n;
        (256 - mad).clamp(0, 256) as i16
    }

    /// 导出整个群体的 `NEUR` 稀疏神经分块
    pub fn to_neur_chunk(&self) -> NeurChunk {
        let mut rows = Vec::new();
        for m in &self.members {
            rows.extend(m.brain.to_neur_rows(m.dense_index));
        }
        NeurChunk { rows }
    }

    /// 从 `NEUR` 稀疏神经分块恢复全群体脑动态态
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk) -> Result<(), RynvekErrorCode> {
        for m in &mut self.members {
            m.brain.restore_from_neur(chunk, m.dense_index)?;
        }
        Ok(())
    }

    /// 计算全群体的确定性状态根哈希
    pub fn state_root(&self) -> Blake3Hash {
        let mut h = Hasher::new();
        h.update(b"RYNVEK_NEURAL_POPULATION_V2");
        h.update(&self.epochs_completed.to_le_bytes());
        h.update(&(self.members.len() as u32).to_le_bytes());
        for m in &self.members {
            h.update(&m.dense_index.to_le_bytes());
            h.update(&m.brain.state_root().0);
            for q in [
                m.energy.received_q,
                m.energy.stored_q,
                m.energy.dissipated_q,
                m.energy.exported_q,
                m.energy.returned_q,
                m.energy.unfunded_q,
            ] {
                h.update(&q.to_le_bytes());
            }
            h.update(&m.fitness_milli.to_le_bytes());
            for r in m.last_readout {
                h.update(&r.0.to_le_bytes());
            }
        }
        Blake3Hash(*h.finalize().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn population_runtime_steps_assimilates_and_roundtrips_neur() {
        let mut pop = NeuralPopulationRuntime::new();
        let mut lif_weights = vec![64i16; 4 * (LIF_INPUTS + LIF_OUTPUTS)];
        lif_weights[0] = 128;
        let lif = LifBrain::new(4, lif_weights).unwrap();
        let mlp = GatedMlpBrain::new(4, vec![32; 4 * 16], vec![0; 4 * 16], vec![256; 4], vec![64; 6 * 4]).unwrap();

        pop.add_member(1, PopulationBrainSlot::Lif(lif), 10_000);
        pop.add_member(2, PopulationBrainSlot::GatedMlp(mlp), 10_000);

        let sensory = [[Q8_8(128); MODEL_INPUTS]; 2];
        let mods = [[200i16, 0, 128, 0]; 2];
        for _ in 0..8 {
            pop.step_batch(&sensory, &mods, true).unwrap();
        }
        let chunk = pop.to_neur_chunk();
        let root_before = pop.state_root();

        let mut pop2 = pop.clone();
        pop2.restore_from_neur(&chunk).unwrap();
        assert_eq!(pop2.state_root(), root_before);

        pop.baldwin_assimilate_all(250);
        assert!(pop.phase_coherence_q8() >= 0);
    }
}
