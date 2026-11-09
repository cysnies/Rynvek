// crates/rynvek-neural/src/runtime.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三层神经对象模型、三种图存储布局与四因子 STDP 完整实现（规范 05 §5.1，源码逐字冻结）。

use rynvek_core::fixed::Q8_8;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum NeuronModelKind {
    GatedMLP = 0,
    LIF = 1,
    Izhikevich = 2,
    CTRNN = 3,
}

impl NeuronModelKind {
    /// 由 `repr(u8)` 值还原（用于 NEUR 快照行）
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::GatedMLP),
            1 => Some(Self::LIF),
            2 => Some(Self::Izhikevich),
            3 => Some(Self::CTRNN),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum TopologyStorageClass {
    /// 单脑总神经元 N <= 64：采用 16x16 分块稠密权重矩阵
    DenseTiledMatrix = 0,
    /// 64 < N <= 4096 且仅在 Epoch 边界改变拓扑：采用 8x8 块稀疏压缩行
    BlockSparseCSR = 1,
    /// 支持运行期高频突触增删：每神经元固定预分配 32 个突触槽位页
    PagedDynamicAdjacency = 2,
}

/// LIF 漏电系数 β（Q8.8，≈0.898），与 `step_lif_brain_deterministic` 内常量一致
pub const LIF_LEAK_BETA_Q8: i32 = 230;
/// 脉冲后不应期 Tick 数，与 `step_lif_brain_deterministic` 内常量一致
pub const LIF_REFRACTORY_TICKS: u8 = 2;

/// 去甲肾上腺素调制后的有效阈值 `theta_eff = 256 - ((77 * m_NE) >> 8)`
///
/// 与 `step_lif_brain_deterministic` 内联公式逐位一致，供 Golden Vector（GV-004）与诊断复用。
#[inline]
pub const fn lif_theta_eff_q8(m_ne_q8: i32) -> i32 {
    256 - ((77 * m_ne_q8) >> 8)
}

/// 单神经元膜电位候选值 `v_next = ((v_prev * β) >> 8) + I_syn`
#[inline]
pub const fn lif_membrane_candidate_q8(v_prev_q8: i32, leak_beta_q8: i32, syn_current_q8: i32) -> i32 {
    ((v_prev_q8 * leak_beta_q8) >> 8) + syn_current_q8
}

