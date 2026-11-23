// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-server
//!
//! Rynvek 的 Tier 4 权威服务端（规范 11、D202–D208、D328–D333、D645–D657）：
//!
//! - [`api`]：规范 11.3 的 20 条 HTTP / WebSocket 路由（axum），错误一律为规范 11.4 稳定码；
//! - [`stream`] / [`host`]：双栈实时流握手与每局一个的权威对局 Actor（命令闸门、迷雾裁剪复制、终局回放）；
//! - [`matchmaking`] / [`glicko2`]：按赛道与规则集分桶的匹配队列与 Glicko-2 天梯；
//! - [`replay`]：签名 `.rreplay`（`RYRP`）的录制与重新仿真验证；
//! - [`store`]：与规范 DDL 一一对应的存储层（内存 / PostgreSQL 17）；
//! - [`source`]：AGPLv3 §13 源码要约与 SPDX 2.3 SBOM；
//! - [`server`]：组装 HTTP + WebTransport 监听的启动入口。

#![deny(unsafe_code)]

pub mod agent_package;
pub mod audio_package;
pub mod agent_runs;
pub mod api;
pub mod app;
pub mod browser;
pub mod client;
pub mod error;
pub mod glicko2;
pub mod host;
pub mod ids;
pub mod lab_jobs;
pub mod local;
pub mod matchmaking;
pub mod patch;
pub mod ranked;
pub mod replay;
pub mod server;
pub mod source;
pub mod store;
pub mod stream;
pub mod verify;

pub use app::{AppConfig, AppState};
pub use error::{ApiError, ApiResult};
pub use server::{RunningServer, ServerConfig, start};

pub mod transactions;

pub mod visual_package;
