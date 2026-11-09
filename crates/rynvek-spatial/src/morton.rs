// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 二维 Morton（Z-order）编码：每轴 7 位 → 14 位键，覆盖 `128×128` 均匀网格。

use crate::csr_grid::GRID_CELLS_PER_AXIS;

/// 把 7 位整数的位隔一插入（`0babcdefg` → `0ba0b0c0d0e0f0g`）
#[inline]
pub const fn part1_by1_u7(mut n: u32) -> u32 {
    n &= 0x7F;
    n = (n | (n << 8)) & 0x00_7F_00_7F;
    n = (n | (n << 4)) & 0x0F_0F_0F_0F;
    n = (n | (n << 2)) & 0x33_33_33_33;
    n = (n | (n << 1)) & 0x55_55_55_55;
    n
}

/// 编码 `(cell_x, cell_y)` 为 14 位 Morton 键；输入须 `< 128`
#[inline]
pub const fn encode_morton2_u7(cell_x: u32, cell_y: u32) -> u32 {
    debug_assert!(cell_x < GRID_CELLS_PER_AXIS as u32);
    debug_assert!(cell_y < GRID_CELLS_PER_AXIS as u32);
    (part1_by1_u7(cell_y) << 1) | part1_by1_u7(cell_x)
}

/// Morton 键反解为格子坐标
#[inline]
pub const fn morton_to_cell(key: u32) -> (u32, u32) {
    (compact1_by1(key), compact1_by1(key >> 1))
}

#[inline]
const fn compact1_by1(mut n: u32) -> u32 {
    n &= 0x55_55_55_55;
    n = (n | (n >> 1)) & 0x33_33_33_33;
    n = (n | (n >> 2)) & 0x0F_0F_0F_0F;
    n = (n | (n >> 4)) & 0x00_FF_00_FF;
    n = (n | (n >> 8)) & 0x00_00_FF_FF;
    n
}

/// 由 Q32.32 局部坐标得到格子坐标。
///
/// 规范：每格 `2.0` World Units，Chunk 覆盖 `256×256` → 整数瓦片坐标 `tile = local >> 32`，
/// 格子 `cell = tile / 2`，钳制到 `[0, 127]`。
#[inline]
pub const fn cell_coords_from_local_q32(local_x: u64, local_y: u64) -> (u32, u32) {
    let tile_x = (local_x >> 32) as u32;
    let tile_y = (local_y >> 32) as u32;
    let max_cell = (GRID_CELLS_PER_AXIS as u32) - 1;
    let cx = tile_x / 2;
    let cy = tile_y / 2;
    // Rust 1.85 中 Ord::min 非 const；手工钳制（同 E-002）
    (
        if cx > max_cell { max_cell } else { cx },
        if cy > max_cell { max_cell } else { cy },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morton_roundtrip_corners() {
        for &(x, y) in &[(0u32, 0), (127, 0), (0, 127), (127, 127), (3, 5), (64, 64)] {
            let k = encode_morton2_u7(x, y);
            assert_eq!(morton_to_cell(k), (x, y));
        }
    }

    #[test]
    fn q32_maps_to_expected_cell() {
        // tile (4, 6) → cell (2, 3)
        let lx = 4u64 << 32;
        let ly = 6u64 << 32;
        assert_eq!(cell_coords_from_local_q32(lx, ly), (2, 3));
    }
}
