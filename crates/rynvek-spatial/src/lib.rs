// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-spatial
//!
//! UniformGrid CSR 空间索引（规范 06 §6.2）：
//!
//! - [`morton`]：7+7 位 Morton 编码（覆盖 128×128 格）；
//! - [`radix`]：对 `(morton_key, dense_index)` 的 4-Pass 基数排序；
//! - [`csr_grid`]：`SpatialCsrIndex` 构建与 3×3 邻域邻居表查询。

#![deny(unsafe_code)]

pub mod csr_grid;
pub mod morton;
pub mod radix;

pub use csr_grid::{GRID_CELLS_PER_AXIS, MAX_NEIGHBORS_PER_ENTITY, SpatialCsrIndex, TOTAL_GRID_CELLS};
pub use morton::{cell_coords_from_local_q32, encode_morton2_u7, morton_to_cell};
