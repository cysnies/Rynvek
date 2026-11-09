// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规则编译器 WGSL 后端（GPU Tier-0）的宿主执行器。
//!
//! 绑定布局严格按 `emit_wgsl_compute_shader` 生成的声明构建（BindGroup 0–3 约定）：
//! - `@group(0)`：`binding 0` RuleParams Uniform、`binding 1` 实体序号 `vec2<u32>`（另自动追加诊断 `binding 15`）；
//! - `@group(1)`：可读组件列 `comp_in_{slot}`；`@group(2)`：可写组件列 `comp_out_{slot}`；
//! - `@group(3)`：`0` 场采样、`1` 场沉积、`2/3` CSR 邻居表（按模块实际使用情况）。
//!
//! 组件原始值以 `i32` 承载（Bool / I32 / Q8.8），Q32.32 以 `vec2<u32>(lo, hi)` 承载；
//! 可写列在调度前以规范化后的输入初始化，与 CPU 解释器 `execute_rule_tick` 的输出初值完全一致。

use bytemuck::{Pod, Zeroable};
use rynvek_rules::cpu::RuleWorldState;
use rynvek_rules::ir::{Opcode, RuleModule, ValueType};
use rynvek_rules::semantics::normalize;
use rynvek_rules::wgsl::{RULE_ENTRY_POINT, emit_wgsl_compute_shader};
use rynvek_wgpu::kernel::{ComputeKernel, KernelLayout, create_storage_buffer, create_uniform_buffer};
use rynvek_wgpu::{BindGroupSlot, BindingSpec, GpuResult, linear_workgroups};

/// 场通道数（与规则 IR 的 `FIELD_CHANNEL_NAMES` 一致）
const CHANNELS: usize = 5;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct RuleParams {
    entity_count: u32,
    tick_lo: u32,
    tick_hi: u32,
    seed_lo: u32,
    seed_hi: u32,
    _pad: [u32; 3],
}

/// 规则 GPU 执行结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuRuleOutput {
    pub components: Vec<Vec<i64>>,
    pub field_deposits: Vec<i64>,
}

/// 已编译的规则计算内核
#[derive(Debug)]
pub struct GpuRuleKernel {
    module: RuleModule,
    kernel: ComputeKernel,
    uses_samples: bool,
    uses_deposits: bool,
    uses_neighbors: bool,
}

fn encode_column(ty: ValueType, values: &[i64]) -> Vec<u8> {
    if ty == ValueType::Q32_32 {
        let words: Vec<[u32; 2]> = values
            .iter()
            .map(|&v| [v as u64 as u32, ((v as u64) >> 32) as u32])
            .collect();
        bytemuck::cast_slice(&words).to_vec()
    } else {
        let words: Vec<i32> = values.iter().map(|&v| normalize(ty, v) as i32).collect();
        bytemuck::cast_slice(&words).to_vec()
    }
}

fn decode_column(ty: ValueType, bytes: &[u8]) -> Vec<i64> {
    if ty == ValueType::Q32_32 {
        let words: Vec<[u32; 2]> = bytemuck::pod_collect_to_vec(bytes);
        words
            .iter()
            .map(|w| (u64::from(w[0]) | (u64::from(w[1]) << 32)) as i64)
            .collect()
    } else {
        let words: Vec<i32> = bytemuck::pod_collect_to_vec(bytes);
        words.iter().map(|&w| i64::from(w)).collect()
    }
}

impl GpuRuleKernel {
    pub fn new(device: &wgpu::Device, module: &RuleModule) -> GpuResult<Self> {
        let uses_samples = module.uses(Opcode::SampleField);
        let uses_deposits = module.uses(Opcode::EmitFieldDeposit);
        let uses_neighbors = module.uses(Opcode::NeighborCount) || module.uses(Opcode::LoadNeighborComponent);
        let mut layout = KernelLayout::new()
            .with(BindGroupSlot::Frame, BindingSpec::uniform(0))
            .with(BindGroupSlot::Frame, BindingSpec::read(1));
        for (slot, c) in module.components.iter().enumerate() {
            if c.read {
                layout = layout.with(BindGroupSlot::StateIn, BindingSpec::read(slot as u32));
            }
            if c.write {
                layout = layout.with(BindGroupSlot::StateOut, BindingSpec::read_write(slot as u32));
            }
        }
        if uses_samples {
            layout = layout.with(BindGroupSlot::Auxiliary, BindingSpec::read(0));
        }
        if uses_deposits {
            layout = layout.with(BindGroupSlot::Auxiliary, BindingSpec::read_write(1));
        }
        if uses_neighbors {
            layout = layout
                .with(BindGroupSlot::Auxiliary, BindingSpec::read(2))
                .with(BindGroupSlot::Auxiliary, BindingSpec::read(3));
        }
        let code = emit_wgsl_compute_shader(module);
        let kernel = ComputeKernel::from_wgsl(
            device,
            &format!("rynvek.rule.{}", module.name),
            &code,
            RULE_ENTRY_POINT,
            &layout,
        )?;
        Ok(Self {
            module: module.clone(),
            kernel,
            uses_samples,
            uses_deposits,
            uses_neighbors,
        })
    }

