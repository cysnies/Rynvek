// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端 `VisibilityGrid` 物理裁剪协议（GAP-12）。
//!
//! `rynvek-rts` 的 [`FactionFog`] 以 Tile 分辨率执行对称阴影投射；本模块把它冻结为一帧的不可变快照：
//!
//! - **Tile 位图**：用于序列化阶段的逐实体裁剪——敌方移动单位所在 Tile 不可见即**零字节下发**；
//!   敌方建筑任一占地 Tile 可见才下发（与 `ArenaWorld::observe` 判定一致）；
//! - **聚合格**：每 `4×4` Tiles 一格、每 Chunk `64×64` 格，取值 `0` 未探索 / `1` 可见 / `2` 仅已探索，
//!   以每格 2 bit 打包下发给客户端绘制迷雾（[`VisibilityGrid::pack_cells`]）。
//!
//! 网格只由该阵营自己的视野构成，不含任何敌方信息，因此可以原样下发。

use rynvek_rts::fog::{FactionFog, VISIBILITY_CELL_TILES};

/// 每个 256×256 Chunk 的聚合格边长
pub const CELLS_PER_CHUNK_SIDE: u32 = 256 / VISIBILITY_CELL_TILES;

/// 一个阵营在某 Tick 的可见性快照
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibilityGrid {
    pub width: u32,
    pub height: u32,
    pub cells_w: u32,
    pub cells_h: u32,
    tiles: Vec<u64>,
    cells: Vec<u8>,
}

impl VisibilityGrid {
    /// 由阵营迷雾构造快照
    pub fn from_fog(fog: &FactionFog) -> Self {
        let (width, height) = (fog.width, fog.height);
        let mut tiles = vec![0u64; ((width * height) as usize).div_ceil(64)];
        for y in 0..height {
            for x in 0..width {
                if fog.is_visible(x, y) {
                    let i = (y * width + x) as usize;
                    tiles[i / 64] |= 1 << (i % 64);
                }
            }
        }
        Self {
            width,
            height,
            cells_w: width.div_ceil(VISIBILITY_CELL_TILES),
            cells_h: height.div_ceil(VISIBILITY_CELL_TILES),
            tiles,
            cells: fog.visibility_cells(),
        }
    }

    /// Tile 当前是否可见（越界为不可见）
    pub fn tile_visible(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let i = (y * self.width + x) as usize;
        self.tiles[i / 64] & (1 << (i % 64)) != 0
    }

    /// 矩形内是否有任一可见 Tile
    pub fn rect_visible(&self, x0: u32, y0: u32, w: u32, h: u32) -> bool {
        (y0..y0.saturating_add(h).min(self.height))
            .any(|y| (x0..x0.saturating_add(w).min(self.width)).any(|x| self.tile_visible(x, y)))
    }

    /// 聚合格（行优先）
    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    /// 可见 Tile 数
    pub fn visible_tiles(&self) -> u32 {
        self.tiles.iter().map(|w| w.count_ones()).sum()
    }

    /// 每格 2 bit、每字节 4 格、低位在前打包
    pub fn pack_cells(&self) -> Vec<u8> {
        pack_cells(&self.cells)
    }

    /// 取出一个 Chunk 的 `64×64` 聚合格（行优先）
    pub fn chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Option<Vec<u8>> {
        let (x0, y0) = (chunk_x * CELLS_PER_CHUNK_SIDE, chunk_y * CELLS_PER_CHUNK_SIDE);
        if x0 >= self.cells_w || y0 >= self.cells_h {
            return None;
        }
        let mut out = Vec::with_capacity((CELLS_PER_CHUNK_SIDE * CELLS_PER_CHUNK_SIDE) as usize);
        for y in y0..y0 + CELLS_PER_CHUNK_SIDE {
            for x in x0..x0 + CELLS_PER_CHUNK_SIDE {
                out.push(if x < self.cells_w && y < self.cells_h {
                    self.cells[(y * self.cells_w + x) as usize]
                } else {
                    0
                });
            }
        }
        Some(out)
    }
}

/// 2 bit 打包
pub fn pack_cells(cells: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; cells.len().div_ceil(4)];
    for (i, &c) in cells.iter().enumerate() {
        out[i / 4] |= (c & 0b11) << ((i % 4) * 2);
    }
    out
}

/// 2 bit 解包出 `n` 格
pub fn unpack_cells(bytes: &[u8], n: usize) -> Option<Vec<u8>> {
    if bytes.len() < n.div_ceil(4) {
        return None;
    }
    Some((0..n).map(|i| (bytes[i / 4] >> ((i % 4) * 2)) & 0b11).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_mirrors_fog_and_packs_two_bits_per_cell() {
        let mut fog = FactionFog::new(256, 256);
        fog.reveal_from((32, 32), 12, |_, _| false);
        let grid = VisibilityGrid::from_fog(&fog);
        assert_eq!((grid.cells_w, grid.cells_h), (64, 64));
        assert_eq!(grid.visible_tiles(), fog.visible_count());
        for (x, y) in [(32, 32), (40, 32), (32, 44), (100, 100), (255, 0)] {
            assert_eq!(grid.tile_visible(x, y), fog.is_visible(x, y));
        }
        assert!(!grid.tile_visible(256, 0));
        let packed = grid.pack_cells();
        assert_eq!(packed.len(), 64 * 64 / 4);
        assert_eq!(unpack_cells(&packed, 64 * 64).unwrap(), grid.cells());
        assert_eq!(grid.chunk_cells(0, 0).unwrap(), grid.cells());
        assert!(grid.chunk_cells(1, 0).is_none());

        fog.begin_frame();
        let explored_only = VisibilityGrid::from_fog(&fog);
        assert_eq!(explored_only.visible_tiles(), 0);
        assert_eq!(
            explored_only.cells()[(8 * 64 + 8) as usize],
            2,
            "离开视野的格保留已探索"
        );
    }

    #[test]
    fn pack_is_lsb_first() {
        assert_eq!(pack_cells(&[1, 2, 0, 1, 2]), vec![0b01_00_10_01, 0b10]);
    }
}
