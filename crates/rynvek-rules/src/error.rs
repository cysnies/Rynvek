// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规则编译与执行错误：统一携带 `RynvekErrorCode`（0x5000 规则/沙箱错误域）。

use std::fmt;

use rynvek_core::error::RynvekErrorCode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleError {
    pub code: RynvekErrorCode,
    /// 出错的规则图节点（若可定位）
    pub node: Option<usize>,
    pub message: String,
}

impl RuleError {
    pub fn new(code: RynvekErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            node: None,
            message: message.into(),
        }
    }

    pub fn at(code: RynvekErrorCode, node: usize, message: impl Into<String>) -> Self {
        Self {
            code,
            node: Some(node),
            message: message.into(),
        }
    }

    pub fn type_error(node: usize, message: impl Into<String>) -> Self {
        Self::at(RynvekErrorCode::RuleStaticTypeCheckFailed, node, message)
    }
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.node {
            Some(n) => write!(f, "{}（节点 #{n}）：{}", self.code, self.message),
            None => write!(f, "{}：{}", self.code, self.message),
        }
    }
}

impl std::error::Error for RuleError {}

pub type RuleResult<T> = Result<T, RuleError>;
