// crates/rynvek-spatial/src/csr_grid.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `UniformGridCsr` 空间索引与 CSR 邻居表（规范 06 §6.2）。

use rynvek_core::error::RynvekErrorCode;

use crate::morton::{cell_coords_from_local_q32, encode_morton2_u7, morton_to_cell};
use crate::radix::{MortonRecord, radix_sort_morton_records};

pub const GRID_CELLS_PER_AXIS: usize = 128; // 每格 2.0 x 2.0 World Units，覆盖 256x256 Chunk
pub const TOTAL_GRID_CELLS: usize = GRID_CELLS_PER_AXIS * GRID_CELLS_PER_AXIS; // 16,384
pub const MAX_NEIGHBORS_PER_ENTITY: usize = 32;

/// 均匀网格压缩行存储索引
#[derive(Clone, Debug, Default)]
pub struct SpatialCsrIndex {
    /// 长度固定为 16,385：cell `c` 内的实体位于 sorted_dense_indices[row_offsets[c]..row_offsets[c+1]]
    pub row_offsets: Vec<u32>,
    /// 按 (cell_morton_key, dense_index) 稳定升序排列的实体紧凑索引
    pub sorted_dense_indices: Vec<u32>,
    /// 与 `sorted_dense_indices` 平行的 Morton 键（构建邻居表时避免重复编码）
    morton_keys: Vec<u32>,
}

impl SpatialCsrIndex {
    /// 空索引（仅含 offsets 哨兵）
    pub fn empty() -> Self {
        Self {
            row_offsets: vec![0u32; TOTAL_GRID_CELLS + 1],
            sorted_dense_indices: Vec::new(),
            morton_keys: Vec::new(),
        }
    }

