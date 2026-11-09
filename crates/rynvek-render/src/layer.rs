// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! L0–L7 WebGPU 渲染层。
//!
//! 文档只冻结了「L0–L7 渲染层」这一名称与数量，各层语义按 Rynvek Industrial Emergence 的绘制需求
//! 冻结如下（ERRATA E-015），编号即自底向上的绘制顺序：
//!
//! | 层 | 名称 | 内容 |
//! |----|------|------|
//! | L0 | Terrain | 深石墨底色、32×32 像素网格与地形 |
//! | L1 | FieldHeatmap | 五通道连续环境场热力图（直接读取 GPU 驻留场缓冲，零回读） |
//! | L2 | Structures | 建筑与资源点（Nexus Core、Harvester 等） |
//! | L3 | Organisms | 生命体与单位簇（GPU Instancing） |
//! | L4 | Bonds | 粘附键、间隙连接与 Kuramoto 同步脉冲环 |
//! | L5 | NeuralOverlay | 选中个体的神经元节点与突触（随 L0–L4 神经 LOD 展开） |
//! | L6 | FogOfWar | 战争迷雾遮罩（服务端裁剪后的可见性） |
//! | L7 | Overlay | 选择框、拾取高亮、信标范围与调试标注 |

/// 渲染层
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RenderLayer {
    Terrain = 0,
    FieldHeatmap = 1,
    Structures = 2,
    Organisms = 3,
    Bonds = 4,
    NeuralOverlay = 5,
    FogOfWar = 6,
    Overlay = 7,
}

impl RenderLayer {
    /// 自底向上的绘制顺序
    pub const ALL: [Self; 8] = [
        Self::Terrain,
        Self::FieldHeatmap,
        Self::Structures,
        Self::Organisms,
        Self::Bonds,
        Self::NeuralOverlay,
        Self::FogOfWar,
        Self::Overlay,
    ];

    /// 由编号还原
    pub const fn from_index(i: u8) -> Option<Self> {
        if (i as usize) < Self::ALL.len() {
            Some(Self::ALL[i as usize])
        } else {
            None
        }
    }

    pub const fn index(self) -> u8 {
        self as u8
    }

    /// 短代号 `L0`–`L7`
    pub const fn code(self) -> &'static str {
        ["L0", "L1", "L2", "L3", "L4", "L5", "L6", "L7"][self as usize]
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Terrain => "terrain",
            Self::FieldHeatmap => "field_heatmap",
            Self::Structures => "structures",
            Self::Organisms => "organisms",
            Self::Bonds => "bonds",
            Self::NeuralOverlay => "neural_overlay",
            Self::FogOfWar => "fog_of_war",
            Self::Overlay => "overlay",
        }
    }

    /// 是否以全屏方式绘制（其余层为实例化图元）
    pub const fn is_fullscreen(self) -> bool {
        matches!(self, Self::Terrain | Self::FieldHeatmap)
    }

    /// 该层图元是否写入拾取目标（迷雾与地形不可拾取）
    pub const fn is_pickable(self) -> bool {
        matches!(
            self,
            Self::Structures | Self::Organisms | Self::Bonds | Self::NeuralOverlay | Self::Overlay
        )
    }
}

/// 渲染层开关位集
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RenderLayerMask(pub u8);

impl RenderLayerMask {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0xFF);

    pub const fn with(self, layer: RenderLayer) -> Self {
        Self(self.0 | (1 << layer as u8))
    }

    pub const fn without(self, layer: RenderLayer) -> Self {
        Self(self.0 & !(1 << layer as u8))
    }

    pub const fn contains(self, layer: RenderLayer) -> bool {
        self.0 & (1 << layer as u8) != 0
    }

    /// 按绘制顺序迭代启用的层
    pub fn iter(self) -> impl Iterator<Item = RenderLayer> {
        RenderLayer::ALL.into_iter().filter(move |l| self.contains(*l))
    }
}

impl Default for RenderLayerMask {
    fn default() -> Self {
        Self::ALL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_layers_in_draw_order() {
        assert_eq!(RenderLayer::ALL.len(), 8);
        for (i, l) in RenderLayer::ALL.iter().enumerate() {
            assert_eq!(l.index() as usize, i);
            assert_eq!(RenderLayer::from_index(i as u8), Some(*l));
            assert_eq!(l.code(), format!("L{i}"));
        }
        assert_eq!(RenderLayer::from_index(8), None);
        assert!(RenderLayer::Terrain < RenderLayer::Overlay);
    }

    #[test]
    fn mask_toggles_layers() {
        let m = RenderLayerMask::ALL.without(RenderLayer::FogOfWar);
        assert!(!m.contains(RenderLayer::FogOfWar));
        assert_eq!(m.iter().count(), 7);
        let only = RenderLayerMask::NONE.with(RenderLayer::Organisms);
        assert_eq!(only.iter().collect::<Vec<_>>(), [RenderLayer::Organisms]);
        assert!(!RenderLayer::FogOfWar.is_pickable());
        assert!(RenderLayer::Organisms.is_pickable());
    }
}
