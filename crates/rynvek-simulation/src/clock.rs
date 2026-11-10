// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 多速率时钟：权威 Tick（30 Hz）、连续环境场步（10 Hz，每 3 Tick）与 Coupling Epoch（7.5 Hz，每 4 Tick，见 GAP-03）。

/// 每个 Coupling Epoch 包含的权威 Tick 数（30 Hz / 4 = 7.5 Hz）
pub const TICKS_PER_COUPLING_EPOCH: u64 = 4;
/// 每个连续环境场步包含的权威 Tick 数（30 Hz / 3 = 10 Hz，与 `FIELD_UPDATE_INTERVAL_TICKS` 一致）
pub const TICKS_PER_FIELD_STEP: u64 = 3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiRateClock {
    pub tick: u64,
    pub coupling_epoch: u64,
    pub world_revision: u64,
}

impl MultiRateClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Phase 00：推进时钟。返回是否刚跨入新的 Coupling Epoch 边界。
    pub fn advance(&mut self) -> bool {
        self.tick = self.tick.saturating_add(1);
        self.world_revision = self.world_revision.saturating_add(1);
        let crossed = self.tick % TICKS_PER_COUPLING_EPOCH == 0;
        if crossed {
            self.coupling_epoch = self.coupling_epoch.saturating_add(1);
        }
        crossed
    }

    /// 当前 Tick（`advance()` 之后，从 1 起计）是否应当推进 10 Hz 连续环境场（即第 1, 4, 7, ... 个 Tick，每 3 Tick 推进一次）
    #[inline]
    pub const fn is_field_step_due(&self) -> bool {
        self.tick == 0 || (self.tick - 1) % TICKS_PER_FIELD_STEP == 0
    }

    pub fn is_evolution_gate_open(&self) -> bool {
        self.tick > 0 && self.tick % TICKS_PER_COUPLING_EPOCH == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_step_runs_at_10hz_ten_times_per_thirty_ticks() {
        let mut clock = MultiRateClock::new();
        let mut field_steps = 0;
        let mut epochs = 0;
        for _ in 0..30 {
            if clock.advance() {
                epochs += 1;
            }
            if clock.is_field_step_due() {
                field_steps += 1;
            }
        }
        assert_eq!(field_steps, 10, "30 个 30Hz Tick 内环境场必须精确推进 10 次 (10Hz)");
        assert_eq!(epochs, 7);
    }
}
