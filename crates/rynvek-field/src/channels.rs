// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 标准五通道环境场参数与多通道 Tile（项目文档 §5.2.2 / 规范 06）。

use crate::solver::{STRIDE, TILE_DIM};

/// 规范命名的标准场通道标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FieldChannelId {
    Nutrient = 0,
    Energy = 1,
    SignalA = 2,
    SignalB = 3,
    Hazard = 4,
}

impl FieldChannelId {
    pub const ALL: [FieldChannelId; 5] = [Self::Nutrient, Self::Energy, Self::SignalA, Self::SignalB, Self::Hazard];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nutrient => "nutrient",
            Self::Energy => "energy",
            Self::SignalA => "signal_a",
            Self::SignalB => "signal_b",
            Self::Hazard => "hazard",
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// 单通道物理参数（Q8.8）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldChannelParams {
    pub id: FieldChannelId,
    /// 扩散率 D_c（Q8.8）；可大于 64，由 CFL 子步拆分
    pub diffusion_q8: i32,
    /// 衰减率 λ_c（Q8.8）
    pub decay_q8: i32,
    pub min_val_q8: i16,
    pub max_val_q8: i16,
}

/// 项目文档给出的五通道默认参数（扩散/衰减为示意量级，单位 Q8.8）
///
/// 说明：文档以物理浮点描述；此处取位级可复现的定点近似：
/// - nutrient: D≈0.20 → 51, λ≈0.01 → 3
/// - energy:   D≈0.15 → 38, λ≈0.02 → 5
/// - signal_a: D≈0.25 → 64, λ≈0.05 → 13
/// - signal_b: D≈0.25 → 64, λ≈0.05 → 13
/// - hazard:   D≈0.10 → 26, λ≈0.00 → 0
pub const STANDARD_FIELD_CHANNELS: [FieldChannelParams; 5] = [
    FieldChannelParams {
        id: FieldChannelId::Nutrient,
        diffusion_q8: 51,
        decay_q8: 3,
        min_val_q8: 0,
        max_val_q8: i16::MAX,
    },
    FieldChannelParams {
        id: FieldChannelId::Energy,
        diffusion_q8: 38,
        decay_q8: 5,
        min_val_q8: 0,
        max_val_q8: i16::MAX,
    },
    FieldChannelParams {
        id: FieldChannelId::SignalA,
        diffusion_q8: 64,
        decay_q8: 13,
        min_val_q8: i16::MIN,
        max_val_q8: i16::MAX,
    },
    FieldChannelParams {
        id: FieldChannelId::SignalB,
        diffusion_q8: 64,
        decay_q8: 13,
        min_val_q8: i16::MIN,
        max_val_q8: i16::MAX,
    },
    FieldChannelParams {
        id: FieldChannelId::Hazard,
        diffusion_q8: 26,
        decay_q8: 0,
        min_val_q8: 0,
        max_val_q8: i16::MAX,
    },
];

/// 单通道双缓冲 Tile（含 Halo）
#[derive(Clone)]
pub struct FieldTileQ8 {
    pub buffers: Box<[[i16; STRIDE * STRIDE]; 2]>,
    pub current: usize,
    pub params: FieldChannelParams,
    pub sources: Box<[i16; TILE_DIM * TILE_DIM]>,
}

impl FieldTileQ8 {
    pub fn new(params: FieldChannelParams) -> Self {
        Self {
            buffers: Box::new([[0i16; STRIDE * STRIDE]; 2]),
            current: 0,
            params,
            sources: Box::new([0i16; TILE_DIM * TILE_DIM]),
        }
    }

    pub fn current_buf(&self) -> &[i16; STRIDE * STRIDE] {
        &self.buffers[self.current]
    }

    pub fn current_buf_mut(&mut self) -> &mut [i16; STRIDE * STRIDE] {
        &mut self.buffers[self.current]
    }
}

impl std::fmt::Debug for FieldTileQ8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldTileQ8")
            .field("current", &self.current)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}
