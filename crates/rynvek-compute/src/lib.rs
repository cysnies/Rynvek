// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-compute
//!
//! 异构计算抽象层（K1，ADR-010–012）：
//!
//! - [`capability`]：硬件能力档位 `CapabilityProfile`（Tier A–D）；
//! - [`backend`]：[`ComputeBackend`] Trait；
//! - [`kernel`]：内核注册表 [`KernelRegistry`]。
//!
//! 具体后端由 `rynvek-compute-cpu`（黄金参照）与后续 `rynvek-wgpu` 提供。

#![deny(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod kernel;

pub use backend::{ComputeBackend, DispatchError};
pub use capability::CapabilityProfile;
pub use kernel::{KernelId, KernelRegistry, KernelSignature};
