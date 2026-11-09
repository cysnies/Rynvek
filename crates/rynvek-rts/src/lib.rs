// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-rts
//!
//! Rynvek Arena 的 Tier 3 RTS 领域包（项目文档第五章「游戏设计圣经」与规范 09）：
//!
//! - [`ids`] / [`resources`]：稳定文本 ID 与三资源毫单位账本；
//! - [`math`]：定点向量与速率拆分工具；
//! - [`map_gen`] / [`terrain`]：规范冻结的 180° 中心对称地图生成器与五种地形物理属性；
//!
//! 宏观权威层全部使用 `Q32_32` 定点与整数运算（D0 确定性），本 crate 只依赖 Tier 0..=2，且禁止 `unsafe`。

#![deny(unsafe_code)]

pub mod ids;
pub mod map_gen;
pub mod math;
pub mod resources;
pub mod terrain;

pub use ids::{BuildingKind, DamageType, EntitySerial, FactionId, FieldId, OrderKind, ResourceKind, TechId, UnitKind};
pub use map_gen::{TerrainTileKind, generate_symmetric_arena_map};
pub use math::Vec2;
pub use resources::{ResourceAmounts, ResourceLedger};
