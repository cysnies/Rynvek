// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 批量 LIF 脑实例 GPU 内核宿主（规范 05 `step_lif_brain_deterministic`，与 `rynvek-neural` 逐位一致）。
//!
//! 布局（每脑 `n_hid` 个隐藏神经元，16 输入 / 6 输出）：
//! - `sensory`：`brain_count × 16`；`modulators`：`brain_count × 4`（DA, 5-HT, NE, ACh）；
//! - `weights_base` / `weights_delta`：`brain_count × n_hid × 22`（前 `n_hid×16` 为输入权重，后 `n_hid×6` 为输出权重）；
//! - `membrane` / `refractory` / `spike_ring`：`brain_count × n_hid`；`eligibility`：`brain_count × n_hid × 16`；
//! - `readout`：`brain_count × 6`。
//!
//! 一次 `encode_step` 依次调度 `lif_step`（每神经元一个调用）与 `lif_readout`（每脑一个调用）。

use bytemuck::{Pod, Zeroable};

use crate::binding::{BindGroupSlot, BindingSpec};
use crate::error::{GpuError, GpuResult};
use crate::kernel::{ComputeKernel, KernelLayout, create_storage_buffer, create_uniform_buffer};
use crate::linear_workgroups;
use crate::shader::ShaderSource;

const INPUTS: usize = 16;
const OUTPUTS: usize = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct LifUniform {
    brain_count: u32,
    n_hid: u32,
    plasticity_enabled: u32,
    _pad: u32,
}

/// 批量 LIF 的可变状态（宿主侧镜像，Q8.8 以 `i32` 承载，u64 脉冲环拆为 `[lo, hi]`）
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LifBatchState {
    pub membrane_v_q8: Vec<i32>,
    pub refractory_ticks: Vec<u32>,
    pub spike_ring: Vec<[u32; 2]>,
    pub weights_delta_q8: Vec<i32>,
    pub eligibility_q8: Vec<i32>,
}

impl LifBatchState {
    /// 全零初始状态
    pub fn zeros(brain_count: usize, n_hid: usize) -> Self {
        Self {
            membrane_v_q8: vec![0; brain_count * n_hid],
            refractory_ticks: vec![0; brain_count * n_hid],
            spike_ring: vec![[0, 0]; brain_count * n_hid],
            weights_delta_q8: vec![0; brain_count * n_hid * (INPUTS + OUTPUTS)],
            eligibility_q8: vec![0; brain_count * n_hid * INPUTS],
        }
    }

    /// u64 脉冲环拆分
    pub fn split_ring(v: u64) -> [u32; 2] {
        [v as u32, (v >> 32) as u32]
    }

    /// u64 脉冲环合并
    pub fn join_ring(v: [u32; 2]) -> u64 {
        u64::from(v[0]) | (u64::from(v[1]) << 32)
    }
}

/// LIF 内核对（每设备一次）
#[derive(Debug)]
pub struct LifKernels {
    step: ComputeKernel,
    readout: ComputeKernel,
}

impl LifKernels {
    pub fn new(device: &wgpu::Device) -> GpuResult<Self> {
        let frame = || KernelLayout::new().with(BindGroupSlot::Frame, BindingSpec::uniform(0));
        let mut step_layout = frame()
            .with(BindGroupSlot::StateIn, BindingSpec::read(0))
            .with(BindGroupSlot::StateIn, BindingSpec::read(1))
            .with(BindGroupSlot::StateIn, BindingSpec::read(2));
        for b in 0..5 {
            step_layout = step_layout.with(BindGroupSlot::StateOut, BindingSpec::read_write(b));
        }
        let readout_layout = frame()
            .with(BindGroupSlot::StateIn, BindingSpec::read(1))
            .with(BindGroupSlot::StateIn, BindingSpec::read(3))
            .with(BindGroupSlot::StateIn, BindingSpec::read(4))
            .with(BindGroupSlot::Auxiliary, BindingSpec::read_write(0));
        Ok(Self {
            step: ComputeKernel::new(device, ShaderSource::LifStep, &step_layout)?,
            readout: ComputeKernel::new(device, ShaderSource::LifReadout, &readout_layout)?,
        })
    }
}