    /// 由实体 Q32.32 局部坐标构建 CSR 索引。
    ///
    /// `positions` 按下标即 `dense_index`；长度为实体数。
    pub fn build_from_local_q32(positions: &[(u64, u64)]) -> Result<Self, RynvekErrorCode> {
        if positions.len() > u32::MAX as usize {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let mut records: Vec<MortonRecord> = positions
            .iter()
            .enumerate()
            .map(|(i, &(lx, ly))| {
                let (cx, cy) = cell_coords_from_local_q32(lx, ly);
                MortonRecord {
                    morton_key: encode_morton2_u7(cx, cy),
                    dense_index: i as u32,
                }
            })
            .collect();

        // 输入已按 dense_index 升序；4-Pass 基数排序稳定保留同键次序，再显式整理
        radix_sort_morton_records(&mut records);

        let mut row_offsets = vec![0u32; TOTAL_GRID_CELLS + 1];
        let mut sorted_dense_indices = Vec::with_capacity(records.len());
        let mut morton_keys = Vec::with_capacity(records.len());

        // 按 Morton 键递增填充；Morton 序 ≠ 行主序，故先计数再前缀
        let mut cell_counts = vec![0u32; TOTAL_GRID_CELLS];
        for r in &records {
            let (cx, cy) = morton_to_cell(r.morton_key);
            let cell = (cy as usize) * GRID_CELLS_PER_AXIS + (cx as usize);
            cell_counts[cell] += 1;
            sorted_dense_indices.push(r.dense_index);
            morton_keys.push(r.morton_key);
        }
        let mut sum = 0u32;
        for (i, &c) in cell_counts.iter().enumerate() {
            row_offsets[i] = sum;
            sum += c;
        }
        row_offsets[TOTAL_GRID_CELLS] = sum;

        // 上面按 Morton 遍历写入 sorted_*，与 row_offsets（行主序）不一致。
        // 重新按格子桶散射，保证 CSR 语义：cell c → [row_offsets[c], row_offsets[c+1])。
        let mut write_cursor = row_offsets.clone();
        let mut dense_out = vec![0u32; records.len()];
        let mut key_out = vec![0u32; records.len()];
        for r in &records {
            let (cx, cy) = morton_to_cell(r.morton_key);
            let cell = (cy as usize) * GRID_CELLS_PER_AXIS + (cx as usize);
            let dest = write_cursor[cell] as usize;
            dense_out[dest] = r.dense_index;
            key_out[dest] = r.morton_key;
            write_cursor[cell] += 1;
        }

        Ok(Self {
            row_offsets,
            sorted_dense_indices: dense_out,
            morton_keys: key_out,
        })
    }

    /// 查询实体 `dense` 的邻居表（自身除外），最多 `MAX_NEIGHBORS_PER_ENTITY` 个，
    /// 按 dense_index 升序；搜索范围为本格及 8 邻格。
    pub fn gather_neighbors(&self, dense: u32, positions_len: u32) -> Vec<u32> {
        if dense >= positions_len {
            return Vec::new();
        }
        // 定位该实体所在格：扫描 morton_keys 对应的 dense
        let Some(pos_in_csr) = self.sorted_dense_indices.iter().position(|&d| d == dense) else {
            return Vec::new();
        };
        let key = self.morton_keys[pos_in_csr];
        let (cx, cy) = morton_to_cell(key);

        let mut neighbors = Vec::with_capacity(MAX_NEIGHBORS_PER_ENTITY);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = cx as i32 + dx;
                let ny = cy as i32 + dy;
                if !(0..GRID_CELLS_PER_AXIS as i32).contains(&nx) || !(0..GRID_CELLS_PER_AXIS as i32).contains(&ny) {
                    continue;
                }
                let cell = (ny as usize) * GRID_CELLS_PER_AXIS + (nx as usize);
                let start = self.row_offsets[cell] as usize;
                let end = self.row_offsets[cell + 1] as usize;
                for &cand in &self.sorted_dense_indices[start..end] {
                    if cand == dense {
                        continue;
                    }
                    neighbors.push(cand);
                }
            }
        }
        neighbors.sort_unstable();
        neighbors.dedup();
        if neighbors.len() > MAX_NEIGHBORS_PER_ENTITY {
            neighbors.truncate(MAX_NEIGHBORS_PER_ENTITY);
        }
        neighbors
    }

    /// 为全部实体构建邻居表（验收用）
    pub fn build_neighbor_table(&self, entity_count: u32) -> Vec<Vec<u32>> {
        (0..entity_count)
            .map(|d| self.gather_neighbors(d, entity_count))
            .collect()
    }

    pub fn entity_count_in_cell(&self, cell: usize) -> u32 {
        if cell >= TOTAL_GRID_CELLS {
            return 0;
        }
        self.row_offsets[cell + 1] - self.row_offsets[cell]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q32(tile_x: u32, tile_y: u32) -> (u64, u64) {
        ((tile_x as u64) << 32, (tile_y as u64) << 32)
    }

    #[test]
    fn csr_row_offsets_cover_all_entities() {
        // 两实体同格 (tile 2,2)→cell(1,1)；一实体另一格
        let positions = vec![q32(2, 2), q32(2, 2), q32(10, 10)];
        let idx = SpatialCsrIndex::build_from_local_q32(&positions).unwrap();
        assert_eq!(idx.row_offsets.len(), TOTAL_GRID_CELLS + 1);
        assert_eq!(*idx.row_offsets.last().unwrap(), 3);
        assert_eq!(idx.sorted_dense_indices.len(), 3);

        let cell_a = GRID_CELLS_PER_AXIS + 1;
        assert_eq!(idx.entity_count_in_cell(cell_a), 2);
        let cell_b = 5 * GRID_CELLS_PER_AXIS + 5; // tile 10 → cell 5
        assert_eq!(idx.entity_count_in_cell(cell_b), 1);

        // 同格内 dense 升序
        let s = idx.row_offsets[cell_a] as usize;
        let e = idx.row_offsets[cell_a + 1] as usize;
        assert_eq!(&idx.sorted_dense_indices[s..e], &[0, 1]);
    }

    #[test]
    fn neighbor_table_finds_moore_neighborhood() {
        // cell(1,1) 放 0,1；相邻 cell(2,1) 放 2；远处放 3
        let positions = vec![
            q32(2, 2),     // cell 1,1
            q32(3, 2),     // cell 1,1 (tile 3 → cell 1)
            q32(4, 2),     // cell 2,1
            q32(200, 200), // 远处
        ];
        let idx = SpatialCsrIndex::build_from_local_q32(&positions).unwrap();
        let table = idx.build_neighbor_table(4);
        // 实体 0 应看见 1 与 2
        assert_eq!(table[0], vec![1, 2]);
        assert_eq!(table[1], vec![0, 2]);
        assert_eq!(table[2], vec![0, 1]);
        assert!(table[3].is_empty());
    }
}
