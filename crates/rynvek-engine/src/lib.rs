// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-engine
//!
//! Rynvek Engine 顶层门面（Tier 4）：组合 `rynvek-simulation`（CPU 权威）、`rynvek-rts`（竞技场权威世界）、
//! `rynvek-wgpu`（独占设备与核心内核）与 `rynvek-render`（世界渲染），对 `rynvek-wasm` / 原生宿主提供统一的
//! [`Engine`]、[`WorldRuntime`] 与 Worker 消息协议。
//!
//! - [`arena`]：Rynvek Arena 对局会话（`rynvek-rts` + `rynvek-agent`）与 `rynvek-arena/2` 客户端协议；
//! - [`engine`]：主循环、GPU 驻留镜像核对与 DeviceLost 5 步恢复编排；
//! - [`protocol`]：`ENGINE_INIT` 等主线程 ⇄ Worker 消息；
//! - [`gpu_field`] / [`gpu_neural`] / `gpu_rules`：场、LIF 神经与规则 WGSL 后端的 GPU 宿主适配（CPU/GPU 差分测试对象）；
//! - [`provider`]：内置只读可视化提供者（`EntityProvider` / `ArenaVisualizationProvider`）与双世界统一门面 [`WorldRuntime`]；
//! - `plugins`（特性 `plugins`）：RTS ⇄ `rynvek:plugin@1.0.0` WASM 插件适配层。

#![deny(unsafe_code)]

pub mod arena;
pub mod engine;
pub mod gpu_field;
pub mod gpu_neural;
pub mod observation;
// 规则内核 GPU 执行器目前只提供阻塞回读（差分测试用），浏览器目标不编译
#[cfg(not(target_arch = "wasm32"))]
pub mod gpu_rules;
#[cfg(all(feature = "plugins", not(target_arch = "wasm32")))]
pub mod plugins;
pub mod protocol;
pub mod provider;

pub use engine::{Engine, EngineConfig, EngineError, EngineStats, FaultInjection};
pub use protocol::{ComputeBackendKind, EngineCommand, EngineEvent, GpuMode};
pub use provider::{ArenaVisualizationProvider, EntityProvider, WorldRuntime};

/// 与服务器共用的 Arena 领域权威事务。
pub use rynvek_rts::ArenaRuntime;
