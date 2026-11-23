// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 单位神经脑：由 [`Genome`] 表达为 `rynvek-neural` 的四种确定性 Q8.8 模型之一，并在每个
//! Coupling Epoch（4 Ticks）向宏观层提交 6 维动作读出（GAP-03 双层混合权威执行契约）。
//!
//! **L3 四维调质 `m = [DA, 5-HT, NE, ACh]` 的作用（ERRATA E-025）**：
//! - LIF 脑直接以 `modulators_q8` 传入冻结核（NE 降低发放阈值、DA 门控四因子 STDP）；
//! - 所有模型的感知通道 14 / 15 注入 DA / NE，使非脉冲模型同样感受唤醒与奖赏信号；
//! - ACh（注意）为感知增益：通道 0..=13 乘以 `1 + 0.5·ACh`；
//! - 5-HT（抑制 / 耐心）在读出端压低攻击倾向：`a_attack −= 0.5·max(5-HT, 0)`。
//!
//! 宏观 D0 层每个 Epoch 只推进脑 [`BRAIN_STEPS_PER_EPOCH`] 步（而非 Neural Soup 的 120 Hz × 4 子步），
//! 读出取各步算术平均；这样单位脑的推理代价与 Tick 线性相关，且结果只依赖整数运算。

use rynvek_core::{Blake3Hash, Q8_8, RynvekErrorCode};
use rynvek_neural::models::IzhikevichParams;
use rynvek_neural::{
    CtrnnBrain, GatedMlpBrain, IzhikevichBrain, LifBrain, NeurChunk, NeurNeuronRow, NeuralGraphStorage, NeuronModelKind,
};

use crate::genome::{GENOME_INPUTS, GENOME_OUTPUTS, Genome, action, sensor};

/// 每个 Coupling Epoch 推进脑的步数
pub const BRAIN_STEPS_PER_EPOCH: usize = 4;

/// 四维调质向量（Q8.8，`[-256, 256]` 对应 `[-1, 1]`），顺序 `[DA, 5-HT, NE, ACh]`
pub type ModulatorsQ8 = [i16; 4];

/// 单位脑实例
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitBrain(pub rynvek_neural::PopulationBrainSlot);

