// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 场扩散 GPU 驻留 Tile（规范 06 §6.1 + CFL 自动子步，与 `rynvek-field` 逐位一致）。
//!
//! 每个权威 Tick 在一个计算通道内依次编码 `n = ceil(D / 64)` 个子步：
//! `field_fill_halo(src)` → `field_diffuse(src → dst)` → 交换；衰减按子步均分、余数并入最后一个子步，
//! 源项只在最后一个子步注入（与 CPU `solve_reaction_diffusion_tile_q8_cfl` 完全相同的调度）。
//! 最后追加 `field_row_sums`，供宿主经 3-Slot 回读环异步核对守恒量。
//! 所有子步参数在构造时一次性写入各自的 Uniform 缓冲，Tick 期间无需任何 `write_buffer`。

use bytemuck::{Pod, Zeroable};

use crate::binding::{BindGroupSlot, BindingSpec};
use crate::error::{GpuError, GpuResult};
use crate::kernel::{ComputeKernel, KernelLayout, create_storage_buffer, create_uniform_buffer};
use crate::shader::ShaderSource;
use crate::{grid_workgroups, linear_workgroups};

/// 二维显式格式 CFL 稳定上限（规范 06：`D ≤ 0.25`，Q8.8 = 64），与 `rynvek-field` 同值
pub const CFL_MAX_DIFFUSION_Q8: i32 = 64;
/// Halo 宽度（规范 06：`HALO = 2`）
pub const FIELD_HALO: u32 = 2;

/// 通道参数
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuFieldParams {
    pub diffusion_q8: i32,
    pub decay_q8: i32,
    pub min_val_q8: i16,
    pub max_val_q8: i16,
}

/// 单个 CFL 子步
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSubstep {
    pub diffusion_q8: i32,
    pub decay_q8: i32,
    pub apply_sources: bool,
}

