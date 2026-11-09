// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 批量 Philox4x32-10 内核（规范 02 冻结 WGSL）：恢复第 5 步的校验探针与 CPU/GPU 差分基准。

use bytemuck::{Pod, Zeroable};

use crate::binding::{BindGroupSlot, BindingSpec};
use crate::error::GpuResult;
use crate::kernel::{ComputeKernel, KernelLayout, create_storage_buffer, create_uniform_buffer};
use crate::linear_workgroups;
use crate::shader::ShaderSource;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PhiloxParams {
    count: u32,
    key_lo: u32,
    key_hi: u32,
    _pad: u32,
}

/// Philox 批量内核
#[derive(Debug)]
pub struct PhiloxKernel {
    kernel: ComputeKernel,
}

impl PhiloxKernel {
    pub fn layout() -> KernelLayout {
        KernelLayout::new()
            .with(BindGroupSlot::Frame, BindingSpec::uniform(0))
            .with(BindGroupSlot::StateIn, BindingSpec::read(0))
            .with(BindGroupSlot::StateOut, BindingSpec::read_write(0))
    }

    pub fn new(device: &wgpu::Device) -> GpuResult<Self> {
        Ok(Self {
            kernel: ComputeKernel::new(device, ShaderSource::PhiloxFill, &Self::layout())?,
        })
    }

    /// 编码一次批量计算，返回输出缓冲（`counters.len() × 16` 字节）
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        diagnostics: &wgpu::Buffer,
        key: [u32; 2],
        counters: &[[u32; 4]],
    ) -> GpuResult<wgpu::Buffer> {
        let params = PhiloxParams {
            count: counters.len() as u32,
            key_lo: key[0],
            key_hi: key[1],
            _pad: 0,
        };
        let uniform = create_uniform_buffer(device, "rynvek.philox.params", bytemuck::bytes_of(&params));
        let input = create_storage_buffer(
            device,
            "rynvek.philox.counters",
            bytemuck::cast_slice(counters),
            wgpu::BufferUsages::empty(),
        )?;
        let output = create_storage_buffer(
            device,
            "rynvek.philox.outputs",
            &vec![0u8; counters.len() * 16],
            wgpu::BufferUsages::empty(),
        )?;
        let g0 = self
            .kernel
            .bind_group(device, BindGroupSlot::Frame, &[(0, &uniform), (15, diagnostics)]);
        let g1 = self.kernel.bind_group(device, BindGroupSlot::StateIn, &[(0, &input)]);
        let g2 = self.kernel.bind_group(device, BindGroupSlot::StateOut, &[(0, &output)]);
        let g3 = self.kernel.bind_group(device, BindGroupSlot::Auxiliary, &[]);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(self.kernel.label()),
            timestamp_writes: None,
        });
        self.kernel
            .dispatch(&mut pass, [&g0, &g1, &g2, &g3], [linear_workgroups(params.count), 1, 1]);
        drop(pass);
        Ok(output)
    }

    /// 阻塞执行并回读（仅测试 / 恢复校验探针）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_blocking(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        diagnostics: &wgpu::Buffer,
        key: [u32; 2],
        counters: &[[u32; 4]],
    ) -> GpuResult<Vec<[u32; 4]>> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rynvek.philox"),
        });
        let output = self.encode(device, &mut encoder, diagnostics, key, counters)?;
        queue.submit([encoder.finish()]);
        let bytes = crate::kernel::read_buffer_blocking(device, queue, &output, (counters.len() * 16) as u64)?;
        Ok(bytemuck::pod_collect_to_vec::<u8, [u32; 4]>(&bytes))
    }
}
