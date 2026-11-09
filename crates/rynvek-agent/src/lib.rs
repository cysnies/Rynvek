// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-agent
//!
//! Rynvek Tier 3 控制协议包（规范 09.1，GAP-26，D163–D167，D216–D232）：
//!
//! - [`error`]：Agent 侧错误（携带 [`RynvekErrorCode`](rynvek_core::RynvekErrorCode) 稳定码）；
//! - [`schema`]：`protocols/schemas/agent-tools.v1.json`（规范 9.1 原文）与 Draft 2020-12 子集校验器；
//! - [`tools`]：6 个标准受控工具的强类型调用 [`ToolCall`]；
//! - [`control`]：`Principal`、五级 [`ControlLevel`] 与带 Fencing Token 的 [`ControlLease`]（人类无感接管）；
//! - [`apm`]：`600 APM` 公平限流窗口；
//! - [`memory`]：按 Principal 隔离的 64 条环形 [`AgentEpisodicMemory`]；
//! - [`broker`]：[`ToolBroker`]（单次 `2,000 ms` 超时、最小间隔 `30 Ticks`）与宿主适配接口 [`ToolHost`]。
//!
//! 本 crate 只依赖 Tier 0..=2，不依赖 `rynvek-rts`：工具语义由 Tier 4 宿主（`rynvek-engine::arena`）实现 [`ToolHost`]。

#![deny(unsafe_code)]

pub mod apm;
pub mod broker;
pub mod control;
pub mod error;
pub mod memory;
pub mod schema;
pub mod tools;

pub use apm::{APM_MAX_ACTIONS, APM_WINDOW_TICKS, ApmLimiter};
pub use broker::{
    ManualClock, MonotonicClock, SystemClock, TOOL_CALL_TIMEOUT_MS, TOOL_MIN_INTERVAL_TICKS, ToolBroker, ToolContext,
    ToolHost, ToolRequest, ToolResponse,
};
pub use control::{ControlLease, ControlLevel, LeaseManager, Principal, PrincipalKind};
pub use error::AgentError;
pub use memory::{AGENT_MEMORY_CAPACITY, AgentEpisodicMemory, EpisodeOutcome, EpisodeRecord};
pub use schema::{AGENT_TOOLS_V1_JSON, ToolRegistry, ToolSchema};
pub use tools::{CrucibleToolAction, ToolCall, ToolName};