impl UnitBrain {
    /// 按基因组表达为脑实例
    pub fn from_genome(g: &Genome) -> Result<Self, RynvekErrorCode> {
        let n = g.n_hid as usize;
        // 来自创作API的基因不可信；先验尺寸，避免LIF输出索引panic或巨额分配。
        if n == 0
            || n > rynvek_neural::storage::BLOCK_SPARSE_MAX_NEURONS
            || g.w_in.len() != n * GENOME_INPUTS
            || g.w_out.len() != n * GENOME_OUTPUTS
            || g.w_rec.len()
                != if matches!(g.model, NeuronModelKind::Izhikevich | NeuronModelKind::CTRNN) {
                    n * n
                } else {
                    0
                }
            || (g.model == NeuronModelKind::CTRNN && (g.tau_ticks.len() != n || g.tau_ticks.contains(&0)))
        {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        Ok(Self(match g.model {
            NeuronModelKind::GatedMLP => {
                // 门控全开：w_gate = 0、b_g = +2.0 → hard_sigmoid = 1.0，即标准两层前馈网络
                rynvek_neural::PopulationBrainSlot::GatedMlp(GatedMlpBrain::new(
                    n,
                    g.w_in.clone(),
                    vec![0; n * GENOME_INPUTS],
                    vec![512; n],
                    g.w_out.clone(),
                )?)
            }
            NeuronModelKind::LIF => {
                // 冻结核布局：[h*16 + i] 输入行，其后 [n*16 + h*6 + k] 输出
                let mut w = g.w_in.clone();
                w.resize(n * (GENOME_INPUTS + GENOME_OUTPUTS), 0);
                for h in 0..n {
                    for k in 0..GENOME_OUTPUTS {
                        w[n * GENOME_INPUTS + h * GENOME_OUTPUTS + k] = g.w_out[k * n + h];
                    }
                }
                rynvek_neural::PopulationBrainSlot::Lif(LifBrain::new(n, w)?)
            }
            NeuronModelKind::Izhikevich => rynvek_neural::PopulationBrainSlot::Izhikevich(IzhikevichBrain::new(
                IzhikevichParams::REGULAR_SPIKING,
                g.w_in.clone(),
                NeuralGraphStorage::build(g.storage, n, &g.w_rec)?,
                g.w_out.clone(),
            )?),
            NeuronModelKind::CTRNN => rynvek_neural::PopulationBrainSlot::Ctrnn(CtrnnBrain::new(
                &g.tau_ticks,
                vec![0; n],
                g.w_in.clone(),
                NeuralGraphStorage::build(g.storage, n, &g.w_rec)?,
                g.w_out.clone(),
            )?),
        }))
    }

    /// 推进一个 Coupling Epoch，返回平均动作读出（Q8.8 原始值）
    pub fn step_epoch(&mut self, sensory: &[i16; GENOME_INPUTS], m: ModulatorsQ8, plasticity: bool) -> [i16; 6] {
        self.step_epoch_metered(sensory, m, plasticity).0
    }

    /// 使用 NeuralPopulationRuntime 同一槽位核计数每个真实子步的发放，不能计读出幅值。
    pub fn step_epoch_metered(
        &mut self,
        sensory: &[i16; GENOME_INPUTS],
        m: ModulatorsQ8,
        plasticity: bool,
    ) -> ([i16; 6], u32) {
        let x = modulated_sensory(sensory, m);
        let mut spikes = 0;
        let mut acc = [0i32; GENOME_OUTPUTS];
        for _ in 0..BRAIN_STEPS_PER_EPOCH {
            let (y, n) = self.0.step_metered(&x, m, plasticity);
            spikes += n;
            for (a, v) in acc.iter_mut().zip(y) {
                *a += v.0 as i32;
            }
        }
        let mut out = acc.map(|a| (a / BRAIN_STEPS_PER_EPOCH as i32) as i16);
        // 5-HT 抑制攻击倾向
        let serotonin = m[1].max(0) as i32;
        out[action::ATTACK] = (out[action::ATTACK] as i32 - serotonin / 2).clamp(-256, 256) as i16;
        (out, spikes)
    }

    /// 以真实共享脑核计价候选Epoch，支付许可与脑/动作一起提交。
    /// 不足时膜电位、脉冲环、可塑性等全部不变；候选CPU评估不是免付费权威推理。
    pub fn step_epoch_budgeted(
        &mut self,
        sensory: &[i16; GENOME_INPUTS],
        m: ModulatorsQ8,
        plasticity: bool,
        account: &mut rynvek_neural::metabolism::EnergyAccount,
        cost_micro: u32,
        continuous_q: u64,
    ) -> Option<([i16; 6], u32)> {
        let mut candidate = self.clone();
        let (actions, spikes) = candidate.step_epoch_metered(sensory, m, plasticity);
        let Ok(total_q) = u64::try_from(u128::from(spikes) * u128::from(cost_micro) * 4 + u128::from(continuous_q))
        else {
            return None;
        };
        if !account.try_dissipate_q(total_q) {
            return None;
        }
        *self = candidate;
        Some((actions, spikes))
    }

    /// LIF 脑的后天可塑性偏移 δw（鲍德温同化的来源）；其他模型为 `None`
    pub fn plasticity_delta(&self) -> Option<&[i16]> {
        match &self.0 {
            rynvek_neural::PopulationBrainSlot::Lif(b) => Some(&b.weights_delta_q8),
            _ => None,
        }
    }

    /// 计算当前脑实例内部全部动态神经状态（含 `GatedMLP.hidden_q8`、`LIF` 膜电位/不应期/脉冲环/可塑性/资格迹、
    /// `Izhikevich` 膜电位/恢复变量/脉冲环、`CTRNN.y_q8`）的确定性状态根哈希
    pub fn state_root(&self) -> Blake3Hash {
        self.0.state_root()
    }

    /// 导出该单位脑的 `NEUR` 快照行
    pub fn to_neur_rows(&self, dense_index: u32) -> Vec<NeurNeuronRow> {
        self.0.to_neur_rows(dense_index)
    }

    /// 导出该单位脑的独立 `NEUR` 分块
    pub fn to_neur_chunk(&self, dense_index: u32) -> NeurChunk {
        NeurChunk {
            rows: self.to_neur_rows(dense_index),
        }
    }

    /// 从 `NEUR` 分块恢复单位脑内部动态状态
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk, dense_index: u32) -> Result<(), RynvekErrorCode> {
        self.0.restore_from_neur(chunk, dense_index)
    }
}