/// 确定性 Q8.8 LIF 神经元与四因子 STDP 更新核（CPU Reference 与 D0/D1 权威归约标准实现）
// 以下 clippy 豁免仅为保持规范冻结源码逐字不变（索引循环、八参数签名、Dale 分支同构）。
#[allow(clippy::too_many_arguments, clippy::needless_range_loop, clippy::if_same_then_else)]
pub fn step_lif_brain_deterministic(
    sensory_in_q8: &[Q8_8; 16],
    membrane_v_q8: &mut [Q8_8],
    refractory_ticks: &mut [u8],
    spike_ring: &mut [u64],
    weights_base_q8: &[i16],
    weights_delta_q8: &mut [i16],
    eligibility_q8: &mut [i16],
    modulators_q8: [i16; 4], // [DA, 5-HT, NE, ACh] in [-256, +256]
    plasticity_enabled: bool,
) -> [Q8_8; 6] {
    let n_hid = membrane_v_q8.len();
    let m_da = modulators_q8[0] as i32;
    let m_ne = modulators_q8[2] as i32;
    // 去甲肾上腺素动态调节阈值：theta_eff = 256 * (1 - 0.30 * m_NE / 256)
    let theta_eff: i32 = 256 - ((77 * m_ne) >> 8);
    let leak_beta: i32 = 230; // ~0.898 in Q8.8

    let mut out_acc = [0i32; 6];

    for h in 0..n_hid {
        if refractory_ticks[h] > 0 {
            refractory_ticks[h] -= 1;
            membrane_v_q8[h] = Q8_8::ZERO;
            spike_ring[h] <<= 1;
            continue;
        }

        let mut syn_current: i32 = 0;
        let row_off = h * 16;
        for i in 0..16 {
            let w_eff =
                (weights_base_q8[row_off + i] as i32 + weights_delta_q8[row_off + i] as i32).clamp(-32767, 32767);
            syn_current += ((sensory_in_q8[i].0 as i32) * w_eff) >> 8;
        }

        let v_prev = membrane_v_q8[h].0 as i32;
        let v_next = ((v_prev * leak_beta) >> 8) + syn_current;
        let spiked = v_next >= theta_eff;

        spike_ring[h] = (spike_ring[h] << 1) | (spiked as u64);

        if spiked {
            membrane_v_q8[h] = Q8_8::ZERO;
            refractory_ticks[h] = 2;
            let out_off = n_hid * 16 + h * 6;
            for k in 0..6 {
                let w_out =
                    (weights_base_q8[out_off + k] as i32 + weights_delta_q8[out_off + k] as i32).clamp(-32767, 32767);
                out_acc[k] += w_out;
            }
        } else {
            membrane_v_q8[h] = Q8_8(v_next.clamp(-512, 512) as i16);
        }

        // 四因子调制可塑性更新（遵守 Dale 定律：兴奋/抑制符号锁死不可翻转）
        if plasticity_enabled {
            for i in 0..16 {
                let idx = row_off + i;
                let e_prev = eligibility_q8[idx] as i32;
                let pre_act = (sensory_in_q8[i].0 as i32).abs();
                let stdp_term = if spiked {
                    (pre_act * 15) >> 8
                } else {
                    -((pre_act * 4) >> 8)
                };
                let e_next = (((e_prev * 243) >> 8) + stdp_term).clamp(-512, 512);
                eligibility_q8[idx] = e_next as i16;

                let dw = ((m_da * e_next) >> 11).clamp(-8, 8);
                let base_w = weights_base_q8[idx] as i32;
                let new_delta = (weights_delta_q8[idx] as i32 + dw).clamp(-128, 128);
                // Dale's Law 符号保护
                if base_w >= 0 && (base_w + new_delta) < 0 {
                    weights_delta_q8[idx] = (-base_w) as i16;
                } else if base_w < 0 && (base_w + new_delta) > 0 {
                    weights_delta_q8[idx] = (-base_w) as i16;
                } else {
                    weights_delta_q8[idx] = new_delta as i16;
                }
            }
        }
    }

    let mut readout = [Q8_8::ZERO; 6];
    for k in 0..6 {
        readout[k] = Q8_8((out_acc[k] >> 2).clamp(-256, 256) as i16);
    }
    readout
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GV-004：v_prev=180, β=230, I=96, m_NE=128 → θ=218, v=257, 发放并复位、不应期=2
    #[test]
    fn golden_vector_gv004_single_lif_step() {
        assert_eq!(lif_theta_eff_q8(128), 218);
        assert_eq!(lif_membrane_candidate_q8(180, LIF_LEAK_BETA_Q8, 96), 257);

        // 1 个隐藏神经元：输入 0 通道 96 × 权重 1.0 → I_syn = 96
        let mut sensory = [Q8_8::ZERO; 16];
        sensory[0] = Q8_8(96);
        let mut membrane = [Q8_8(180)];
        let mut refractory = [0u8];
        let mut ring = [0u64];
        let mut base = vec![0i16; 16 + 6];
        base[0] = 256;
        base[16] = 40;
        let mut delta = vec![0i16; 22];
        let mut elig = vec![0i16; 16];
        let readout = step_lif_brain_deterministic(
            &sensory,
            &mut membrane,
            &mut refractory,
            &mut ring,
            &base,
            &mut delta,
            &mut elig,
            [0, 0, 128, 0],
            false,
        );
        assert_eq!(membrane[0], Q8_8::ZERO);
        assert_eq!(refractory[0], LIF_REFRACTORY_TICKS);
        assert_eq!(ring[0], 1);
        assert_eq!(readout[0], Q8_8(10)); // 40 >> 2
    }

    #[test]
    fn dale_law_keeps_excitatory_weight_non_negative() {
        let mut sensory = [Q8_8::ZERO; 16];
        sensory[0] = Q8_8(256);
        let mut membrane = [Q8_8::ZERO];
        let mut refractory = [0u8];
        let mut ring = [0u64];
        // 兴奋性小权重 + 正多巴胺 × 负资格迹 + 不发放（阈值抬高）→ δw 持续为负，但 w_eff 不得 < 0
        let mut base = vec![0i16; 22];
        base[0] = 3;
        let mut delta = vec![0i16; 22];
        let mut elig = vec![0i16; 16];
        for _ in 0..200 {
            step_lif_brain_deterministic(
                &sensory,
                &mut membrane,
                &mut refractory,
                &mut ring,
                &base,
                &mut delta,
                &mut elig,
                [256, 0, -256, 0],
                true,
            );
            assert!(base[0] as i32 + delta[0] as i32 >= 0, "Dale 定律被破坏");
        }
    }
}
