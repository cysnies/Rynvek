// crates/rynvek-wgpu/shaders/compute/lif_step.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// 规范 05 `step_lif_brain_deterministic` 的 GPU 版本（批量脑实例），与 rynvek-neural 逐位一致。
//
// 并行分解：每个调用处理一个 (脑, 隐藏神经元)；神经元之间的状态彼此独立，
// 唯一的跨神经元归约（6 维动作读出）由第二个内核 `lif_readout` 按脑完成——
// 发放与否可由脉冲环最低位恢复（不应期分支左移后最低位必为 0），整数求和与顺序无关，故逐位一致。
// 线性 Workgroup 128×1×1。所有 Q8.8 / i16 量以 i32 承载，u64 脉冲环以 vec2<u32>(lo, hi) 承载。

struct LifParams {
    brain_count: u32,
    n_hid: u32,
    plasticity_enabled: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: LifParams;
// 每脑 16 维感知输入
@group(1) @binding(0) var<storage, read> sensory_q8: array<i32>;
// 每脑 n_hid*16 + n_hid*6 个基础权重（weights_delta 同布局；eligibility 每脑 n_hid*16）
@group(1) @binding(1) var<storage, read> weights_base_q8: array<i32>;
// 每脑 4 个调质 [DA, 5-HT, NE, ACh]
@group(1) @binding(2) var<storage, read> modulators_q8: array<i32>;
@group(2) @binding(0) var<storage, read_write> membrane_v_q8: array<i32>;
@group(2) @binding(1) var<storage, read_write> refractory_ticks: array<u32>;
@group(2) @binding(2) var<storage, read_write> spike_ring: array<vec2<u32>>;
@group(2) @binding(3) var<storage, read_write> weights_delta_q8: array<i32>;
@group(2) @binding(4) var<storage, read_write> eligibility_q8: array<i32>;

// 复现 Rust `(v as i16)` 的截断回绕（base_w = -32768 时 -base_w 回绕为 -32768）
fn wrap_i16(v: i32) -> i32 {
    return (v << 16u) >> 16u;
}

fn ring_shift(r: vec2<u32>, bit: u32) -> vec2<u32> {
    return vec2<u32>((r.x << 1u) | bit, (r.y << 1u) | (r.x >> 31u));
}

@compute @workgroup_size(128, 1, 1)
fn lif_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n_hid = params.n_hid;
    let total = params.brain_count * n_hid;
    let g = gid.x;
    if (g >= total) {
        return;
    }
    let b = g / n_hid;
    let h = g - b * n_hid;
    let w_per_brain = n_hid * 16u + n_hid * 6u;
    let w_base = b * w_per_brain;
    let s_base = b * 16u;

    let m_da = modulators_q8[b * 4u + 0u];
    let m_ne = modulators_q8[b * 4u + 2u];
    // 去甲肾上腺素动态调节阈值：theta_eff = 256 * (1 - 0.30 * m_NE / 256)
    let theta_eff = 256 - ((77 * m_ne) >> 8u);
    let leak_beta = 230; // ~0.898 in Q8.8

    if (refractory_ticks[g] > 0u) {
        refractory_ticks[g] = refractory_ticks[g] - 1u;
        membrane_v_q8[g] = 0;
        spike_ring[g] = ring_shift(spike_ring[g], 0u);
        return;
    }

    var syn_current = 0;
    let row_off = h * 16u;
    for (var i = 0u; i < 16u; i = i + 1u) {
        let w_eff = clamp(weights_base_q8[w_base + row_off + i] + weights_delta_q8[w_base + row_off + i], -32767, 32767);
        syn_current = syn_current + ((rv_checked_q8(sensory_q8[s_base + i]) * w_eff) >> 8u);
    }

    let v_prev = membrane_v_q8[g];
    let v_next = ((v_prev * leak_beta) >> 8u) + syn_current;
    let spiked = v_next >= theta_eff;
    spike_ring[g] = ring_shift(spike_ring[g], select(0u, 1u, spiked));

    if (spiked) {
        membrane_v_q8[g] = 0;
        refractory_ticks[g] = 2u;
    } else {
        membrane_v_q8[g] = clamp(v_next, -512, 512);
    }

    // 四因子调制可塑性更新（遵守 Dale 定律：兴奋/抑制符号锁死不可翻转）
    if (params.plasticity_enabled != 0u) {
        for (var i = 0u; i < 16u; i = i + 1u) {
            let idx = w_base + row_off + i;
            // 资格迹每脑只有 n_hid*16 项（与 CPU `eligibility_q8[row_off + i]` 对应）
            let e_idx = b * n_hid * 16u + row_off + i;
            let e_prev = eligibility_q8[e_idx];
            let pre_act = abs(sensory_q8[s_base + i]);
            var stdp_term = -((pre_act * 4) >> 8u);
            if (spiked) {
                stdp_term = (pre_act * 15) >> 8u;
            }
            let e_next = clamp(((e_prev * 243) >> 8u) + stdp_term, -512, 512);
            eligibility_q8[e_idx] = e_next;

            let dw = clamp((m_da * e_next) >> 11u, -8, 8);
            let base_w = weights_base_q8[idx];
            let new_delta = clamp(weights_delta_q8[idx] + dw, -128, 128);
            if (base_w >= 0 && (base_w + new_delta) < 0) {
                weights_delta_q8[idx] = wrap_i16(-base_w);
            } else if (base_w < 0 && (base_w + new_delta) > 0) {
                weights_delta_q8[idx] = wrap_i16(-base_w);
            } else {
                weights_delta_q8[idx] = new_delta;
            }
        }
    }
}
