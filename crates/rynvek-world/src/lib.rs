// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-world
//!
//! Tier 3 World 包。规范把 “World 生命周期、Partition/Halo 协调、Fork/Patch/Merge、`.rworld` 加载”
//! 划归本 crate；M8 落地其中的分发部分（ERRATA E-049）：
//!
//! - [`package`]：`.rworld`（RYWL）/ `.rpack`（RYPK）/ `.rview`（RYVW）共用的 64 字节头容器，
//!   与 `@rynvek/assets` 的 E-019 实现逐字节兼容；
//! - [`rworld`]：`.rworld` 世界包（`world.json` 世界配置 + 可选 `dependencies.lock`）解析为
//!   [`rynvek_rts::MatchConfig`] 并创建权威世界。
//!
//! Arena 对局的世界生命周期仍由 `rynvek-rts` 与 `rynvek-server` 承担；Partition/Halo 与 Fork/Merge
//! 属于通用模拟世界，不在 M8 范围内。

#![deny(unsafe_code)]

pub mod package;
pub mod rworld;

pub use package::{
    Package, PackageEntry, PackageError, PackageInput, PackageInputEntry, PackageKind, PackageManifest, read_package,
    write_package,
};
pub use rworld::{WorldBundle, WorldConfigFile};
