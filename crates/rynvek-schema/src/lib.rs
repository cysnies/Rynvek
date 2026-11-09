// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-schema
//!
//! Rynvek 的 Tier 0 Schema 基础设施：
//!
//! - [`canonical_cbor`]：RFC 8949 核心确定性 CBOR 编码；
//! - [`canonical_json`]：RFC 8785 Canonical JSON 序列化；
//! - [`migration`]：Schema 有向迁移图 `SchemaMigrationGraph` 与确定性 BFS 最短路径搜索；
//! - [`registry`]：按 `(领域, 版本)` 登记 Schema 并执行原子迁移的 `SchemaRegistry`。
//!
//! 本 crate 不依赖任何内部业务 crate，且禁止 `unsafe` 代码。

#![deny(unsafe_code)]

pub mod canonical_cbor;
pub mod canonical_json;
pub mod migration;
pub mod registry;

pub use migration::{MigrationEdge, SchemaMigrationGraph, SchemaRevisionId};
pub use registry::{RegisteredSchema, SchemaError, SchemaFingerprint, SchemaRegistry};
