// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `600 APM` 公平限流（项目文档 GameMode `PvP_Ranked`）。
//!
//! 以 Tick 为时间基准的滑动窗口：任意连续 `1,800 Ticks`（60 s @ 30 Hz）内至多 600 个动作。
//! 人类、脚本与 Agent 共用同一阵营窗口；感知类查询不计入动作。

use std::collections::VecDeque;

use crate::error::AgentError;

/// 窗口长度：60 s × 30 Hz
pub const APM_WINDOW_TICKS: u64 = 1800;
/// 每窗口动作上限
pub const APM_MAX_ACTIONS: usize = 600;

/// 滑动窗口限流器
#[derive(Clone, Debug)]
pub struct ApmLimiter {
    window_ticks: u64,
    max_actions: usize,
    actions: VecDeque<u64>,
}

impl Default for ApmLimiter {
    fn default() -> Self {
        Self::new(APM_WINDOW_TICKS, APM_MAX_ACTIONS)
    }
}

impl ApmLimiter {
    pub fn new(window_ticks: u64, max_actions: usize) -> Self {
        Self {
            window_ticks,
            max_actions,
            actions: VecDeque::new(),
        }
    }

    fn expire(&mut self, tick: u64) {
        while self.actions.front().is_some_and(|&t| t + self.window_ticks <= tick) {
            self.actions.pop_front();
        }
    }

    /// 当前窗口内已用动作数
    pub fn used(&mut self, tick: u64) -> usize {
        self.expire(tick);
        self.actions.len()
    }

    /// 检查是否还能执行一个动作（不消耗）
    pub fn check(&mut self, tick: u64) -> Result<(), AgentError> {
        if self.used(tick) >= self.max_actions {
            return Err(AgentError::rate_limited(format!(
                "超出 {} 动作 / {} Ticks 限额",
                self.max_actions, self.window_ticks
            )));
        }
        Ok(())
    }

    /// 消耗一个动作
    pub fn try_acquire(&mut self, tick: u64) -> Result<(), AgentError> {
        self.check(tick)?;
        self.actions.push_back(tick);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliding_window_allows_600_per_1800_ticks() {
        let mut l = ApmLimiter::default();
        for i in 0..600 {
            l.try_acquire(i).unwrap();
        }
        assert_eq!(l.try_acquire(600).unwrap_err().code.code(), 0x8004);
        assert_eq!(l.try_acquire(1799).unwrap_err().code.code(), 0x8004);
        // Tick 1800 时第 0 Tick 的动作滑出窗口
        l.try_acquire(1800).unwrap();
        assert!(l.try_acquire(1800).is_err());
        assert_eq!(l.used(1800), 600);
        assert_eq!(l.used(1801), 599);
    }
}