/// 把调质注入感知向量：通道 14 / 15 = DA / NE；ACh 放大通道 0..=13
pub fn modulated_sensory(sensory: &[i16; GENOME_INPUTS], m: ModulatorsQ8) -> [Q8_8; GENOME_INPUTS] {
    let gain = 256 + (m[3] as i32) / 2;
    let mut x = [Q8_8::ZERO; GENOME_INPUTS];
    for i in 0..GENOME_INPUTS {
        let v = match i {
            sensor::MOD_DA => m[0] as i32,
            sensor::MOD_NE => m[2] as i32,
            _ => (sensory[i] as i32 * gain) >> 8,
        };
        x[i] = Q8_8(v.clamp(-256, 256) as i16);
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::UnitKind;

    fn run(unit: UnitKind, set: &[(usize, i16)], m: ModulatorsQ8, epochs: usize) -> [i16; 6] {
        let mut brain = UnitBrain::from_genome(&Genome::seed(unit)).unwrap();
        let mut x = [0i16; GENOME_INPUTS];
        for (i, v) in set {
            x[*i] = *v;
        }
        let mut out = [0; 6];
        for _ in 0..epochs {
            out = brain.step_epoch(&x, m, false);
        }
        out
    }

    #[test]
    fn every_seed_genome_expresses_a_brain_and_roundtrips_neur() {
        for (idx, unit) in UnitKind::ALL.iter().enumerate() {
            let g = Genome::seed(*unit);
            let mut b = UnitBrain::from_genome(&g).unwrap();
            let y = b.step_epoch(&[64; 16], [128, 0, 64, 32], true);
            assert!(y.iter().all(|v| (-256..=256).contains(v)));
            let chunk = b.to_neur_chunk(idx as u32);
            let mut restored = UnitBrain::from_genome(&g).unwrap();
            restored.restore_from_neur(&chunk, idx as u32).unwrap();
            assert_eq!(restored.state_root(), b.state_root());
            assert_eq!(
                restored.step_epoch(&[32; 16], [0, 64, 0, 0], true),
                b.step_epoch(&[32; 16], [0, 64, 0, 0], true)
            );
        }
    }

    #[test]
    fn forager_climbs_nutrient_gradient_and_avoids_enemies() {
        let y = run(
            UnitKind::Forager,
            &[(sensor::NUTRIENT_GRAD_X, 200), (sensor::NUTRIENT, 128)],
            [0; 4],
            1,
        );
        assert!(y[action::MOVE_X] > 100, "{y:?}");
        assert!(y[action::HARVEST] > 100, "{y:?}");
        let y = run(UnitKind::Forager, &[(sensor::ENEMY_DIR_Y, 200)], [0; 4], 1);
        assert!(y[action::MOVE_Y] < -100, "{y:?}");
    }

    #[test]
    fn striker_fires_when_enemy_in_range_and_noradrenaline_lowers_threshold() {
        let y = run(UnitKind::Striker, &[(sensor::ENEMY_IN_RANGE, 256)], [0; 4], 3);
        assert!(y[action::ATTACK] > 0, "{y:?}");
        // 弱输入 I = 0.06 × 2.0：膜电位缓慢逼近阈值；NE = +1.0 把阈值从 1.0 降到 0.7，发放更早更频繁
        let total_attack = |m: ModulatorsQ8| {
            let mut brain = UnitBrain::from_genome(&Genome::seed(UnitKind::Striker)).unwrap();
            let mut x = [0i16; GENOME_INPUTS];
            x[sensor::ENEMY_IN_RANGE] = 15;
            (0..8)
                .map(|_| brain.step_epoch(&x, m, false)[action::ATTACK] as i32)
                .sum::<i32>()
        };
        let (quiet, aroused) = (total_attack([0; 4]), total_attack([0, 0, 256, 0]));
        assert!(aroused > quiet, "{quiet} vs {aroused}");
    }

    #[test]
    fn spitter_kites_away_from_close_enemies() {
        let y = run(UnitKind::Spitter, &[(sensor::ENEMY_DIR_X, 200)], [0; 4], 2);
        assert!(y[action::MOVE_X] < 0, "{y:?}");
    }

    #[test]
    fn serotonin_suppresses_attack() {
        let calm = run(UnitKind::Leviathan, &[(sensor::ENEMY_IN_RANGE, 256)], [0, 256, 0, 0], 3);
        let base = run(UnitKind::Leviathan, &[(sensor::ENEMY_IN_RANGE, 256)], [0; 4], 3);
        assert!(calm[action::ATTACK] < base[action::ATTACK]);
    }

    #[test]
    fn acetylcholine_amplifies_sensory_gain() {
        let mut x = [0i16; 16];
        x[0] = 100;
        assert_eq!(modulated_sensory(&x, [0, 0, 0, 256])[0], Q8_8(150));
        assert_eq!(modulated_sensory(&x, [0, 0, 0, -256])[0], Q8_8(50));
        assert_eq!(modulated_sensory(&x, [77, 0, -30, 0])[sensor::MOD_DA], Q8_8(77));
        assert_eq!(modulated_sensory(&x, [77, 0, -30, 0])[sensor::MOD_NE], Q8_8(-30));
    }
}

#[cfg(test)]
mod continuous_cost_tests {
    use super::*;
    use crate::UnitKind;
    use rynvek_neural::metabolism::EnergyAccount;
    #[test]
    fn continuous_cost_both_nonspiking_models_commit_only_after_full_payment() {
        for kind in [UnitKind::Forager, UnitKind::Carapace] {
            let mut brain = UnitBrain::from_genome(&Genome::seed(kind)).unwrap();
            let before = brain.state_root();
            let mut account = EnergyAccount::default();
            assert!(account.try_receive_milli(1));
            let original = account.clone();
            assert!(
                brain
                    .step_epoch_budgeted(&[256; 16], [0; 4], false, &mut account, 800, 4001)
                    .is_none()
            );
            assert_eq!(brain.state_root(), before);
            assert_eq!(account, original);
            let mut reference = brain.clone();
            let (actions, spikes) = reference.step_epoch_metered(&[256; 16], [0; 4], false);
            assert_eq!(spikes, 0, "连续输出不可伪装为脉冲");
            assert_eq!(
                brain.step_epoch_budgeted(&[256; 16], [0; 4], false, &mut account, 800, 4000),
                Some((actions, 0))
            );
            assert_eq!(brain.state_root(), reference.state_root());
            assert_eq!((account.stored_q, account.dissipated_q), (0, 4000));
            assert!(account.is_conserved());
        }
    }
}
