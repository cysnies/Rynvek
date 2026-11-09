// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! [`ComputeBackend`]：CPU / GPU 统一调度入口。

use core::fmt;

use rynvek_core::error::RynvekErrorCode;

use crate::capability::CapabilityProfile;
use crate::kernel::{KernelId, KernelRegistry};

/// 调度失败
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    UnknownKernel,
    ProfileTooLow {
        have: CapabilityProfile,
        need: CapabilityProfile,
    },
    Backend(RynvekErrorCode),
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKernel => write!(f, "unknown compute kernel"),
            Self::ProfileTooLow { have, need } => {
                write!(
                    f,
                    "capability {} cannot run kernel requiring {}",
                    have.as_str(),
                    need.as_str()
                )
            }
            Self::Backend(code) => write!(f, "backend error {code}"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// 计算后端 Trait
pub trait ComputeBackend: Send + Sync {
    /// 后端显示名
    fn name(&self) -> &'static str;

    /// 当前能力档位
    fn profile(&self) -> CapabilityProfile;

    /// 内核注册表
    fn kernels(&self) -> &KernelRegistry;

    /// 调度一维内核：`work_items` 为逻辑工作项数
    fn dispatch_1d(&self, kernel: KernelId, work_items: u32) -> Result<(), DispatchError> {
        let sig = self.kernels().get(kernel).ok_or(DispatchError::UnknownKernel)?;
        if !self.profile().meets(sig.min_profile) {
            return Err(DispatchError::ProfileTooLow {
                have: self.profile(),
                need: sig.min_profile,
            });
        }
        self.dispatch_1d_checked(kernel, work_items, sig.workgroup_x)
    }

    /// 子类实现的实际调度（调用前已通过能力检查）
    fn dispatch_1d_checked(&self, kernel: KernelId, work_items: u32, workgroup_x: u32) -> Result<(), DispatchError>;
}
