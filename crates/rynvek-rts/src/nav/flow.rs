// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 中观层：多单位共享的 Eikonal 向量流场（定点 Fast Marching Method）。
//!
//! 对同一目标的所有单位只求解一次到达时间场 `T`：`|∇T| = c(x)`，`c` 为地形代价（‰ / Tile）。
//! 采用一阶迎风格式的整数 FMM：
//! - 取 `a = min(T_west, T_east)`、`b = min(T_south, T_north)`；
//! - 若 `|a − b| ≥ c`：`T = min(a, b) + c`；否则 `T = (a + b + isqrt(2c² − (a − b)²)) / 2`；
//! - 窄带用 `(T, tile_index)` 全序小顶堆推进，结果与平台无关。
//!
//! **扇区稀疏**：可只在 HPA* 走廊扇区（及其邻居）内求解，走廊外视为不可达；
//! 方向向量为 `−∇T` 的迎风差分（不可达邻格以自身值代替），零梯度时退化为指向 `T` 最小的八邻格。

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use rynvek_core::Q32_32;

use super::grid::{NEIGHBORS_8, NavGrid};
use super::hpa::SECTOR_SIZE;
use crate::math::Vec2;

/// 不可达
pub const UNREACHED: u64 = u64::MAX;

/// 确定性 64 位整数平方根（向下取整）
pub const fn isqrt_u64(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = 1u64 << ((64 - v.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + v / x) >> 1;
        if y >= x {
            break;
        }
        x = y;
    }
    while x * x > v {
        x -= 1;
    }
    x
}

/// 一个目标的流场
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowField {
    pub width: u32,
    pub height: u32,
    pub goal: (u32, u32),
    /// 到达时间（‰ 代价单位），`UNREACHED` 表示走廊外或不可达
    pub arrival: Vec<u64>,
}

impl FlowField {
    /// 求解流场；`corridor` 为允许求解的扇区集合（`None` = 全图）
    pub fn solve(grid: &NavGrid, goal: (u32, u32), corridor: Option<&BTreeSet<u32>>) -> Self {
        let (w, h) = (grid.width, grid.height);
        let sectors_x = w.div_ceil(SECTOR_SIZE);
        let allowed =
            |x: u32, y: u32| corridor.is_none_or(|c| c.contains(&((y / SECTOR_SIZE) * sectors_x + x / SECTOR_SIZE)));
        let n = (w * h) as usize;
        let mut t = vec![UNREACHED; n];
        let mut frozen = vec![false; n];
        let mut heap = BinaryHeap::new();
        if grid.passable(goal.0 as i64, goal.1 as i64) {
            let gi = grid.index(goal.0, goal.1);
            t[gi] = 0;
            heap.push(Reverse((0u64, gi as u32)));
        }
        let at = |t: &[u64], x: i64, y: i64| -> u64 {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                UNREACHED
            } else {
                t[(y as u32 * w + x as u32) as usize]
            }
        };
        while let Some(Reverse((d, i))) = heap.pop() {
            let i = i as usize;
            if frozen[i] || d > t[i] {
                continue;
            }
            frozen[i] = true;
            let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if !grid.passable(nx, ny) || !allowed(nx as u32, ny as u32) {
                    continue;
                }
                let ni = (ny as u32 * w + nx as u32) as usize;
                if frozen[ni] {
                    continue;
                }
                let c = grid.cost(nx as u32, ny as u32) as u64;
                let a = at(&t, nx - 1, ny).min(at(&t, nx + 1, ny));
                let b = at(&t, nx, ny - 1).min(at(&t, nx, ny + 1));
                let cand = eikonal_update(a, b, c);
                if cand < t[ni] {
                    t[ni] = cand;
                    heap.push(Reverse((cand, ni as u32)));
                }
            }
        }
        Self {
            width: w,
            height: h,
            goal,
            arrival: t,
        }
    }

    #[inline]
    pub fn arrival_at(&self, x: u32, y: u32) -> u64 {
        self.arrival[(y * self.width + x) as usize]
    }

    /// Tile 处的单位方向（指向 `−∇T`）；不可达或已在目标格返回零向量
    pub fn direction(&self, grid: &NavGrid, x: u32, y: u32) -> Vec2 {
        let here = self.arrival_at(x, y);
        if here == UNREACHED || (x, y) == self.goal {
            return Vec2::ZERO;
        }
        let sample = |dx: i64, dy: i64| -> i64 {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if !grid.passable(nx, ny) {
                return here as i64;
            }
            let v = self.arrival_at(nx as u32, ny as u32);
            if v == UNREACHED { here as i64 } else { v as i64 }
        };
        // 迎风分量：只朝更低的一侧下降，两侧都不更低时该轴分量为 0
        let upwind = |lo: i64, hi: i64| -> i64 {
            let h = here as i64;
            if lo.min(hi) >= h {
                0
            } else if lo < hi {
                -(h - lo)
            } else {
                h - hi
            }
        };
        let gx = upwind(sample(-1, 0), sample(1, 0));
        let gy = upwind(sample(0, -1), sample(0, 1));
        if gx != 0 || gy != 0 {
            return Vec2::new(Q32_32(gx << 16), Q32_32(gy << 16)).normalized();
        }
        // 零梯度（鞍点 / 狭窄通道）：取到达时间最小的可走八邻格
        let mut best: Option<(u64, (i32, i32))> = None;
        for (dx, dy) in NEIGHBORS_8 {
            if grid.step_cost(x, y, dx, dy).is_none() {
                continue;
            }
            let v = self.arrival_at((x as i64 + dx as i64) as u32, (y as i64 + dy as i64) as u32);
            if v < here && best.is_none_or(|(b, _)| v < b) {
                best = Some((v, (dx, dy)));
            }
        }
        match best {
            Some((_, (dx, dy))) => Vec2::from_int(dx, dy).normalized(),
            None => Vec2::ZERO,
        }
    }
}

