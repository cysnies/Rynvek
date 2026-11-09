// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-ecs
//!
//! Rynvek Tier 1 实体与列式存储内核（GAP-04、规范 04）：
//!
//! - [`identity`]：`EntityId` / `EntityHandle` / `GpuEntityRef` 三层标识；
//! - [`registry`]：稀疏-紧凑双表实体注册表与两阶段 Tombstone 回收；
//! - [`soa`]：按页切分的 SoA 列存储（`DomainPage` / `CHUNK_CAPACITY`）。
//!
//! 仅依赖 Tier 0/1（`rynvek-core`、`rynvek-memory`），禁止 `unsafe` 代码。

#![deny(unsafe_code)]

pub mod identity;
pub mod registry;
pub mod soa;

pub use identity::{EntityHandle, EntityId, EntityLifecycleState, EntityTombstone, GpuEntityRef};
pub use registry::{CHUNK_CAPACITY, EntityRegistry, PAGE_CAPACITY, SparsePageEntry};
pub use soa::{ComponentColumn, DomainPage};
