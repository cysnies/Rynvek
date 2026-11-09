// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-storage
//!
//! Rynvek Tier 1 存储抽象（D130–D140 / D525–D540）：
//!
//! - [`blob`]：内容寻址（BLAKE3）不可变 Blob 存储；
//! - [`engine`]：[`StorageEngine`] Trait 与内存实现；
//! - `sqlite`：本地 SQLite / OPFS 对等的 DDL 迁移执行器（仅非 wasm32 目标）；
//! - [`migrations`]：嵌入的 PostgreSQL 17 与 SQLite DDL 文本。
//!
//! 仅依赖 Tier 0。PostgreSQL 运行时驱动留待服务端里程碑；M1 提供完整 DDL 与可执行的 SQLite 适配。

#![deny(unsafe_code)]

pub mod blob;
pub mod engine;
pub mod migrations;
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite;

#[cfg(not(target_arch = "wasm32"))]
pub use blob::FsBlobStore;
pub use blob::{BlobId, BlobStore, MemoryBlobStore};
pub use engine::{MemoryStorageEngine, StorageEngine, WorldCommitRecord, WorldMeta};
#[cfg(not(target_arch = "wasm32"))]
pub use sqlite::SqliteStorageEngine;
