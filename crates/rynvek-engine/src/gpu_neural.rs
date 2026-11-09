// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `rynvek-neural::LifBrain` 与 GPU 批量 LIF 内核之间的打包 / 解包（同拓扑合批，ADR-013）。

use rynvek_core::fixed::Q8_8;
use rynvek_neural::{LIF_INPUTS, LIF_OUTPUTS, LifBrain};
use rynvek_wgpu::GpuResult;
use rynvek_wgpu::kernels::{GpuLifBatch, LifBatchState, LifKernels};

/// 把同一 `n_hid` 的一组脑实例打包上传
pub fn upload_lif_batch(
    device: &wgpu::Device,
    kernels: &LifKernels,
    brains: &[LifBrain],
    plasticity: bool,
    diagnostics: &wgpu::Buffer,
) -> GpuResult<GpuLifBatch> {
    let n_hid = brains.first().map_or(0, LifBrain::n_hid);
    if brains.iter().any(|b| b.n_hid() != n_hid) {
        return Err(rynvek_wgpu::GpuError::new(
            rynvek_core::error::RynvekErrorCode::DimensionMismatch,
            "合批的脑实例必须同拓扑",
        ));
    }
    let mut weights = Vec::new();
    let mut state = LifBatchState::default();
    for b in brains {
        weights.extend(b.weights_base_q8.iter().map(|&w| i32::from(w)));
        state
            .membrane_v_q8
            .extend(b.membrane_v_q8.iter().map(|v| i32::from(v.0)));
        state
            .refractory_ticks
            .extend(b.refractory_ticks.iter().map(|&r| u32::from(r)));
        state
            .spike_ring
            .extend(b.spike_ring.iter().map(|&r| LifBatchState::split_ring(r)));
        state
            .weights_delta_q8
            .extend(b.weights_delta_q8.iter().map(|&w| i32::from(w)));
        state
            .eligibility_q8
            .extend(b.eligibility_q8.iter().map(|&e| i32::from(e)));
    }
    GpuLifBatch::new(
        device,
        kernels,
        brains.len() as u32,
        n_hid as u32,
        &weights,
        &state,
        plasticity,
        diagnostics,
    )
}

/// 打包一个神经子步的输入
pub fn pack_inputs(sensory: &[[Q8_8; LIF_INPUTS]], modulators: &[[i16; 4]]) -> (Vec<i32>, Vec<i32>) {
    (
        sensory.iter().flatten().map(|v| i32::from(v.0)).collect(),
        modulators.iter().flatten().map(|&m| i32::from(m)).collect(),
    )
}

/// 把 GPU 状态解包回 `LifBrain`（只覆盖可变状态，先天权重不变）
pub fn unpack_into(brains: &mut [LifBrain], state: &LifBatchState) {
    let mut h_off = 0;
    let mut w_off = 0;
    let mut e_off = 0;
    for b in brains {
        let n = b.n_hid();
        let nw = n * (LIF_INPUTS + LIF_OUTPUTS);
        let ne = n * LIF_INPUTS;
        for i in 0..n {
            b.membrane_v_q8[i] = Q8_8(state.membrane_v_q8[h_off + i] as i16);
            b.refractory_ticks[i] = state.refractory_ticks[h_off + i] as u8;
            b.spike_ring[i] = LifBatchState::join_ring(state.spike_ring[h_off + i]);
        }
        for i in 0..nw {
            b.weights_delta_q8[i] = state.weights_delta_q8[w_off + i] as i16;
        }
        for i in 0..ne {
            b.eligibility_q8[i] = state.eligibility_q8[e_off + i] as i16;
        }
        h_off += n;
        w_off += nw;
        e_off += ne;
    }
}

/// 解包读出
pub fn unpack_readout(readout: &[i32]) -> Vec<[Q8_8; LIF_OUTPUTS]> {
    readout
        .chunks_exact(LIF_OUTPUTS)
        .map(|c| std::array::from_fn(|k| Q8_8(c[k] as i16)))
        .collect()
}
