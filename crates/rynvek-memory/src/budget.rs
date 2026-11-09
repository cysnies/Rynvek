// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 四级内存预算状态机（D541–D560）。
//!
//! 阈值以「已用字节 / 预算上限」的百分比划分，跨过阈值时单向升级；
//! 仅在水位回落到更低阈值减去滞回余量后才降级，避免抖动。

use rynvek_core::error::RynvekErrorCode;

/// 四级预算状态（数值越大越紧张）
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemoryBudgetLevel {
    /// 正常：可自由分配
    #[default]
    Normal = 0,
    /// 软限制：应开始压缩缓存、推迟非关键分配
    SoftLimit = 1,
    /// 硬限制：仅允许权威仿真路径分配，拒绝表现层缓存扩张
    HardLimit = 2,
    /// 危急：拒绝一切新分配，上层应触发降级或快照卸载
    Critical = 3,
}

/// 一次分配请求在预算上的裁决
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetDecision {
    /// 允许分配，并报告分配后将进入的预算等级
    Allow { level_after: MemoryBudgetLevel },
    /// 拒绝分配（对应 `RynvekErrorCode::WorldMemoryBudgetExceeded`）
    Deny {
        code: RynvekErrorCode,
        level: MemoryBudgetLevel,
    },
}

/// 内存预算记账器
#[derive(Clone, Debug)]
pub struct MemoryBudget {
    /// 总预算上限（字节）
    limit_bytes: u64,
    /// 当前已用字节
    used_bytes: u64,
    /// 当前等级
    level: MemoryBudgetLevel,
    /// 降级滞回：水位需低于阈值该百分比才允许降级
    hysteresis_bps: u64,
}

impl MemoryBudget {
    /// 软限制阈值：75%
    pub const SOFT_BPS: u64 = 7_500;
    /// 硬限制阈值：90%
    pub const HARD_BPS: u64 = 9_000;
    /// 危急阈值：98%
    pub const CRITICAL_BPS: u64 = 9_800;
    /// 默认滞回：2%
    pub const DEFAULT_HYSTERESIS_BPS: u64 = 200;

    /// 创建预算记账器；`limit_bytes` 必须大于 0
    pub fn new(limit_bytes: u64) -> Self {
        assert!(limit_bytes > 0, "内存预算上限必须为正");
        Self {
            limit_bytes,
            used_bytes: 0,
            level: MemoryBudgetLevel::Normal,
            hysteresis_bps: Self::DEFAULT_HYSTERESIS_BPS,
        }
    }

    /// 预算上限
    pub const fn limit_bytes(&self) -> u64 {
        self.limit_bytes
    }

    /// 已用字节
    pub const fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// 剩余可分配字节
    pub const fn remaining_bytes(&self) -> u64 {
        self.limit_bytes.saturating_sub(self.used_bytes)
    }

    /// 当前等级
    pub const fn level(&self) -> MemoryBudgetLevel {
        self.level
    }

    /// 以基点（万分比）表示的占用率
    pub fn usage_bps(&self) -> u64 {
        if self.limit_bytes == 0 {
            return 10_000;
        }
        (self.used_bytes.saturating_mul(10_000)) / self.limit_bytes
    }

    /// 预检一次分配是否可接受（不修改记账）
    pub fn evaluate(&self, request_bytes: u64) -> BudgetDecision {
        let projected = self.used_bytes.saturating_add(request_bytes);
        if projected > self.limit_bytes || self.level == MemoryBudgetLevel::Critical {
            return BudgetDecision::Deny {
                code: RynvekErrorCode::WorldMemoryBudgetExceeded,
                level: MemoryBudgetLevel::Critical.max(self.level),
            };
        }
        let level_after = level_for_bps((projected.saturating_mul(10_000)) / self.limit_bytes);
        // 已处于硬限制时，禁止把占用推入危急区以外的「表现层」扩张由调用方自行区分；
        // 预算层只保证不超过上限，并把等级反馈给上层。
        BudgetDecision::Allow { level_after }
    }

    /// 记账一次成功分配，并刷新等级（只升不降，降级见 [`Self::reclaim`]）
    pub fn commit_allocation(&mut self, bytes: u64) -> Result<MemoryBudgetLevel, RynvekErrorCode> {
        match self.evaluate(bytes) {
            BudgetDecision::Allow { level_after } => {
                self.used_bytes = self.used_bytes.saturating_add(bytes);
                if level_after > self.level {
                    self.level = level_after;
                }
                Ok(self.level)
            }
            BudgetDecision::Deny { code, .. } => Err(code),
        }
    }

    /// 回收字节并按滞回规则尝试降级
    pub fn reclaim(&mut self, bytes: u64) -> MemoryBudgetLevel {
        self.used_bytes = self.used_bytes.saturating_sub(bytes);
        let bps = self.usage_bps();
        // 仅当水位低于「当前等级进入阈值 − 滞回」时才允许降到 `level_for_bps` 的结果
        let can_leave_critical = bps + self.hysteresis_bps < Self::CRITICAL_BPS;
        let can_leave_hard = bps + self.hysteresis_bps < Self::HARD_BPS;
        let can_leave_soft = bps + self.hysteresis_bps < Self::SOFT_BPS;
        let target = level_for_bps(bps);
        self.level = match self.level {
            MemoryBudgetLevel::Critical if can_leave_critical => target,
            MemoryBudgetLevel::HardLimit if can_leave_hard => target,
            MemoryBudgetLevel::SoftLimit if can_leave_soft => target,
            MemoryBudgetLevel::Normal => MemoryBudgetLevel::Normal,
            keep => keep,
        };
        self.level
    }
}

fn level_for_bps(bps: u64) -> MemoryBudgetLevel {
    if bps >= MemoryBudget::CRITICAL_BPS {
        MemoryBudgetLevel::Critical
    } else if bps >= MemoryBudget::HARD_BPS {
        MemoryBudgetLevel::HardLimit
    } else if bps >= MemoryBudget::SOFT_BPS {
        MemoryBudgetLevel::SoftLimit
    } else {
        MemoryBudgetLevel::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_escalate_with_usage() {
        let mut budget = MemoryBudget::new(10_000);
        assert_eq!(budget.commit_allocation(7_400), Ok(MemoryBudgetLevel::Normal));
        assert_eq!(budget.commit_allocation(200), Ok(MemoryBudgetLevel::SoftLimit));
        assert_eq!(budget.commit_allocation(1_500), Ok(MemoryBudgetLevel::HardLimit));
        assert_eq!(budget.commit_allocation(700), Ok(MemoryBudgetLevel::Critical));
        assert_eq!(
            budget.commit_allocation(1),
            Err(RynvekErrorCode::WorldMemoryBudgetExceeded)
        );
    }

    #[test]
    fn reclaim_uses_hysteresis() {
        let mut budget = MemoryBudget::new(10_000);
        budget.commit_allocation(9_800).unwrap();
        assert_eq!(budget.level(), MemoryBudgetLevel::Critical);
        // 回收到 97.9%：仍高于 98%−2%，保持 Critical
        budget.reclaim(10);
        assert_eq!(budget.level(), MemoryBudgetLevel::Critical);
        // 再回收到 95%：低于危急阈值减滞回，降到 HardLimit/Soft 由实际占用决定
        budget.reclaim(290);
        assert!(budget.level() <= MemoryBudgetLevel::HardLimit);
    }
}
