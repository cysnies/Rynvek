// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 全局冻结的时钟与空间常数（规范 01 §1.1「时钟与空间常数」）。

/// 权威主时钟频率：每秒 30 个 Tick
pub const BASE_TICK_HZ: u32 = 30;

/// 每个权威 Tick 内的神经子步数（30 Hz × 4 = 120 Hz）
pub const NEURAL_SUBSTEPS_PER_TICK: u32 = 4;

/// 神经子步频率：120 Hz
pub const NEURAL_SUBSTEP_HZ: u32 = BASE_TICK_HZ * NEURAL_SUBSTEPS_PER_TICK;

/// 环境场更新间隔：每 3 个 Tick 更新一次（10 Hz）
pub const FIELD_UPDATE_INTERVAL_TICKS: u32 = 3;

/// 环境场更新频率：10 Hz
pub const FIELD_UPDATE_HZ: u32 = BASE_TICK_HZ / FIELD_UPDATE_INTERVAL_TICKS;

/// 宏观-微观耦合周期（Coupling Epoch）：每 4 个 Tick
pub const COUPLING_EPOCH_TICKS: u32 = 4;

/// 单个 Chunk 每条边的世界单位数（256 × 256 Units）
pub const CHUNK_SIZE_UNITS: u32 = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_rates_match_spec() {
        assert_eq!(NEURAL_SUBSTEP_HZ, 120);
        assert_eq!(FIELD_UPDATE_HZ, 10);
        assert_eq!(BASE_TICK_HZ % FIELD_UPDATE_INTERVAL_TICKS, 0);
    }
}
