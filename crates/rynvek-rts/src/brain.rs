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

use rynvek_core::{Q8_8, RynvekErrorCode};
use rynvek_neural::models::IzhikevichParams;
use rynvek_neural::{CtrnnBrain, GatedMlpBrain, IzhikevichBrain, LifBrain, NeuralGraphStorage, NeuronModelKind};

use crate::genome::{GENOME_INPUTS, GENOME_OUTPUTS, Genome, action, sensor};

/// 每个 Coupling Epoch 推进脑的步数
pub const BRAIN_STEPS_PER_EPOCH: usize = 4;

/// 四维调质向量（Q8.8，`[-256, 256]` 对应 `[-1, 1]`），顺序 `[DA, 5-HT, NE, ACh]`
pub type ModulatorsQ8 = [i16; 4];

/// 单位脑实例
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitBrain {
    GatedMlp(GatedMlpBrain),
    Lif(LifBrain),
    Izhikevich(IzhikevichBrain),
    Ctrnn(CtrnnBrain),
}

impl UnitBrain {
    /// 按基因组表达为脑实例
    pub fn from_genome(g: &Genome) -> Result<Self, RynvekErrorCode> {
        let n = g.n_hid as usize;
        Ok(match g.model {
            NeuronModelKind::GatedMLP => {
                // 门控全开：w_gate = 0、b_g = +2.0 → hard_sigmoid = 1.0，即标准两层前馈网络
                Self::GatedMlp(GatedMlpBrain::new(
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
                Self::Lif(LifBrain::new(n, w)?)
            }
            NeuronModelKind::Izhikevich => Self::Izhikevich(IzhikevichBrain::new(
                IzhikevichParams::REGULAR_SPIKING,
                g.w_in.clone(),
                NeuralGraphStorage::build(g.storage, n, &g.w_rec)?,
                g.w_out.clone(),
            )?),
            NeuronModelKind::CTRNN => Self::Ctrnn(CtrnnBrain::new(
                &g.tau_ticks,
                vec![0; n],
                g.w_in.clone(),
                NeuralGraphStorage::build(g.storage, n, &g.w_rec)?,
                g.w_out.clone(),
            )?),
        })
    }

    /// 推进一个 Coupling Epoch，返回平均动作读出（Q8.8 原始值）
    pub fn step_epoch(&mut self, sensory: &[i16; GENOME_INPUTS], m: ModulatorsQ8, plasticity: bool) -> [i16; 6] {
        let x = modulated_sensory(sensory, m);
        let mut acc = [0i32; GENOME_OUTPUTS];
        for _ in 0..BRAIN_STEPS_PER_EPOCH {
            let y = match self {
                Self::GatedMlp(b) => b.step(&x),
                Self::Lif(b) => b.step(&x, m, plasticity),
                Self::Izhikevich(b) => b.step(&x),
                Self::Ctrnn(b) => b.step(&x),
            };
            for (a, v) in acc.iter_mut().zip(y) {
                *a += v.0 as i32;
            }
        }
        let mut out = acc.map(|a| (a / BRAIN_STEPS_PER_EPOCH as i32) as i16);
        // 5-HT 抑制攻击倾向
        let serotonin = m[1].max(0) as i32;
        out[action::ATTACK] = (out[action::ATTACK] as i32 - serotonin / 2).clamp(-256, 256) as i16;
        out
    }

    /// LIF 脑的后天可塑性偏移 δw（鲍德温同化的来源）；其他模型为 `None`
    pub fn plasticity_delta(&self) -> Option<&[i16]> {
        match self {
            Self::Lif(b) => Some(&b.weights_delta_q8),
            _ => None,
        }
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
    fn every_seed_genome_expresses_a_brain() {
        for unit in UnitKind::ALL {
            let mut b = UnitBrain::from_genome(&Genome::seed(*unit)).unwrap();
            let y = b.step_epoch(&[0; 16], [0; 4], false);
            assert!(y.iter().all(|v| (-256..=256).contains(v)));
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
