// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 通用计算管线：按 BindGroup 0–3 冻结约定构建显式 `PipelineLayout`。
//!
//! - 4 个 BindGroup 槽位始终全部存在（未使用的槽位为空布局），保证 `@group(n)` 语义在所有内核中一致；
//! - Group 0 自动追加诊断计数器 `@binding(15)`（INV-01），调用方只需声明内核自身的绑定；
//! - 布局在创建前校验：Group 1 只能是只读存储，Group 2 不得放 Uniform，绑定号不得重复。

use wgpu::util::DeviceExt;

use crate::binding::{BindGroupSlot, BindingSpec, DIAGNOSTICS_BINDING, DIAGNOSTICS_BYTES};
use crate::error::{GpuError, GpuResult};
use crate::segment::check_monolithic_storage_size;
use crate::shader::{ShaderLibrary, ShaderSource};

/// 内核的 4 组绑定声明
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KernelLayout {
    groups: [Vec<BindingSpec>; 4],
}

impl KernelLayout {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个绑定（构建器风格）
    pub fn with(mut self, slot: BindGroupSlot, spec: BindingSpec) -> Self {
        self.groups[slot.index() as usize].push(spec);
        self
    }

    /// 某槽位的声明（Group 0 含自动追加的诊断绑定）
    pub fn entries(&self, slot: BindGroupSlot) -> Vec<BindingSpec> {
        let mut specs = self.groups[slot.index() as usize].clone();
        if slot == DIAGNOSTICS_BINDING.0 && !specs.iter().any(|s| s.binding == DIAGNOSTICS_BINDING.1) {
            specs.push(BindingSpec::read_write(DIAGNOSTICS_BINDING.1));
        }
        specs
    }

    /// 校验槽位约束与重复绑定号
    pub fn validate(&self) -> GpuResult<()> {
        for slot in BindGroupSlot::ALL {
            let specs = self.entries(slot);
            for (i, s) in specs.iter().enumerate() {
                if !s.allowed_in(slot) {
                    return Err(GpuError::shader(format!(
                        "{}: 绑定 {} 的类型 {:?} 违反槽位约定",
                        slot.label(),
                        s.binding,
                        s.kind
                    )));
                }
                if specs[..i].iter().any(|p| p.binding == s.binding) {
                    return Err(GpuError::shader(format!("{}: 绑定号 {} 重复", slot.label(), s.binding)));
                }
            }
        }
        Ok(())
    }
}

/// 已编译的计算内核
#[derive(Debug)]
pub struct ComputeKernel {
    label: String,
    layouts: [wgpu::BindGroupLayout; 4],
    pipeline: wgpu::ComputePipeline,
}

impl ComputeKernel {
    /// 由核心着色器构建
    pub fn new(device: &wgpu::Device, source: ShaderSource, layout: &KernelLayout) -> GpuResult<Self> {
        let module = ShaderLibrary::create_module(device, source)?;
        Self::from_module(device, source.label(), &module, source.entry_point(), layout)
    }

    /// 由任意 WGSL（如规则编译器产物）构建
    pub fn from_wgsl(
        device: &wgpu::Device,
        label: &str,
        code: &str,
        entry_point: &str,
        layout: &KernelLayout,
    ) -> GpuResult<Self> {
        let module = ShaderLibrary::create_module_from_wgsl(device, label, code)?;
        Self::from_module(device, label, &module, entry_point, layout)
    }

    fn from_module(
        device: &wgpu::Device,
        label: &str,
        module: &wgpu::ShaderModule,
        entry_point: &str,
        layout: &KernelLayout,
    ) -> GpuResult<Self> {
        layout.validate()?;
        let layouts = BindGroupSlot::ALL.map(|slot| {
            let entries: Vec<_> = layout
                .entries(slot)
                .into_iter()
                .map(|s| s.layout_entry(wgpu::ShaderStages::COMPUTE))
                .collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(slot.label()),
                entries: &entries,
            })
        });
        let layout_refs: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(Some).collect();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &layout_refs,
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            module,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Self {
            label: label.to_owned(),
            layouts,
            pipeline,
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn layout(&self, slot: BindGroupSlot) -> &wgpu::BindGroupLayout {
        &self.layouts[slot.index() as usize]
    }

    /// 以 `(binding, buffer)` 列表创建某槽位的 BindGroup（整段绑定）
    pub fn bind_group(
        &self,
        device: &wgpu::Device,
        slot: BindGroupSlot,
        buffers: &[(u32, &wgpu::Buffer)],
    ) -> wgpu::BindGroup {
        let entries: Vec<_> = buffers
            .iter()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(slot.label()),
            layout: self.layout(slot),
            entries: &entries,
        })
    }

    /// 在计算通道中调度一次（4 个 BindGroup 依次绑定到 0–3）
    pub fn dispatch(&self, pass: &mut wgpu::ComputePass<'_>, groups: [&wgpu::BindGroup; 4], workgroups: [u32; 3]) {
        pass.set_pipeline(&self.pipeline);
        for (i, g) in groups.into_iter().enumerate() {
            pass.set_bind_group(i as u32, g, &[]);
        }
        pass.dispatch_workgroups(workgroups[0], workgroups[1], workgroups[2]);
    }
}

