// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! GPU 侧错误：统一映射到规范 11.4 的 `0x6000` 段错误码。

use core::fmt;

use rynvek_core::error::RynvekErrorCode;

/// 携带统一错误码与诊断信息的 GPU 错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuError {
    pub code: RynvekErrorCode,
    pub message: String,
}

pub type GpuResult<T> = Result<T, GpuError>;

impl GpuError {
    pub fn new(code: RynvekErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// `WebGpuAdapterUnavailable (0x6001)`
    pub fn adapter_unavailable(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::WebGpuAdapterUnavailable, message)
    }

    /// `GpuDeviceLost (0x6002)`
    pub fn device_lost(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::GpuDeviceLost, message)
    }

    /// `GpuBufferAllocationFailed (0x6003)`
    pub fn allocation(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::GpuBufferAllocationFailed, message)
    }

    /// `WgslShaderValidationFailed (0x6004)`
    pub fn shader(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::WgslShaderValidationFailed, message)
    }
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GpuError {}
