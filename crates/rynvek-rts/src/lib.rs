// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-rts
//!
//! Rynvek Arena 的 Tier 3 RTS 领域包（项目文档第五章「游戏设计圣经」与规范 09）：
//!
//! - [`ids`] / [`resources`]：稳定文本 ID 与三资源毫单位账本；
//! - [`math`]：定点向量与速率拆分工具；
//! - [`map_gen`] / [`terrain`]：规范冻结的 180° 中心对称地图生成器与五种地形物理属性；
//! - [`arena_map`]：四种标准尺寸、出生锚点、对称性校验与「2 条 ≥ 6 宽通路」最小顶点割验证；
//! - [`buildings`] / [`units`] / [`tech`] / [`combat`]：设计圣经冻结的建筑、单位、科技平衡表与伤害克制；
//! - [`genome`] / [`brain`] / [`evolution`]：6 大种子基因组、单位神经脑表达与演化算子；
//! - [`fields`] / [`modulators`]：五层连续环境场与 L3 信标调质广播；
//! - [`nav`]：HPA* + Eikonal 流场 + 定点 ORCA 三级导航；
//! - [`fog`]：对称阴影投射战争迷雾；
//! - [`pve`]：PvE 自适应演化虫潮；
//! - [`commands`] / [`world`] / [`observation`]：权威命令、`ArenaWorld` 对局状态机与迷雾裁剪观测；
//!
//! 宏观权威层全部使用 `Q32_32` 定点与整数运算（D0 确定性），本 crate 只依赖 Tier 0..=2，且禁止 `unsafe`。

#![deny(unsafe_code)]

pub mod arena_map;
pub mod brain;
pub mod buildings;
pub mod combat;
pub mod commands;
pub mod evolution;
pub mod fields;
pub mod fog;
pub mod genome;
pub mod ids;
pub mod lab;
pub mod map_gen;
pub mod math;
pub mod modulators;
pub mod nav;
pub mod observation;
pub mod pve;
pub mod resources;
pub mod tech;
pub mod terrain;
pub mod units;
pub mod world;

pub use arena_map::{ArenaMap, ArenaMapSize, ConnectivityReport};
pub use ids::{BuildingKind, DamageType, EntitySerial, FactionId, FieldId, OrderKind, ResourceKind, TechId, UnitKind};
pub use map_gen::{TerrainTileKind, generate_symmetric_arena_map};
pub use math::Vec2;
pub use resources::{ResourceAmounts, ResourceLedger};
pub use world::{ArenaWorld, MatchConfig, MatchMode, MatchOutcome, RecordedCommand, RtsEvent};
