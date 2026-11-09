// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Agent 侧错误：一律映射到规范 13 的 [`RynvekErrorCode`] 稳定码（ERRATA E-030）。
//!
//! | 场景 | 错误码 |
//! |---|---|
//! | 租约缺失 / 过期 / Fencing Token 失效 / 被人类接管 | `0x8001 UnauthorizedCapabilityToken` |
//! | 单次调用超过 `2,000 ms` | `0x8003 CommandExpiredOrFutureTooFar` |
//! | 未满 `30 Ticks` 最小间隔或超出 `600 APM` | `0x8004 CommandRateLimitExceeded` |
//! | 未知工具 / 参数不符合 JSON Schema | `0x5001 RuleStaticTypeCheckFailed` |
//! | 控制等级不足以调用该工具 | `0x5006 PluginCapabilityDenied` |
//!
//! 宿主执行期错误（资源不足 `0x8005`、放置非法 `0x8006` 等）原样透传。

use core::fmt;

use rynvek_core::RynvekErrorCode;
use serde::{Deserialize, Serialize};

/// Agent 调用错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentError {
    pub code: RynvekErrorCode,
    pub message: String,
}

impl AgentError {
    pub fn new(code: RynvekErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// `0x5001`：参数不符合 Schema 或未知工具
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::RuleStaticTypeCheckFailed, message)
    }

    /// `0x8001`：租约 / 授权失败
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::UnauthorizedCapabilityToken, message)
    }

    /// `0x5006`：控制等级不足
    pub fn capability_denied(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::PluginCapabilityDenied, message)
    }

    /// `0x8004`：限流
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::CommandRateLimitExceeded, message)
    }

    /// `0x8003`：超时
    pub fn timed_out(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::CommandExpiredOrFutureTooFar, message)
    }

    /// 线上传输形态
    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            code: self.code.code(),
            name: format!("{:?}", self.code),
            message: self.message.clone(),
        }
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:#06x}] {}", self.code.code(), self.message)
    }
}

impl std::error::Error for AgentError {}

/// 返回给 LLM 的错误体（`code` 为十进制数值，`name` 为错误码变体名）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: u16,
    pub name: String,
    pub message: String,
}
