// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 多速率时钟：权威 Tick 与 Coupling Epoch（每 4 Tick，见 GAP-03）。

/// 每个 Coupling Epoch 包含的权威 Tick 数
pub const TICKS_PER_COUPLING_EPOCH: u64 = 4;

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

    pub fn is_evolution_gate_open(&self) -> bool {
        self.tick > 0 && self.tick % TICKS_PER_COUPLING_EPOCH == 0
    }
}
