// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! L0–L4 五级多尺度神经可视化（项目文档 6.1：随摄像机缩放无缝过渡）。
//!
//! 文档冻结了五级的内容，未给出切换阈值；此处以「每世界格像素数」冻结阈值（ERRATA E-015）。
//! 32×32 像素网格下 `px_per_cell = 32` 即 1:1 原始像素尺寸：
//!
//! | 级别 | `px_per_cell` | 内容 |
//! |------|---------------|------|
//! | L0 | `< 2` | 群体活动密度热力云与阵营控制边界 |
//! | L1 | `[2, 8)` | 单位簇几何外框、朝向与同步脉冲环 |
//! | L2 | `[8, 32)` | 个体间粘附键与间隙连接光束 |
//! | L3 | `[32, 128)` | 选中生命体内部神经元节点与兴奋/抑制突触图 |
//! | L4 | `≥ 128` | Inspector 浮层单突触波形（膜电位、权重、资格迹） |

use crate::layer::{RenderLayer, RenderLayerMask};

/// 神经可视化级别
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NeuralLod {
    L0 = 0,
    L1 = 1,
    L2 = 2,
    L3 = 3,
    L4 = 4,
}

impl NeuralLod {
    pub const ALL: [Self; 5] = [Self::L0, Self::L1, Self::L2, Self::L3, Self::L4];
    /// 各级下限阈值（每格像素数）
    pub const THRESHOLDS: [f32; 5] = [0.0, 2.0, 8.0, 32.0, 128.0];

    /// 由缩放倍率选级
    pub fn for_zoom(px_per_cell: f32) -> Self {
        let mut lod = Self::L0;
        for (l, t) in Self::ALL.iter().zip(Self::THRESHOLDS) {
            if px_per_cell >= t {
                lod = *l;
            }
        }
        lod
    }

    /// 该级别默认启用的渲染层
    pub fn visible_layers(self) -> RenderLayerMask {
        let base = RenderLayerMask::NONE
            .with(RenderLayer::Terrain)
            .with(RenderLayer::FieldHeatmap)
            .with(RenderLayer::Structures)
            .with(RenderLayer::FogOfWar)
            .with(RenderLayer::Overlay);
        match self {
            // 远景只看热力云：个体不逐一绘制
            Self::L0 => base,
            Self::L1 => base.with(RenderLayer::Organisms),
            Self::L2 => base.with(RenderLayer::Organisms).with(RenderLayer::Bonds),
            Self::L3 | Self::L4 => base
                .with(RenderLayer::Organisms)
                .with(RenderLayer::Bonds)
                .with(RenderLayer::NeuralOverlay),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_thresholds_select_levels() {
        assert_eq!(NeuralLod::for_zoom(0.5), NeuralLod::L0);
        assert_eq!(NeuralLod::for_zoom(2.0), NeuralLod::L1);
        assert_eq!(NeuralLod::for_zoom(31.9), NeuralLod::L2);
        assert_eq!(NeuralLod::for_zoom(32.0), NeuralLod::L3);
        assert_eq!(NeuralLod::for_zoom(1000.0), NeuralLod::L4);
    }

    #[test]
    fn deeper_levels_reveal_more_layers() {
        assert!(!NeuralLod::L0.visible_layers().contains(RenderLayer::Organisms));
        assert!(NeuralLod::L1.visible_layers().contains(RenderLayer::Organisms));
        assert!(!NeuralLod::L2.visible_layers().contains(RenderLayer::NeuralOverlay));
        assert!(NeuralLod::L3.visible_layers().contains(RenderLayer::NeuralOverlay));
    }
}
