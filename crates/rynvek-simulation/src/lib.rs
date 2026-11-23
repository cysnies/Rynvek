// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-simulation
//!
//! 16 阶段标准 Tick 执行引擎（规范 03）：
//!
//! - [`phase`]：`CanonicalPhaseId` 与 `TickCommitManifest`；
//! - [`invariant`]：`INV-01..05` 注册表；
//! - [`conflict`]：`ConflictPolicy` 确定性归约；
//! - [`clock`]：多速率时钟（Tick / Coupling Epoch）；
//! - [`engine`]：按固定拓扑推进并提交状态根。

#![deny(unsafe_code)]

pub mod clock;
pub mod conflict;
pub mod engine;
pub mod invariant;
pub mod phase;

pub use clock::MultiRateClock;
pub use conflict::{ConflictPolicy, ConflictResolution};
pub use engine::{SimulationEngine, SimulationError};
pub use invariant::{InvariantId, InvariantRegistry, InvariantReport};
pub use phase::{CanonicalPhaseId, KERNEL_IMMUTABLE_PHASES, TickCommitManifest};

pub mod life;
pub mod rules;
pub mod persistence;
pub mod physics;
