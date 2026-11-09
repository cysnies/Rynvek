// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 全局冻结的时钟与空间常数（规范 01 §1.1，与 `rynvek_core::constants` 一致）。
 */

/** 权威主时钟频率：每秒 30 个 Tick */
export const BASE_TICK_HZ = 30;

/** 每个权威 Tick 内的神经子步数（30 Hz × 4 = 120 Hz） */
export const NEURAL_SUBSTEPS_PER_TICK = 4;

/** 神经子步频率：120 Hz */
export const NEURAL_SUBSTEP_HZ = BASE_TICK_HZ * NEURAL_SUBSTEPS_PER_TICK;

/** 环境场更新间隔：每 3 个 Tick 更新一次（10 Hz） */
export const FIELD_UPDATE_INTERVAL_TICKS = 3;

/** 环境场更新频率：10 Hz */
export const FIELD_UPDATE_HZ = BASE_TICK_HZ / FIELD_UPDATE_INTERVAL_TICKS;

/** 宏观-微观耦合周期（Coupling Epoch）：每 4 个 Tick */
export const COUPLING_EPOCH_TICKS = 4;

/** 单个 Chunk 每条边的世界单位数（256 × 256 Units） */
export const CHUNK_SIZE_UNITS = 256;
