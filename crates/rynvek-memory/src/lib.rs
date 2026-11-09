// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-memory
//!
//! Rynvek Tier 1 内存管理内核（K2 的分配与预算层，D541–D560）：
//!
//! - [`budget`]：四级内存预算状态机；
//! - [`pool`]：64B / 256B 对齐块池；
//! - [`slab`]：固定 32 MiB 的 Slab 分配器；
//! - [`manager`]：统一门面 [`MemoryManager`]。
//!
//! 仅依赖 Tier 0（`rynvek-core`），且禁止 `unsafe` 代码。
//! 因此本 crate 不直接做原始指针分配，而是用 Rust 标准堆上的对齐缓冲池与预算记账，
//! 为上层 ECS / 快照提供可观测、可降级的内存契约。

#![deny(unsafe_code)]

pub mod budget;
pub mod manager;
pub mod pool;
pub mod slab;

pub use budget::{BudgetDecision, MemoryBudget, MemoryBudgetLevel};
pub use manager::MemoryManager;
pub use pool::{ALIGN_64, ALIGN_256, AlignedBlock, AlignedPool};
pub use slab::{SLAB_SIZE_BYTES, SlabAllocator};
