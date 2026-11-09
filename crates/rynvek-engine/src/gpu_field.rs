// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 五通道环境场的 GPU 驻留镜像。
//!
//! CPU `FieldWorld` 是 D0 权威状态；镜像在 GPU 上以完全相同的定点算子同步推进，
//! 用途有二：L1 热力图直接绑定 GPU 缓冲（零回读），以及每 Tick 经 3-Slot 回读环异步核对守恒摘要。
//! 摘要缓冲布局：`[s_diagnostics: 4×u32][通道 0 行和: 256×i32] … [通道 4 行和]`。

use rynvek_field::channels::FieldChannelId;
use rynvek_field::solver::{HALO, TILE_DIM};
use rynvek_field::world::FieldWorld;
use rynvek_wgpu::GpuResult;
use rynvek_wgpu::kernels::{FieldKernels, GpuFieldParams, GpuFieldTile};

/// 通道数
pub const FIELD_CHANNELS: usize = 5;
/// 诊断计数器字节数
const DIAG_BYTES: u64 = 16;

/// 解析后的 GPU 摘要
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSummary {
    pub diagnostics: [u32; 4],
    pub interior_sums: [i64; FIELD_CHANNELS],
}

/// 五通道 GPU 镜像
#[derive(Debug)]
pub struct GpuFieldMirror {
    tiles: Vec<GpuFieldTile>,
    summary: wgpu::Buffer,
}

impl GpuFieldMirror {
    /// 摘要字节数
    pub const SUMMARY_BYTES: u64 = DIAG_BYTES + (FIELD_CHANNELS * TILE_DIM * 4) as u64;

    pub fn new(
        device: &wgpu::Device,
        kernels: &FieldKernels,
        field: &FieldWorld,
        diagnostics: &wgpu::Buffer,
    ) -> GpuResult<Self> {
        let tiles = FieldChannelId::ALL
            .iter()
            .map(|&id| {
                let p = field.channel(id).params;
                GpuFieldTile::new(
                    device,
                    kernels,
                    TILE_DIM as u32,
                    GpuFieldParams {
                        diffusion_q8: p.diffusion_q8,
                        decay_q8: p.decay_q8,
                        min_val_q8: p.min_val_q8,
                        max_val_q8: p.max_val_q8,
                    },
                    diagnostics,
                )
            })
            .collect::<GpuResult<Vec<_>>>()?;
        let summary = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rynvek.engine.field.summary"),
            size: Self::SUMMARY_BYTES,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        Ok(Self { tiles, summary })
    }

    /// 从 CPU 权威状态整体上传（初始化、恢复第 4 步与分歧后重同步）
    pub fn upload_from(&self, queue: &wgpu::Queue, field: &FieldWorld) -> GpuResult<()> {
        for (tile, &id) in self.tiles.iter().zip(FieldChannelId::ALL.iter()) {
            let cpu = field.channel(id);
            tile.upload_padded(queue, cpu.current_buf())?;
            tile.upload_sources(queue, &cpu.sources[..])?;
        }
        Ok(())
    }

    /// 编码一个权威 Tick，并把诊断计数与各通道行和汇总进摘要缓冲
    pub fn encode_tick(
        &mut self,
        kernels: &FieldKernels,
        encoder: &mut wgpu::CommandEncoder,
        diagnostics: &wgpu::Buffer,
    ) {
        for tile in &mut self.tiles {
            tile.encode_tick(kernels, encoder);
        }
        encoder.copy_buffer_to_buffer(diagnostics, 0, &self.summary, 0, DIAG_BYTES);
        for (c, tile) in self.tiles.iter().enumerate() {
            let offset = DIAG_BYTES + (c * TILE_DIM * 4) as u64;
            encoder.copy_buffer_to_buffer(tile.row_sums_buffer(), 0, &self.summary, offset, tile.row_sums_bytes());
        }
    }

    pub fn summary_buffer(&self) -> &wgpu::Buffer {
        &self.summary
    }

    /// 某通道当前 GPU 缓冲（热力图绑定）
    pub fn channel_buffer(&self, id: FieldChannelId) -> &wgpu::Buffer {
        self.tiles[id.index()].current_buffer()
    }

    pub fn halo() -> u32 {
        HALO as u32
    }

    pub fn tile(&self, id: FieldChannelId) -> &GpuFieldTile {
        &self.tiles[id.index()]
    }

    /// 解析摘要字节
    pub fn parse_summary(bytes: &[u8]) -> Option<FieldSummary> {
        if bytes.len() as u64 != Self::SUMMARY_BYTES {
            return None;
        }
        let words: Vec<u32> = bytemuck::pod_collect_to_vec(bytes);
        let diagnostics = [words[0], words[1], words[2], words[3]];
        let mut interior_sums = [0i64; FIELD_CHANNELS];
        for (c, sum) in interior_sums.iter_mut().enumerate() {
            let start = 4 + c * TILE_DIM;
            *sum = words[start..start + TILE_DIM]
                .iter()
                .map(|&w| i64::from(w as i32))
                .sum();
        }
        Some(FieldSummary {
            diagnostics,
            interior_sums,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_layout_parses_signed_row_sums() {
        let mut words = vec![0u32; (GpuFieldMirror::SUMMARY_BYTES / 4) as usize];
        words[0] = 3;
        words[4] = 10; // 通道 0 第 0 行
        words[4 + TILE_DIM - 1] = (-4i32) as u32;
        words[4 + 4 * TILE_DIM] = 7; // 通道 4
        let s = GpuFieldMirror::parse_summary(bytemuck::cast_slice(&words)).unwrap();
        assert_eq!(s.diagnostics, [3, 0, 0, 0]);
        assert_eq!(s.interior_sums, [6, 0, 0, 0, 7]);
        assert!(GpuFieldMirror::parse_summary(&[0u8; 8]).is_none());
    }
}
