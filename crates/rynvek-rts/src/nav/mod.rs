// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三级确定性导航（GAP-11）：宏观 `16×16` 扇区 HPA* + 中观多单位共享 Eikonal 向量流场 +
//! 微观 `Q32.32` 定点 2-Iteration ORCA 互惠避障。

pub mod grid;
pub mod hpa;

pub use grid::{NavGrid, Rect, astar, terrain_cost};
pub use hpa::{HpaGraph, HpaPath, SECTOR_SIZE};
