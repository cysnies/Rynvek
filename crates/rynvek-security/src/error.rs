// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 安全层错误：稳定错误码 + 便于审计日志的人类可读说明。

use core::fmt;

use rynvek_core::error::RynvekErrorCode;

/// 安全检查失败
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityError {
    pub code: RynvekErrorCode,
    pub message: String,
}

impl SecurityError {
    pub fn new(code: RynvekErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// `0x8001`：令牌、身份或权限不足
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::UnauthorizedCapabilityToken, message)
    }

    /// `0x9001`：Ed25519 签名无效
    pub fn bad_signature(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::InvalidEd25519Signature, message)
    }
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:#06x}): {}", self.code, self.code.code(), self.message)
    }
}

impl std::error::Error for SecurityError {}
