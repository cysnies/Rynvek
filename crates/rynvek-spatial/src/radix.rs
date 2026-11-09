// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 对 `(morton_key, dense_index)` 的 **4-Pass LSD 基数排序**（规范目录树「4-Pass 基数排序 CSR」）。
//!
//! 键宽 16 bit（高 2 bit 恒 0，有效 Morton 14 bit），每趟取 4 bit，共 4 趟；
//! 计数排序稳定，保证同格内按 `dense_index` 升序——此为邻居表确定性的根基。
//!
//! 规范未给出完整排序源码；本实现按经典 GPU UniformGrid 构建管线设计，并在注释中标明。

/// 排序记录：Morton 键 + 实体紧凑下标
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MortonRecord {
    pub morton_key: u32,
    pub dense_index: u32,
}

const RADIX_BITS: u32 = 4;
const RADIX_SIZE: usize = 1 << RADIX_BITS; // 16
const PASSES: u32 = 4; // 覆盖 16 bit

/// 原地 4-Pass LSD 基数排序（稳定）
pub fn radix_sort_morton_records(records: &mut [MortonRecord]) {
    if records.len() <= 1 {
        return;
    }
    let n = records.len();
    let mut tmp = vec![MortonRecord::default(); n];
    let mut counts = [0u32; RADIX_SIZE];

    for pass in 0..PASSES {
        let shift = pass * RADIX_BITS;
        counts.fill(0);
        for r in records.iter() {
            let digit = ((r.morton_key >> shift) & (RADIX_SIZE as u32 - 1)) as usize;
            counts[digit] += 1;
        }
        // 前缀和 → 起始偏移
        let mut sum = 0u32;
        for c in counts.iter_mut() {
            let v = *c;
            *c = sum;
            sum += v;
        }
        for &r in records.iter() {
            let digit = ((r.morton_key >> shift) & (RADIX_SIZE as u32 - 1)) as usize;
            let dest = counts[digit] as usize;
            tmp[dest] = r;
            counts[digit] += 1;
        }
        records.copy_from_slice(&tmp);
    }

    // 同键内按 dense_index 稳定次序：基数排序仅按 morton_key；
    // 若输入已按 dense_index 预排序，稳定性会保留；此处再做同键插入整理以保证契约。
    stabilize_dense_within_equal_keys(records);
}

fn stabilize_dense_within_equal_keys(records: &mut [MortonRecord]) {
    let mut i = 0;
    while i < records.len() {
        let mut j = i + 1;
        while j < records.len() && records[j].morton_key == records[i].morton_key {
            j += 1;
        }
        records[i..j].sort_by_key(|r| r.dense_index);
        i = j;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_morton_then_dense() {
        let mut v = vec![
            MortonRecord {
                morton_key: 5,
                dense_index: 9,
            },
            MortonRecord {
                morton_key: 1,
                dense_index: 3,
            },
            MortonRecord {
                morton_key: 5,
                dense_index: 2,
            },
            MortonRecord {
                morton_key: 1,
                dense_index: 1,
            },
        ];
        radix_sort_morton_records(&mut v);
        assert_eq!(
            v.iter().map(|r| (r.morton_key, r.dense_index)).collect::<Vec<_>>(),
            vec![(1, 1), (1, 3), (5, 2), (5, 9)]
        );
    }
}
