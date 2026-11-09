// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! HTTP API 错误：一律以规范 11.4 [`RynvekErrorCode`] 稳定码返回。
//!
//! 响应体：`{ "code": 32769, "code_hex": "0x8001", "name": "UnauthorizedCapabilityToken", "message": "…" }`。
//! HTTP 状态码只是传输层提示，客户端应以 `code` 为准（ERRATA 记录映射表）。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rynvek_core::error::RynvekErrorCode;
use rynvek_security::SecurityError;
use serde::Serialize;

/// API 错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub code: RynvekErrorCode,
    pub status: StatusCode,
    pub message: String,
}

/// 错误响应体
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    pub code: u16,
    pub code_hex: String,
    pub name: String,
    pub message: String,
}

/// 错误码 → 缺省 HTTP 状态
pub fn default_status(code: RynvekErrorCode) -> StatusCode {
    use RynvekErrorCode as C;
    match code {
        C::UnauthorizedCapabilityToken => StatusCode::FORBIDDEN,
        C::DuplicateCommandIdempotencyKey | C::DependencyLockConflict | C::StructuralMutationConflict => {
            StatusCode::CONFLICT
        }
        C::CommandExpiredOrFutureTooFar
        | C::InsufficientFactionResources
        | C::InvalidBuildingPlacementGrid
        | C::UnsafeRviewShaderRejected
        | C::RuleStaticTypeCheckFailed => StatusCode::UNPROCESSABLE_ENTITY,
        C::CommandRateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
        C::InvalidEd25519Signature | C::InvalidFileMagicOrVersion | C::Blake3ChecksumMismatch => {
            StatusCode::BAD_REQUEST
        }
        C::RevokedPackageBlocked => StatusCode::GONE,
        C::EntityNotFoundOrTombstoned => StatusCode::NOT_FOUND,
        C::WorldMemoryBudgetExceeded => StatusCode::INSUFFICIENT_STORAGE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl ApiError {
    pub fn new(code: RynvekErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            status: default_status(code),
            message: message.into(),
        }
    }

    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// 缺少或无效凭证：`0x8001` + 401
    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::UnauthorizedCapabilityToken, message).with_status(StatusCode::UNAUTHORIZED)
    }

    /// 已认证但无权：`0x8001` + 403
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::UnauthorizedCapabilityToken, message)
    }

    /// 资源不存在：`0x2002` + 404
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::EntityNotFoundOrTombstoned, message)
    }

    /// 请求体不符合契约：`0x5001`（与 `rynvek-schema` 的 Schema 校验失败同码）+ 400
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::RuleStaticTypeCheckFailed, message).with_status(StatusCode::BAD_REQUEST)
    }

    /// 存储后端故障
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::StructuralMutationConflict, message).with_status(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            code: self.code.code(),
            code_hex: format!("{:#06x}", self.code.code()),
            name: format!("{:?}", self.code),
            message: self.message.clone(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({:#06x}): {}", self.code, self.code.code(), self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<SecurityError> for ApiError {
    fn from(e: SecurityError) -> Self {
        Self::new(e.code, e.message)
    }
}

impl From<RynvekErrorCode> for ApiError {
    fn from(code: RynvekErrorCode) -> Self {
        Self::new(code, code.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body())).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