/// 一阶迎风 Eikonal 更新
#[inline]
pub fn eikonal_update(a: u64, b: u64, c: u64) -> u64 {
    match (a == UNREACHED, b == UNREACHED) {
        (true, true) => UNREACHED,
        (false, true) => a + c,
        (true, false) => b + c,
        (false, false) => {
            let diff = a.abs_diff(b);
            if diff >= c {
                a.min(b) + c
            } else {
                (a + b + isqrt_u64(2 * c * c - diff * diff)) / 2
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_gen::TerrainTileKind;

    #[test]
    fn isqrt_is_exact_floor() {
        for v in [0u64, 1, 2, 3, 4, 15, 16, 17, 99, 100, 1 << 40, u32::MAX as u64 * 3] {
            let r = isqrt_u64(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "{v}");
        }
    }

    #[test]
    fn eikonal_diagonal_is_shorter_than_manhattan() {
        let grid = NavGrid::from_terrain(32, 32, &vec![TerrainTileKind::OpenSubstrate; 32 * 32]);
        let f = FlowField::solve(&grid, (0, 0), None);
        assert_eq!(f.arrival_at(10, 0), 10_000);
        let diag = f.arrival_at(10, 10);
        assert!(diag < 20_000 && diag > 14_000, "{diag}");
        // 远离目标处方向指向目标
        let d = f.direction(&grid, 20, 0);
        assert!(d.x.0 < 0 && d.y.0 == 0);
        let d = f.direction(&grid, 10, 10);
        assert!(d.x.0 < 0 && d.y.0 < 0);
    }

    #[test]
    fn flow_goes_around_walls_and_respects_corridor() {
        let mut tiles = vec![TerrainTileKind::OpenSubstrate; 32 * 32];
        for y in 0..28 {
            tiles[y * 32 + 16] = TerrainTileKind::ImperviousWall;
        }
        let grid = NavGrid::from_terrain(32, 32, &tiles);
        let f = FlowField::solve(&grid, (28, 2), None);
        assert_ne!(f.arrival_at(4, 2), UNREACHED);
        // 墙左侧底部单位应先往 +y（缺口方向）走
        let d = f.direction(&grid, 15, 2);
        assert!(d.y.0 > 0, "{d:?}");
        let corridor: BTreeSet<u32> = [1u32].into_iter().collect();
        let f2 = FlowField::solve(&grid, (28, 2), Some(&corridor));
        assert_eq!(f2.arrival_at(4, 2), UNREACHED, "走廊外不求解");
        assert_eq!(f2.arrival_at(28, 3), f.arrival_at(28, 3));
    }
}
