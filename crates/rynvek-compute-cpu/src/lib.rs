// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-compute-cpu
//!
//! CPU Reference 确定性执行器（ADR-010）：作为跨设备数值一致性的黄金参照后端。
//! M2 阶段以可调度的 noop/计数内核验证注册与能力门禁；场/空间的权威算法仍在
//! `rynvek-field` / `rynvek-spatial` 中直接以纯函数实现，供本后端后续挂接。

#![deny(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use rynvek_compute::{CapabilityProfile, ComputeBackend, DispatchError, KernelId, KernelRegistry};
use rynvek_core::error::RynvekErrorCode;

/// CPU Reference 后端
#[derive(Debug)]
pub struct CpuReferenceBackend {
    kernels: KernelRegistry,
    /// 累计已调度的工作项数（测试可观测性）
    dispatched_work_items: AtomicU64,
}

impl Default for CpuReferenceBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuReferenceBackend {
    pub fn new() -> Self {
        Self {
            kernels: KernelRegistry::with_cpu_reference_kernels(),
            dispatched_work_items: AtomicU64::new(0),
        }
    }

    pub fn dispatched_work_items(&self) -> u64 {
        self.dispatched_work_items.load(Ordering::Relaxed)
    }
}

impl ComputeBackend for CpuReferenceBackend {
    fn name(&self) -> &'static str {
        "cpu-reference"
    }

    fn profile(&self) -> CapabilityProfile {
        CapabilityProfile::TierD
    }

    fn kernels(&self) -> &KernelRegistry {
        &self.kernels
    }

    fn dispatch_1d_checked(&self, kernel: KernelId, work_items: u32, workgroup_x: u32) -> Result<(), DispatchError> {
        if workgroup_x == 0 {
            return Err(DispatchError::Backend(RynvekErrorCode::DimensionMismatch));
        }
        // 确定性空转：仅累计工作项，保证多次调度结果可复现
        match kernel.0 {
            "field.reaction_diffusion_q8" | "spatial.build_uniform_grid_csr" | "sim.tick_phase_noop" => {
                self.dispatched_work_items
                    .fetch_add(u64::from(work_items), Ordering::Relaxed);
                Ok(())
            }
            _ => Err(DispatchError::UnknownKernel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_known_kernels_and_rejects_unknown() {
        let backend = CpuReferenceBackend::new();
        assert_eq!(backend.profile(), CapabilityProfile::TierD);
        backend.dispatch_1d(KernelId("sim.tick_phase_noop"), 16).unwrap();
        assert_eq!(backend.dispatched_work_items(), 16);
        assert_eq!(
            backend.dispatch_1d(KernelId("no.such.kernel"), 1),
            Err(DispatchError::UnknownKernel)
        );
    }
}
