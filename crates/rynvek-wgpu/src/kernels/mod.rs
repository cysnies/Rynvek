// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 核心计算内核的宿主侧封装。
//!
//! 本模块只处理原始定点数组（Q8.8 以 `i32` 承载），不依赖同层的 `rynvek-field` / `rynvek-neural`；
//! 与 CPU Reference 的逐位差分由 Tier 4 的 `rynvek-engine` 完成。

pub mod field;
pub mod lif;
pub mod philox;

pub use field::{
    CFL_MAX_DIFFUSION_Q8, FieldKernels, FieldSubstep, GpuFieldParams, GpuFieldTile, field_substep_schedule,
};
pub use lif::{GpuLifBatch, LifBatchState, LifKernels};
pub use philox::PhiloxKernel;

/// 将 Q8.8 `i16` 样本扩展为 GPU 的 `i32` 承载
pub fn widen_i16(values: &[i16]) -> Vec<i32> {
    values.iter().map(|&v| i32::from(v)).collect()
}

/// 将 GPU 回读的 `i32` 收窄为 `i16`（越界说明内核违反钳制约定，按饱和处理并由诊断计数器另行报告）
pub fn narrow_i32(values: &[i32]) -> Vec<i16> {
    values
        .iter()
        .map(|&v| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
        .collect()
}
