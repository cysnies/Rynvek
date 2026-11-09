// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-wgpu
//!
//! `wgpu 30.0.1` 设备独占所有者与 GPU 计算基础设施（规范 07 §7.1，ADR-010–012 / ADR-015）：
//!
//! - [`device`]：[`GpuDeviceOwner`]——进程（浏览器中为 Dedicated Engine Worker）内唯一持有
//!   `wgpu::Instance / Adapter / Device / Queue` 的对象，负责 DeviceLost 侦测与重新获取设备；
//! - [`profile`]：由 `adapter.limits()` 判定 `ComputeCapabilityProfile`（Tier A–D）；
//! - [`segment`]：`32 MiB` 分段缓冲池 [`SegmentedBufferPool`]（严禁 `> 128 MiB` 单体存储缓冲）；
//! - [`readback`]：`3-Slot Readback Ring`（`Slot[tick % 3]`，`map_async` 非阻塞回读）；
//! - [`binding`]：BindGroup 0–3 的冻结约定；
//! - [`shader`]：WGSL 核心着色器库（`shaders/`）与 naga 预校验（`WgslShaderValidationFailed`）；
//! - [`kernel`] / [`kernels`]：计算管线构建与 Philox / 场扩散 / LIF 核心内核；
//! - [`recovery`]：DeviceLost 5 步恢复状态机（失败则降级至 CPU Reference）。

#![deny(unsafe_code)]

pub mod binding;
pub mod error;
pub mod profile;
pub mod readback;
pub mod segment;

pub use binding::{BindGroupSlot, BindingKind, BindingSpec, DIAGNOSTICS_BINDING};
pub use error::{GpuError, GpuResult};
pub use profile::{AdapterCapabilitySummary, ComputeCapabilityProfile, classify_compute_capability};
pub use readback::{READBACK_RING_SLOTS, ReadbackRing, ReadbackRingState, ReadbackSlotState};
pub use segment::{MAX_MONOLITHIC_STORAGE_BUFFER_BYTES, SEGMENT_SIZE_BYTES, SegmentPlan, SegmentedBufferPool};

/// 线性 Workgroup 尺寸（规范 01 §1.1：`128×1×1`）
pub const LINEAR_WORKGROUP_SIZE: u32 = 128;
/// 二维网格 Workgroup 边长（规范 01 §1.1：`8×8×1`）
pub const GRID_WORKGROUP_EDGE: u32 = 8;

/// 线性调度所需的 Workgroup 数
pub const fn linear_workgroups(items: u32) -> u32 {
    items.div_ceil(LINEAR_WORKGROUP_SIZE)
}

/// 二维调度所需的 Workgroup 数（每维）
pub const fn grid_workgroups(edge_items: u32) -> u32 {
    edge_items.div_ceil(GRID_WORKGROUP_EDGE)
}