/// GPU 驻留的一批 LIF 脑实例
#[derive(Debug)]
pub struct GpuLifBatch {
    brain_count: u32,
    n_hid: u32,
    sensory: wgpu::Buffer,
    modulators: wgpu::Buffer,
    membrane: wgpu::Buffer,
    refractory: wgpu::Buffer,
    spike_ring: wgpu::Buffer,
    weights_delta: wgpu::Buffer,
    eligibility: wgpu::Buffer,
    readout: wgpu::Buffer,
    groups_step: [wgpu::BindGroup; 4],
    groups_readout: [wgpu::BindGroup; 4],
}

fn bytes_i32(v: &[i32]) -> &[u8] {
    bytemuck::cast_slice(v)
}

impl GpuLifBatch {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        kernels: &LifKernels,
        brain_count: u32,
        n_hid: u32,
        weights_base_q8: &[i32],
        state: &LifBatchState,
        plasticity_enabled: bool,
        diagnostics: &wgpu::Buffer,
    ) -> GpuResult<Self> {
        let b = brain_count as usize;
        let h = n_hid as usize;
        let check = |got: usize, want: usize, what: &str| -> GpuResult<()> {
            if got == want {
                Ok(())
            } else {
                Err(GpuError::new(
                    rynvek_core::error::RynvekErrorCode::DimensionMismatch,
                    format!("LIF {what} 长度 {got} ≠ {want}"),
                ))
            }
        };
        check(weights_base_q8.len(), b * h * (INPUTS + OUTPUTS), "weights_base")?;
        check(state.membrane_v_q8.len(), b * h, "membrane")?;
        check(state.refractory_ticks.len(), b * h, "refractory")?;
        check(state.spike_ring.len(), b * h, "spike_ring")?;
        check(
            state.weights_delta_q8.len(),
            b * h * (INPUTS + OUTPUTS),
            "weights_delta",
        )?;
        check(state.eligibility_q8.len(), b * h * INPUTS, "eligibility")?;

        let none = wgpu::BufferUsages::empty();
        let uniform = create_uniform_buffer(
            device,
            "rynvek.lif.params",
            bytemuck::bytes_of(&LifUniform {
                brain_count,
                n_hid,
                plasticity_enabled: u32::from(plasticity_enabled),
                _pad: 0,
            }),
        );
        let sensory = create_storage_buffer(device, "rynvek.lif.sensory", &vec![0u8; b * INPUTS * 4], none)?;
        let weights_base = create_storage_buffer(device, "rynvek.lif.weights_base", bytes_i32(weights_base_q8), none)?;
        let modulators = create_storage_buffer(device, "rynvek.lif.modulators", &vec![0u8; b * 4 * 4], none)?;
        let membrane = create_storage_buffer(device, "rynvek.lif.membrane", bytes_i32(&state.membrane_v_q8), none)?;
        let refractory = create_storage_buffer(
            device,
            "rynvek.lif.refractory",
            bytemuck::cast_slice(&state.refractory_ticks),
            none,
        )?;
        let spike_ring = create_storage_buffer(
            device,
            "rynvek.lif.spike_ring",
            bytemuck::cast_slice(&state.spike_ring),
            none,
        )?;
        let weights_delta = create_storage_buffer(
            device,
            "rynvek.lif.weights_delta",
            bytes_i32(&state.weights_delta_q8),
            none,
        )?;
        let eligibility =
            create_storage_buffer(device, "rynvek.lif.eligibility", bytes_i32(&state.eligibility_q8), none)?;
        let readout = create_storage_buffer(device, "rynvek.lif.readout", &vec![0u8; b * OUTPUTS * 4], none)?;

        let k = &kernels.step;
        let groups_step = [
            k.bind_group(device, BindGroupSlot::Frame, &[(0, &uniform), (15, diagnostics)]),
            k.bind_group(
                device,
                BindGroupSlot::StateIn,
                &[(0, &sensory), (1, &weights_base), (2, &modulators)],
            ),
            k.bind_group(
                device,
                BindGroupSlot::StateOut,
                &[
                    (0, &membrane),
                    (1, &refractory),
                    (2, &spike_ring),
                    (3, &weights_delta),
                    (4, &eligibility),
                ],
            ),
            k.bind_group(device, BindGroupSlot::Auxiliary, &[]),
        ];
        let k = &kernels.readout;
        let groups_readout = [
            k.bind_group(device, BindGroupSlot::Frame, &[(0, &uniform), (15, diagnostics)]),
            k.bind_group(
                device,
                BindGroupSlot::StateIn,
                &[(1, &weights_base), (3, &spike_ring), (4, &weights_delta)],
            ),
            k.bind_group(device, BindGroupSlot::StateOut, &[]),
            k.bind_group(device, BindGroupSlot::Auxiliary, &[(0, &readout)]),
        ];
        Ok(Self {
            brain_count,
            n_hid,
            sensory,
            modulators,
            membrane,
            refractory,
            spike_ring,
            weights_delta,
            eligibility,
            readout,
            groups_step,
            groups_readout,
        })
    }

    pub fn brain_count(&self) -> u32 {
        self.brain_count
    }

    pub fn n_hid(&self) -> u32 {
        self.n_hid
    }

    /// 写入本神经子步的感知输入与调质（`brain_count × 16` / `brain_count × 4`）
    pub fn upload_inputs(&self, queue: &wgpu::Queue, sensory_q8: &[i32], modulators_q8: &[i32]) -> GpuResult<()> {
        let b = self.brain_count as usize;
        if sensory_q8.len() != b * INPUTS || modulators_q8.len() != b * 4 {
            return Err(GpuError::new(
                rynvek_core::error::RynvekErrorCode::DimensionMismatch,
                "LIF 输入维度不符",
            ));
        }
        queue.write_buffer(&self.sensory, 0, bytes_i32(sensory_q8));
        queue.write_buffer(&self.modulators, 0, bytes_i32(modulators_q8));
        Ok(())
    }

    /// 编码一个神经子步（步进 + 读出）
    pub fn encode_step(&self, kernels: &LifKernels, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("rynvek.lif.step"),
            timestamp_writes: None,
        });
        let g = &self.groups_step;
        kernels.step.dispatch(
            &mut pass,
            [&g[0], &g[1], &g[2], &g[3]],
            [linear_workgroups(self.brain_count * self.n_hid), 1, 1],
        );
        let g = &self.groups_readout;
        kernels.readout.dispatch(
            &mut pass,
            [&g[0], &g[1], &g[2], &g[3]],
            [linear_workgroups(self.brain_count), 1, 1],
        );
    }

    /// 读出缓冲（`brain_count × 6 × i32`），可直接拷入回读环
    pub fn readout_buffer(&self) -> &wgpu::Buffer {
        &self.readout
    }

    /// 阻塞回读读出（仅测试）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_readout_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> GpuResult<Vec<i32>> {
        let n = u64::from(self.brain_count) * OUTPUTS as u64 * 4;
        Ok(bytemuck::pod_collect_to_vec(&crate::kernel::read_buffer_blocking(
            device,
            queue,
            &self.readout,
            n,
        )?))
    }

    /// 阻塞回读完整状态（仅测试 / 恢复校验）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_state_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> GpuResult<LifBatchState> {
        let n = u64::from(self.brain_count * self.n_hid);
        let read = |buf: &wgpu::Buffer, bytes: u64| crate::kernel::read_buffer_blocking(device, queue, buf, bytes);
        Ok(LifBatchState {
            membrane_v_q8: bytemuck::pod_collect_to_vec(&read(&self.membrane, n * 4)?),
            refractory_ticks: bytemuck::pod_collect_to_vec(&read(&self.refractory, n * 4)?),
            spike_ring: bytemuck::pod_collect_to_vec(&read(&self.spike_ring, n * 8)?),
            weights_delta_q8: bytemuck::pod_collect_to_vec(&read(&self.weights_delta, n * 22 * 4)?),
            eligibility_q8: bytemuck::pod_collect_to_vec(&read(&self.eligibility, n * 16 * 4)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_split_join_roundtrip() {
        for v in [0u64, 1, 0x8000_0000, 0xDEAD_BEEF_1234_5678, u64::MAX] {
            assert_eq!(LifBatchState::join_ring(LifBatchState::split_ring(v)), v);
        }
    }
}
