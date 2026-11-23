// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-world
//!
//! Tier 3 World 包（规范 08 / 10）：
//!
//! - [`package`]：`.rworld`（RYWL）/ `.rpack`（RYPK）/ `.rview`（RYVW）共用的 64 字节头容器，
//!   与 `@rynvek/assets` 的 E-019 实现逐字节兼容；
//! - [`rworld`]：`.rworld` 世界包（`world.json` 世界配置 + 可选 `dependencies.lock`）解析为
//!   [`rynvek_rts::MatchConfig`] 并创建权威世界；
//! - [`wgsl_review`]：`.rview` 着色器静态审查（与 `@rynvek/assets` 同规则）与 naga 校验；
//! - [`authoring`]：`WorldPatch` 四级安全分类校验与对 `ArenaWorld` 的真实运行时补丁应用闭环；
//! - [`partition`]：`WorldProfile`、`PartitionCoord` 与跨分区两阶段原子移交协议 `EntityHandoffCoordinator`；
//! - [`federation`]：跨世界联邦星门网关 `FederatedWorldGateway` 与基因组入境检疫。

#![deny(unsafe_code)]

pub mod authoring;
pub mod federation;
// 容器唯一实现下沉内核，World与权威规则共用，避免依赖环与重复解析器。
pub use rynvek_core::package;
pub mod partition;
pub mod rworld;
pub mod wgsl_review;

pub use authoring::{HlcTimestamp, LiveEditSafetyClass, PatchApplyReport, WorldPatch, WorldPatchOp};
pub use federation::{CrossWorldMigrationTicket, FederatedWorldGateway, PortalLink};
pub use package::{
    Package, PackageEntry, PackageError, PackageInput, PackageInputEntry, PackageKind, PackageManifest, read_package,
    write_package,
};
pub use partition::{EntityHandoffCoordinator, EntityHandoffEnvelope, HandoffState, PartitionCoord, WorldProfile};
pub use rworld::{WorldBundle, WorldConfigFile};