/// 创建并初始化一个存储缓冲（单体缓冲受 128 MiB 上限约束，更大的数据应走分段池）
pub fn create_storage_buffer(
    device: &wgpu::Device,
    label: &str,
    contents: &[u8],
    extra_usage: wgpu::BufferUsages,
) -> GpuResult<wgpu::Buffer> {
    check_monolithic_storage_size(contents.len() as u64)?;
    // WebGPU 不允许零长度绑定：至少分配 4 字节
    let padded;
    let data = if contents.len() < 4 {
        padded = [0u8; 4];
        &padded[..]
    } else {
        contents
    };
    Ok(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: data,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC | extra_usage,
    }))
}

/// 创建 Uniform 缓冲
pub fn create_uniform_buffer(device: &wgpu::Device, label: &str, contents: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

/// GPU 诊断计数器 `s_diagnostics`（Group 0 binding 15，4×u32）
#[derive(Debug)]
pub struct GpuDiagnostics {
    pub buffer: wgpu::Buffer,
}

impl GpuDiagnostics {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rynvek.diagnostics"),
                contents: &[0u8; DIAGNOSTICS_BYTES as usize],
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            }),
        }
    }

    /// 每 Tick 开始前清零
    pub fn reset(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.buffer, 0, &[0u8; DIAGNOSTICS_BYTES as usize]);
    }

    /// 阻塞读取（仅测试 / 离线工具）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> GpuResult<[u32; 4]> {
        let bytes = read_buffer_blocking(device, queue, &self.buffer, DIAGNOSTICS_BYTES)?;
        let words: Vec<u32> = bytemuck::pod_collect_to_vec(&bytes);
        Ok([words[0], words[1], words[2], words[3]])
    }
}

/// 阻塞回读整段缓冲：**仅用于测试、恢复校验与离线工具**，Tick 主循环必须使用 [`crate::ReadbackRing`]
#[cfg(not(target_arch = "wasm32"))]
pub fn read_buffer_blocking(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    src: &wgpu::Buffer,
    bytes: u64,
) -> GpuResult<Vec<u8>> {
    let size = bytes.div_ceil(wgpu::COPY_BUFFER_ALIGNMENT) * wgpu::COPY_BUFFER_ALIGNMENT;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rynvek.readback.blocking"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("rynvek.readback.blocking"),
    });
    encoder.copy_buffer_to_buffer(src, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| GpuError::device_lost(format!("poll 失败：{e}")))?;
    match rx.recv() {
        Ok(Ok(())) => {}
        _ => return Err(GpuError::device_lost("回读映射失败（设备可能已丢失）")),
    }
    let data = staging
        .slice(..)
        .get_mapped_range()
        .map_err(|e| GpuError::device_lost(format!("映射区间不可读：{e:?}")))?
        .to_vec();
    staging.unmap();
    Ok(data[..bytes as usize].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_core::error::RynvekErrorCode;

    #[test]
    fn frame_group_always_carries_diagnostics_binding() {
        let layout = KernelLayout::new().with(BindGroupSlot::Frame, BindingSpec::uniform(0));
        let specs = layout.entries(BindGroupSlot::Frame);
        assert_eq!(specs.len(), 2);
        assert!(specs.contains(&BindingSpec::read_write(15)));
        assert!(layout.entries(BindGroupSlot::Auxiliary).is_empty());
        assert!(layout.validate().is_ok());
    }

    #[test]
    fn invalid_layouts_are_rejected() {
        let rw_in_state_in = KernelLayout::new().with(BindGroupSlot::StateIn, BindingSpec::read_write(0));
        assert_eq!(
            rw_in_state_in.validate().unwrap_err().code,
            RynvekErrorCode::WgslShaderValidationFailed
        );
        let dup = KernelLayout::new()
            .with(BindGroupSlot::StateOut, BindingSpec::read_write(0))
            .with(BindGroupSlot::StateOut, BindingSpec::read_write(0));
        assert!(dup.validate().is_err());
    }
}