/// 与 CPU `solve_reaction_diffusion_tile_q8_cfl` 相同的子步调度；参数非法时报 `FieldCflStabilityViolation`
pub fn field_substep_schedule(diffusion_q8: i32, decay_q8: i32) -> GpuResult<Vec<FieldSubstep>> {
    if diffusion_q8 < 0 || !(0..=256).contains(&decay_q8) {
        return Err(GpuError::new(
            rynvek_core::error::RynvekErrorCode::FieldCflStabilityViolation,
            format!("非法场参数 D={diffusion_q8} λ={decay_q8}"),
        ));
    }
    let steps = if diffusion_q8 <= 0 {
        1
    } else {
        (diffusion_q8 as u32).div_ceil(CFL_MAX_DIFFUSION_Q8 as u32).max(1)
    };
    let d_sub = diffusion_q8 / steps as i32;
    let decay_base = decay_q8 / steps as i32;
    let decay_rem = decay_q8 - decay_base * steps as i32;
    Ok((0..steps)
        .map(|s| {
            let last = s + 1 == steps;
            FieldSubstep {
                diffusion_q8: d_sub,
                decay_q8: if last { decay_base + decay_rem } else { decay_base },
                apply_sources: last,
            }
        })
        .collect())
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FieldUniform {
    tile_dim: u32,
    halo: u32,
    stride: u32,
    apply_sources: u32,
    diffusion_q8: i32,
    decay_q8: i32,
    min_val_q8: i32,
    max_val_q8: i32,
}

/// 场内核三件套（每个设备创建一次，所有通道共享）
#[derive(Debug)]
pub struct FieldKernels {
    halo: ComputeKernel,
    diffuse: ComputeKernel,
    row_sums: ComputeKernel,
}

impl FieldKernels {
    pub fn new(device: &wgpu::Device) -> GpuResult<Self> {
        let frame = || KernelLayout::new().with(BindGroupSlot::Frame, BindingSpec::uniform(0));
        Ok(Self {
            halo: ComputeKernel::new(
                device,
                ShaderSource::FieldFillHalo,
                &frame().with(BindGroupSlot::StateOut, BindingSpec::read_write(0)),
            )?,
            diffuse: ComputeKernel::new(
                device,
                ShaderSource::FieldDiffuse,
                &frame()
                    .with(BindGroupSlot::StateIn, BindingSpec::read(0))
                    .with(BindGroupSlot::StateIn, BindingSpec::read(1))
                    .with(BindGroupSlot::StateOut, BindingSpec::read_write(0)),
            )?,
            row_sums: ComputeKernel::new(
                device,
                ShaderSource::FieldRowSums,
                &frame()
                    .with(BindGroupSlot::StateIn, BindingSpec::read(0))
                    .with(BindGroupSlot::Auxiliary, BindingSpec::read_write(0)),
            )?,
        })
    }
}

/// 某一奇偶相位（src = buffers[p]）下的子步 BindGroup
#[derive(Debug)]
struct SubstepGroups {
    frame_halo: wgpu::BindGroup,
    frame_diffuse: wgpu::BindGroup,
    halo_out: [wgpu::BindGroup; 2],
    diffuse_in: [wgpu::BindGroup; 2],
    diffuse_out: [wgpu::BindGroup; 2],
}

/// GPU 驻留的单通道场 Tile（双缓冲 + Halo）
#[derive(Debug)]
pub struct GpuFieldTile {
    tile_dim: u32,
    stride: u32,
    params: GpuFieldParams,
    schedule: Vec<FieldSubstep>,
    buffers: [wgpu::Buffer; 2],
    sources: wgpu::Buffer,
    row_sums: wgpu::Buffer,
    current: usize,
    substeps: Vec<SubstepGroups>,
    halo_empty: [wgpu::BindGroup; 2],
    diffuse_aux: wgpu::BindGroup,
    rows_frame: wgpu::BindGroup,
    rows_in: [wgpu::BindGroup; 2],
    rows_out: wgpu::BindGroup,
    rows_empty: wgpu::BindGroup,
}

impl GpuFieldTile {
    /// 创建 Tile；`tile_dim` 为内区边长（规范为 256，测试可用更小值），Halo 固定为 2
    pub fn new(
        device: &wgpu::Device,
        kernels: &FieldKernels,
        tile_dim: u32,
        params: GpuFieldParams,
        diagnostics: &wgpu::Buffer,
    ) -> GpuResult<Self> {
        let schedule = field_substep_schedule(params.diffusion_q8, params.decay_q8)?;
        let stride = tile_dim + 2 * FIELD_HALO;
        let cells = (stride * stride) as usize;
        let zeros = vec![0u8; cells * 4];
        let buffers = [0, 1].map(|i| {
            create_storage_buffer(
                device,
                &format!("rynvek.field.padded.{i}"),
                &zeros,
                wgpu::BufferUsages::empty(),
            )
        });
        let [b0, b1] = buffers;
        let buffers = [b0?, b1?];
        let sources = create_storage_buffer(
            device,
            "rynvek.field.sources",
            &vec![0u8; (tile_dim * tile_dim) as usize * 4],
            wgpu::BufferUsages::empty(),
        )?;
        let row_sums = create_storage_buffer(
            device,
            "rynvek.field.row_sums",
            &vec![0u8; tile_dim as usize * 4],
            wgpu::BufferUsages::empty(),
        )?;

        let uniform_for = |s: &FieldSubstep| FieldUniform {
            tile_dim,
            halo: FIELD_HALO,
            stride,
            apply_sources: u32::from(s.apply_sources),
            diffusion_q8: s.diffusion_q8,
            decay_q8: s.decay_q8,
            min_val_q8: i32::from(params.min_val_q8),
            max_val_q8: i32::from(params.max_val_q8),
        };
        let substeps = schedule
            .iter()
            .map(|s| {
                let uniform = create_uniform_buffer(device, "rynvek.field.params", bytemuck::bytes_of(&uniform_for(s)));
                let frame =
                    |k: &ComputeKernel| k.bind_group(device, BindGroupSlot::Frame, &[(0, &uniform), (15, diagnostics)]);
                SubstepGroups {
                    frame_halo: frame(&kernels.halo),
                    frame_diffuse: frame(&kernels.diffuse),
                    halo_out: [0, 1].map(|p| {
                        kernels
                            .halo
                            .bind_group(device, BindGroupSlot::StateOut, &[(0, &buffers[p])])
                    }),
                    diffuse_in: [0, 1].map(|p| {
                        kernels
                            .diffuse
                            .bind_group(device, BindGroupSlot::StateIn, &[(0, &buffers[p]), (1, &sources)])
                    }),
                    diffuse_out: [0, 1].map(|p| {
                        kernels
                            .diffuse
                            .bind_group(device, BindGroupSlot::StateOut, &[(0, &buffers[1 - p])])
                    }),
                }
            })
            .collect();
        let rows_uniform = create_uniform_buffer(
            device,
            "rynvek.field.row_sums.params",
            bytemuck::bytes_of(&uniform_for(&schedule[0])),
        );
        let halo_empty = [
            kernels.halo.bind_group(device, BindGroupSlot::StateIn, &[]),
            kernels.halo.bind_group(device, BindGroupSlot::Auxiliary, &[]),
        ];
        let diffuse_aux = kernels.diffuse.bind_group(device, BindGroupSlot::Auxiliary, &[]);
        let rows_frame =
            kernels
                .row_sums
                .bind_group(device, BindGroupSlot::Frame, &[(0, &rows_uniform), (15, diagnostics)]);
        let rows_in = [0, 1].map(|p| {
            kernels
                .row_sums
                .bind_group(device, BindGroupSlot::StateIn, &[(0, &buffers[p])])
        });
        let rows_out = kernels
            .row_sums
            .bind_group(device, BindGroupSlot::Auxiliary, &[(0, &row_sums)]);
        let rows_empty = kernels.row_sums.bind_group(device, BindGroupSlot::StateOut, &[]);
        Ok(Self {
            tile_dim,
            stride,
            params,
            schedule,
            buffers,
            sources,
            row_sums,
            current: 0,
            substeps,
            halo_empty,
            diffuse_aux,
            rows_frame,
            rows_in,
            rows_out,
            rows_empty,
        })
    }

    pub fn tile_dim(&self) -> u32 {
        self.tile_dim
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn params(&self) -> GpuFieldParams {
        self.params
    }

    /// 每 Tick 子步数
    pub fn substeps_per_tick(&self) -> u32 {
        self.schedule.len() as u32
    }

    /// 当前（最新）状态所在缓冲（渲染层热力图直接绑定它，避免回读）
    pub fn current_buffer(&self) -> &wgpu::Buffer {
        &self.buffers[self.current]
    }

    /// 逐行内区和缓冲（`tile_dim × i32`）
    pub fn row_sums_buffer(&self) -> &wgpu::Buffer {
        &self.row_sums
    }

    pub fn row_sums_bytes(&self) -> u64 {
        u64::from(self.tile_dim) * 4
    }

    fn expect_len(&self, got: usize, want: usize, what: &str) -> GpuResult<()> {
        if got == want {
            Ok(())
        } else {
            Err(GpuError::new(
                rynvek_core::error::RynvekErrorCode::DimensionMismatch,
                format!("{what} 长度 {got} ≠ {want}"),
            ))
        }
    }

    /// 上传完整的含 Halo 状态（`stride²` 个 Q8.8 样本），写入当前缓冲
    pub fn upload_padded(&self, queue: &wgpu::Queue, padded_q8: &[i16]) -> GpuResult<()> {
        self.expect_len(padded_q8.len(), (self.stride * self.stride) as usize, "padded")?;
        queue.write_buffer(
            self.current_buffer(),
            0,
            bytemuck::cast_slice(&super::widen_i16(padded_q8)),
        );
        Ok(())
    }

    /// 上传源项（`tile_dim²` 个 Q8.8 样本）
    pub fn upload_sources(&self, queue: &wgpu::Queue, sources_q8: &[i16]) -> GpuResult<()> {
        self.expect_len(sources_q8.len(), (self.tile_dim * self.tile_dim) as usize, "sources")?;
        queue.write_buffer(&self.sources, 0, bytemuck::cast_slice(&super::widen_i16(sources_q8)));
        Ok(())
    }

    /// 编码一个权威 Tick（全部 CFL 子步 + 行和），返回子步数
    pub fn encode_tick(&mut self, kernels: &FieldKernels, encoder: &mut wgpu::CommandEncoder) -> u32 {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("rynvek.field.tick"),
            timestamp_writes: None,
        });
        let halo_groups = grid_workgroups(self.stride);
        let interior_groups = grid_workgroups(self.tile_dim);
        for sub in &self.substeps {
            let p = self.current;
            kernels.halo.dispatch(
                &mut pass,
                [
                    &sub.frame_halo,
                    &self.halo_empty[0],
                    &sub.halo_out[p],
                    &self.halo_empty[1],
                ],
                [halo_groups, halo_groups, 1],
            );
            kernels.diffuse.dispatch(
                &mut pass,
                [
                    &sub.frame_diffuse,
                    &sub.diffuse_in[p],
                    &sub.diffuse_out[p],
                    &self.diffuse_aux,
                ],
                [interior_groups, interior_groups, 1],
            );
            self.current = 1 - p;
        }
        kernels.row_sums.dispatch(
            &mut pass,
            [
                &self.rows_frame,
                &self.rows_in[self.current],
                &self.rows_empty,
                &self.rows_out,
            ],
            [linear_workgroups(self.tile_dim), 1, 1],
        );
        self.substeps.len() as u32
    }

    /// 阻塞回读当前缓冲的内区样本（仅测试 / 恢复校验）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_interior_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> GpuResult<Vec<i16>> {
        let bytes = crate::kernel::read_buffer_blocking(
            device,
            queue,
            self.current_buffer(),
            u64::from(self.stride * self.stride) * 4,
        )?;
        let padded: Vec<i32> = bytemuck::pod_collect_to_vec(&bytes);
        let mut out = Vec::with_capacity((self.tile_dim * self.tile_dim) as usize);
        for y in 0..self.tile_dim {
            let start = ((y + FIELD_HALO) * self.stride + FIELD_HALO) as usize;
            out.extend_from_slice(&super::narrow_i32(&padded[start..start + self.tile_dim as usize]));
        }
        Ok(out)
    }

    /// 阻塞回读逐行内区和（仅测试）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_row_sums_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> GpuResult<Vec<i32>> {
        let bytes = crate::kernel::read_buffer_blocking(device, queue, &self.row_sums, self.row_sums_bytes())?;
        Ok(bytemuck::pod_collect_to_vec(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_matches_cpu_cfl_split() {
        assert_eq!(field_substep_schedule(64, 13).unwrap().len(), 1);
        let s = field_substep_schedule(129, 10).unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s.iter().map(|x| x.diffusion_q8).collect::<Vec<_>>(), [43, 43, 43]);
        // 衰减余数并入最后一个子步
        assert_eq!(s.iter().map(|x| x.decay_q8).collect::<Vec<_>>(), [3, 3, 4]);
        assert_eq!(
            s.iter().map(|x| x.apply_sources).collect::<Vec<_>>(),
            [false, false, true]
        );
        assert_eq!(field_substep_schedule(0, 0).unwrap().len(), 1);
        assert!(field_substep_schedule(-1, 0).is_err());
        assert!(field_substep_schedule(10, 257).is_err());
    }
}
