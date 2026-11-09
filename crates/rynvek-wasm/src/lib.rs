// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-wasm
//!
//! Rynvek Tier 4 浏览器 Dedicated Engine Worker 入口（规范 07 §7.1 第 1 条）：
//!
//! - [`scheduler`]：固定步长 Tick 调度（帧时间 → 整数 Tick，带追赶上限）；
//! - [`host`]：与平台无关的 Worker 宿主状态机（`ENGINE_INIT` 仅一次、初始化期命令缓存、关闭）；
//! - `bridge`（仅 `wasm32`）：wasm-bindgen ABI 桥，接管 OffscreenCanvas、在唯一 `wgpu::Instance` 上
//!   创建 Surface，并以异步方式驱动 DeviceLost 第 3 步“重获设备”。
//!
//! 除 `bridge` 外全部代码 `#![deny(unsafe_code)]`。

#![deny(unsafe_code)]

pub mod host;
pub mod scheduler;

// ABI 桥：wasm-bindgen 宏展开的导出符号需要放开 unsafe_code（见模块文档）
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod bridge;

#[cfg(target_arch = "wasm32")]
pub use bridge::EngineWorker;
pub use host::{HostPhase, InitPlan, WorkerHost, events_to_json};
pub use scheduler::TickScheduler;
