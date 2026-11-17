// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-compute-cpu
//!
//! CPU Reference 确定性执行器（ADR-010 / 审计报告 §8）：作为跨设备数值一致性的黄金参照后端。
//! 支持：
//! 1. 标准 `ComputeBackend::dispatch_1d_checked` 能力校验与工作项记账；
//! 2. 确定性分块回调执行器 `execute_workgroups_1d`，按严格单调递增的 `(group_idx, local_idx)` 顺序调用计算核闭包，杜绝多线程非确定性归约顺序。

#![deny(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use rynvek_compute::{CapabilityProfile, ComputeBackend, DispatchError, KernelId, KernelRegistry};
use rynvek_core::error::RynvekErrorCode;

/// CPU Reference 确定性计算后端
#[derive(Debug)]
pub struct CpuReferenceBackend {
    kernels: KernelRegistry,
    /// 累计已调度的工作项数（可观测性指标）
    dispatched_work_items: AtomicU64,
    /// 累计已执行的确定性工作组数
    executed_workgroups: AtomicU64,
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
            executed_workgroups: AtomicU64::new(0),
        }
    }

    pub fn dispatched_work_items(&self) -> u64 {
        self.dispatched_work_items.load(Ordering::Relaxed)
    }

    pub fn executed_workgroups(&self) -> u64 {
        self.executed_workgroups.load(Ordering::Relaxed)
    }

    /// 校验内核签名并按确定性工作组顺序（`group_id = 0..groups`, `local_id = 0..workgroup_x`）真实执行 CPU 闭包
    pub fn execute_1d<F>(
        &self,
        kernel: KernelId,
        work_items: u32,
        workgroup_x: u32,
        mut f: F,
    ) -> Result<u32, DispatchError>
    where
        F: FnMut(u32, u32, u32),
    {
        let meta = self.kernels.get(kernel).ok_or(DispatchError::UnknownKernel)?;
        if !self.profile().meets(meta.min_profile) {
            return Err(DispatchError::ProfileTooLow {
                need: meta.min_profile,
                have: self.profile(),
            });
        }
        if workgroup_x == 0 {
            return Err(DispatchError::Backend(RynvekErrorCode::DimensionMismatch));
        }
        let groups = work_items.div_ceil(workgroup_x);
        for g in 0..groups {
            let base = g * workgroup_x;
            let limit = (work_items - base).min(workgroup_x);
            for local in 0..limit {
                f(base + local, g, local);
            }
        }
        self.dispatched_work_items
            .fetch_add(u64::from(work_items), Ordering::Relaxed);
        self.executed_workgroups.fetch_add(u64::from(groups), Ordering::Relaxed);
        Ok(groups)
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
        self.execute_1d(kernel, work_items, workgroup_x, |_, _, _| {})
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_known_kernels_and_executes_deterministic_workgroups() {
        let backend = CpuReferenceBackend::new();
        assert_eq!(backend.profile(), CapabilityProfile::TierD);
        let mut sum = 0u64;
        let groups = backend
            .execute_1d(KernelId("sim.tick_phase_noop"), 100, 32, |global_idx, _, _| {
                sum += global_idx as u64;
            })
            .unwrap();
        assert_eq!(groups, 4);
        assert_eq!(sum, (0..100u64).sum::<u64>());
        assert_eq!(backend.dispatched_work_items(), 100);
        assert_eq!(backend.executed_workgroups(), 4);
        assert_eq!(
            backend.dispatch_1d(KernelId("no.such.kernel"), 1),
            Err(DispatchError::UnknownKernel)
        );
    }
}
