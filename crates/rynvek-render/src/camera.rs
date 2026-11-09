// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 二维正交世界摄像机。
//!
//! 世界坐标以「格」为单位，x 向右、y 向下（与场 Tile 行主序一致）；`px_per_cell` 为缩放（32 即 1:1 像素网格）。
//! 摄像机仅属于表现层，使用 `f32` 不影响任何权威状态（D0 定点约束只作用于仿真）。

use crate::lod::NeuralLod;

/// 世界画布标准网格像素尺寸（Rynvek Industrial Emergence：32×32）
pub const GRID_PIXELS: f32 = 32.0;

/// 二维正交摄像机
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera2D {
    /// 视口中心对应的世界坐标（格）
    pub center: [f32; 2],
    /// 每格像素数
    pub px_per_cell: f32,
    /// 视口像素尺寸
    pub viewport: [u32; 2],
}

impl Camera2D {
    pub const MIN_ZOOM: f32 = 0.25;
    pub const MAX_ZOOM: f32 = 512.0;

    pub fn new(center: [f32; 2], px_per_cell: f32, viewport: [u32; 2]) -> Self {
        Self {
            center,
            px_per_cell: px_per_cell.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM),
            viewport: [viewport[0].max(1), viewport[1].max(1)],
        }
    }

    /// 以 1:1 网格像素观察整块 `dim×dim` 区域中心
    pub fn fit(dim_cells: f32, viewport: [u32; 2]) -> Self {
        let px = viewport[0].min(viewport[1]) as f32 / dim_cells.max(1.0);
        Self::new([dim_cells / 2.0, dim_cells / 2.0], px, viewport)
    }

    /// 当前神经可视化级别
    pub fn lod(&self) -> NeuralLod {
        NeuralLod::for_zoom(self.px_per_cell)
    }

    /// 世界 → 屏幕像素（左上为原点）
    pub fn world_to_screen(&self, world: [f32; 2]) -> [f32; 2] {
        [
            (world[0] - self.center[0]) * self.px_per_cell + self.viewport[0] as f32 / 2.0,
            (world[1] - self.center[1]) * self.px_per_cell + self.viewport[1] as f32 / 2.0,
        ]
    }

    /// 屏幕像素 → 世界
    pub fn screen_to_world(&self, screen: [f32; 2]) -> [f32; 2] {
        [
            (screen[0] - self.viewport[0] as f32 / 2.0) / self.px_per_cell + self.center[0],
            (screen[1] - self.viewport[1] as f32 / 2.0) / self.px_per_cell + self.center[1],
        ]
    }

    /// 以屏幕点为锚缩放（锚点下的世界坐标保持不动）
    pub fn zoom_at(&mut self, screen: [f32; 2], factor: f32) {
        let anchor = self.screen_to_world(screen);
        self.px_per_cell = (self.px_per_cell * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let after = self.screen_to_world(screen);
        self.center[0] += anchor[0] - after[0];
        self.center[1] += anchor[1] - after[1];
    }

    /// 按屏幕像素平移
    pub fn pan_pixels(&mut self, dx: f32, dy: f32) {
        self.center[0] -= dx / self.px_per_cell;
        self.center[1] -= dy / self.px_per_cell;
    }

    /// GPU Uniform 表示
    pub fn uniform(&self) -> CameraUniform {
        CameraUniform {
            center: self.center,
            viewport: [self.viewport[0] as f32, self.viewport[1] as f32],
            px_per_cell: self.px_per_cell,
            grid_pixels: GRID_PIXELS,
            _pad: [0.0; 2],
        }
    }
}

/// 摄像机 Uniform（WGSL `CameraUniform`，32 字节）
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub center: [f32; 2],
    pub viewport: [f32; 2],
    pub px_per_cell: f32,
    pub grid_pixels: f32,
    pub _pad: [f32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_world_roundtrip_and_center() {
        let cam = Camera2D::new([10.0, 20.0], 16.0, [800, 600]);
        assert_eq!(cam.world_to_screen([10.0, 20.0]), [400.0, 300.0]);
        let w = cam.screen_to_world([123.0, 456.0]);
        let s = cam.world_to_screen(w);
        assert!((s[0] - 123.0).abs() < 1e-3 && (s[1] - 456.0).abs() < 1e-3);
    }

    #[test]
    fn zoom_keeps_anchor_fixed_and_changes_lod() {
        let mut cam = Camera2D::new([0.0, 0.0], 4.0, [100, 100]);
        assert_eq!(cam.lod(), NeuralLod::L1);
        let anchor = cam.screen_to_world([80.0, 20.0]);
        cam.zoom_at([80.0, 20.0], 16.0);
        let after = cam.screen_to_world([80.0, 20.0]);
        assert!((anchor[0] - after[0]).abs() < 1e-4 && (anchor[1] - after[1]).abs() < 1e-4);
        assert_eq!(cam.lod(), NeuralLod::L3);
        cam.zoom_at([0.0, 0.0], 1e9);
        assert_eq!(cam.px_per_cell, Camera2D::MAX_ZOOM);
        assert_eq!(std::mem::size_of::<CameraUniform>(), 32);
    }
}