    /// 执行一个 Tick 并阻塞回读（差分测试与离线批处理；实时主循环应改用回读环）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_blocking(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        diagnostics: &wgpu::Buffer,
        world: &RuleWorldState,
    ) -> GpuResult<GpuRuleOutput> {
        use rynvek_wgpu::kernel::read_buffer_blocking;
        let n = world.entity_serials.len();
        let none = wgpu::BufferUsages::empty();
        let params = RuleParams {
            entity_count: n as u32,
            tick_lo: world.tick as u32,
            tick_hi: (world.tick >> 32) as u32,
            seed_lo: world.world_seed as u32,
            seed_hi: (world.world_seed >> 32) as u32,
            _pad: [0; 3],
        };
        let uniform = create_uniform_buffer(device, "rynvek.rule.params", bytemuck::bytes_of(&params));
        let serials: Vec<[u32; 2]> = world
            .entity_serials
            .iter()
            .map(|&s| [s as u32, (s >> 32) as u32])
            .collect();
        let serials = create_storage_buffer(device, "rynvek.rule.serials", bytemuck::cast_slice(&serials), none)?;

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for (slot, c) in self.module.components.iter().enumerate() {
            let bytes = encode_column(c.ty, &world.components[slot]);
            if c.read {
                inputs.push((slot as u32, create_storage_buffer(device, &c.name, &bytes, none)?));
            }
            if c.write {
                outputs.push((slot, create_storage_buffer(device, &c.name, &bytes, none)?));
            }
        }
        let to_i32 = |v: &[i64]| -> Vec<i32> { v.iter().map(|&x| x as i32).collect() };
        let samples = create_storage_buffer(
            device,
            "rynvek.rule.field_samples",
            bytemuck::cast_slice(&to_i32(&world.field_samples)),
            none,
        )?;
        let deposits = create_storage_buffer(device, "rynvek.rule.field_deposits", &vec![0u8; CHANNELS * n * 4], none)?;
        let offsets = create_storage_buffer(
            device,
            "rynvek.rule.neighbor_offsets",
            bytemuck::cast_slice(&world.neighbor_offsets),
            none,
        )?;
        let indices = create_storage_buffer(
            device,
            "rynvek.rule.neighbor_indices",
            bytemuck::cast_slice(&world.neighbor_indices),
            none,
        )?;

        let k = &self.kernel;
        let g0 = k.bind_group(
            device,
            BindGroupSlot::Frame,
            &[(0, &uniform), (1, &serials), (15, diagnostics)],
        );
        let in_refs: Vec<(u32, &wgpu::Buffer)> = inputs.iter().map(|(s, b)| (*s, b)).collect();
        let g1 = k.bind_group(device, BindGroupSlot::StateIn, &in_refs);
        let out_refs: Vec<(u32, &wgpu::Buffer)> = outputs.iter().map(|(s, b)| (*s as u32, b)).collect();
        let g2 = k.bind_group(device, BindGroupSlot::StateOut, &out_refs);
        let mut aux: Vec<(u32, &wgpu::Buffer)> = Vec::new();
        if self.uses_samples {
            aux.push((0, &samples));
        }
        if self.uses_deposits {
            aux.push((1, &deposits));
        }
        if self.uses_neighbors {
            aux.push((2, &offsets));
            aux.push((3, &indices));
        }
        let g3 = k.bind_group(device, BindGroupSlot::Auxiliary, &aux);

        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(k.label()),
                timestamp_writes: None,
            });
            k.dispatch(&mut pass, [&g0, &g1, &g2, &g3], [linear_workgroups(n as u32), 1, 1]);
        }
        queue.submit([enc.finish()]);

        let mut components: Vec<Vec<i64>> = self
            .module
            .components
            .iter()
            .enumerate()
            .map(|(slot, c)| world.components[slot].iter().map(|&v| normalize(c.ty, v)).collect())
            .collect();
        for (slot, buf) in &outputs {
            let ty = self.module.components[*slot].ty;
            let elem = if ty == ValueType::Q32_32 { 8 } else { 4 };
            components[*slot] = decode_column(ty, &read_buffer_blocking(device, queue, buf, (n * elem) as u64)?);
        }
        let field_deposits = decode_column(
            ValueType::I32,
            &read_buffer_blocking(device, queue, &deposits, (CHANNELS * n * 4) as u64)?,
        );
        Ok(GpuRuleOutput {
            components,
            field_deposits,
        })
    }
}
