// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 导航代价网格与精确网格 A*（HPA* 簇内搜索、路径细化与最优性基准共用）。
//!
//! **代价模型（ERRATA E-028）**：
//! - 地形代价 = `1000 / 移速系数`：开阔 `1000`、玄武岩 `909`、沼泽 `1176`；
//!   裂隙移速 1.00x 但每 Tick 灼烧 `1.5` 伤害，取 `4000` 让寻路尽量绕行；墙体与建筑占格不可通行。
//! - 单步代价取起止两格地形代价的均值 × 方向系数，使 `cost(a→b) = cost(b→a)`（HPA* 抽象图可用无向边）。
//! - 方向系数：正交 `1000`、对角 `1414`（‰）；对角移动禁止切角（两个正交邻格都须可通行）。
//! - 启发式：八方向 Octile 距离 × 最小地形代价 `909`，可采纳（admissible）。
//!
//! 优先队列以 `(f, h, tile_index)` 全序出队，保证同代价路径的选择与平台无关。

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::map_gen::TerrainTileKind;

/// 不可通行标记
pub const BLOCKED: u16 = 0;
/// 正交 / 对角方向系数（‰）
pub const STRAIGHT: u64 = 1000;
pub const DIAGONAL: u64 = 1414;
/// 可通行地形的最小代价（启发式下界）
pub const MIN_TILE_COST: u64 = 909;

/// 地形基础代价
pub const fn terrain_cost(kind: TerrainTileKind) -> u16 {
    match kind {
        TerrainTileKind::OpenSubstrate => 1000,
        TerrainTileKind::ConductiveBasalt => 909,
        TerrainTileKind::NutrientMarsh => 1176,
        TerrainTileKind::AcidicFissure => 4000,
        TerrainTileKind::ImperviousWall => BLOCKED,
    }
}

/// 八邻域偏移（固定顺序：先正交后对角）
pub const NEIGHBORS_8: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)];

/// 矩形区域（半开区间 `[x0, x1) × [y0, y1)`）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Rect {
    #[inline]
    pub const fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.x0 as i64 && y >= self.y0 as i64 && x < self.x1 as i64 && y < self.y1 as i64
    }
}

/// 导航代价网格
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavGrid {
    pub width: u32,
    pub height: u32,
    base: Vec<u16>,
    /// 建筑占格计数（> 0 即阻塞）
    occupancy: Vec<u8>,
}

impl NavGrid {
    /// 由地形构建
    pub fn from_terrain(width: u32, height: u32, tiles: &[TerrainTileKind]) -> Self {
        assert_eq!(tiles.len(), (width * height) as usize);
        Self {
            width,
            height,
            base: tiles.iter().map(|t| terrain_cost(*t)).collect(),
            occupancy: vec![0; tiles.len()],
        }
    }

    #[inline]
    pub const fn index(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    #[inline]
    pub const fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64
    }

    /// 当前代价（`0` = 不可通行）
    #[inline]
    pub fn cost(&self, x: u32, y: u32) -> u16 {
        let i = self.index(x, y);
        if self.occupancy[i] > 0 { BLOCKED } else { self.base[i] }
    }

    #[inline]
    pub fn passable(&self, x: i64, y: i64) -> bool {
        self.in_bounds(x, y) && self.cost(x as u32, y as u32) != BLOCKED
    }

    /// 建筑落成 / 拆除时增减占格
    pub fn set_occupied(&mut self, x: u32, y: u32, occupied: bool) {
        let i = self.index(x, y);
        if occupied {
            self.occupancy[i] = self.occupancy[i].saturating_add(1);
        } else {
            self.occupancy[i] = self.occupancy[i].saturating_sub(1);
        }
    }

    /// 从 `(x, y)` 沿 `(dx, dy)` 走一步的代价；不可走（越界 / 阻塞 / 切角）返回 `None`
    #[inline]
    pub fn step_cost(&self, x: u32, y: u32, dx: i32, dy: i32) -> Option<u64> {
        let (nx, ny) = (x as i64 + dx as i64, y as i64 + dy as i64);
        if !self.passable(nx, ny) {
            return None;
        }
        if dx != 0
            && dy != 0
            && (!self.passable(x as i64 + dx as i64, y as i64) || !self.passable(x as i64, y as i64 + dy as i64))
        {
            return None;
        }
        let c = (self.cost(x, y) as u64 + self.cost(nx as u32, ny as u32) as u64) / 2;
        Some(if dx != 0 && dy != 0 {
            c * DIAGONAL / 1000
        } else {
            c * STRAIGHT / 1000
        })
    }

    /// 整图矩形
    pub const fn full_rect(&self) -> Rect {
        Rect {
            x0: 0,
            y0: 0,
            x1: self.width,
            y1: self.height,
        }
    }
}

/// Octile 启发式
#[inline]
pub fn octile(a: (u32, u32), b: (u32, u32)) -> u64 {
    let dx = a.0.abs_diff(b.0) as u64;
    let dy = a.1.abs_diff(b.1) as u64;
    let (mx, mn) = if dx > dy { (dx, dy) } else { (dy, dx) };
    MIN_TILE_COST * (STRAIGHT * mx + (DIAGONAL - STRAIGHT) * mn) / 1000
}

/// 限定在 `bounds` 内的网格 A*；返回 `(总代价, 含起终点的 Tile 路径)`
pub fn astar(grid: &NavGrid, start: (u32, u32), goal: (u32, u32), bounds: Rect) -> Option<(u64, Vec<(u32, u32)>)> {
    if !grid.passable(start.0 as i64, start.1 as i64) || !grid.passable(goal.0 as i64, goal.1 as i64) {
        return None;
    }
    if start == goal {
        return Some((0, vec![start]));
    }
    let w = grid.width as usize;
    let n = (grid.width * grid.height) as usize;
    let mut g = vec![u64::MAX; n];
    let mut parent = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    let si = grid.index(start.0, start.1);
    g[si] = 0;
    heap.push(Reverse((octile(start, goal), octile(start, goal), si as u32)));
    while let Some(Reverse((_, _, i))) = heap.pop() {
        let i = i as usize;
        let (x, y) = ((i % w) as u32, (i / w) as u32);
        if (x, y) == goal {
            let mut path = vec![goal];
            let mut c = i;
            while parent[c] != u32::MAX {
                c = parent[c] as usize;
                path.push(((c % w) as u32, (c / w) as u32));
            }
            path.reverse();
            return Some((g[i], path));
        }
        for (dx, dy) in NEIGHBORS_8 {
            let (nx, ny) = (x as i64 + dx as i64, y as i64 + dy as i64);
            if !bounds.contains(nx, ny) {
                continue;
            }
            let Some(c) = grid.step_cost(x, y, dx, dy) else {
                continue;
            };
            let ni = grid.index(nx as u32, ny as u32);
            let ng = g[i] + c;
            if ng < g[ni] {
                g[ni] = ng;
                parent[ni] = i as u32;
                let h = octile((nx as u32, ny as u32), goal);
                heap.push(Reverse((ng + h, h, ni as u32)));
            }
        }
    }
    None
}

/// 限定区域内的单源 Dijkstra（返回到 `targets` 中每个可达点的代价）
pub fn dijkstra_to(grid: &NavGrid, start: (u32, u32), targets: &[(u32, u32)], bounds: Rect) -> Vec<Option<u64>> {
    let w = grid.width as usize;
    let mut dist: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
    let mut remaining: std::collections::BTreeSet<u32> = targets.iter().map(|t| grid.index(t.0, t.1) as u32).collect();
    let mut heap = BinaryHeap::new();
    let si = grid.index(start.0, start.1) as u32;
    if grid.passable(start.0 as i64, start.1 as i64) {
        dist.insert(si, 0);
        heap.push(Reverse((0u64, si)));
    }
    while let Some(Reverse((d, i))) = heap.pop() {
        if dist.get(&i).is_some_and(|&best| d > best) {
            continue;
        }
        remaining.remove(&i);
        if remaining.is_empty() {
            break;
        }
        let (x, y) = ((i as usize % w) as u32, (i as usize / w) as u32);
        for (dx, dy) in NEIGHBORS_8 {
            let (nx, ny) = (x as i64 + dx as i64, y as i64 + dy as i64);
            if !bounds.contains(nx, ny) {
                continue;
            }
            let Some(c) = grid.step_cost(x, y, dx, dy) else {
                continue;
            };
            let ni = grid.index(nx as u32, ny as u32) as u32;
            let nd = d + c;
            if dist.get(&ni).is_none_or(|&best| nd < best) {
                dist.insert(ni, nd);
                heap.push(Reverse((nd, ni)));
            }
        }
    }
    targets
        .iter()
        .map(|t| dist.get(&(grid.index(t.0, t.1) as u32)).copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_grid(w: u32, h: u32) -> NavGrid {
        NavGrid::from_terrain(w, h, &vec![TerrainTileKind::OpenSubstrate; (w * h) as usize])
    }

    #[test]
    fn straight_and_diagonal_costs() {
        let g = open_grid(32, 32);
        let (c, p) = astar(&g, (0, 0), (10, 0), g.full_rect()).unwrap();
        assert_eq!(c, 10_000);
        assert_eq!(p.len(), 11);
        let (c, _) = astar(&g, (0, 0), (5, 5), g.full_rect()).unwrap();
        assert_eq!(c, 5 * 1414);
    }

    #[test]
    fn no_corner_cutting_and_walls_block() {
        let mut g = open_grid(8, 8);
        g.set_occupied(1, 0, true);
        assert_eq!(g.step_cost(0, 0, 1, 1), None, "禁止切角");
        for y in 0..8 {
            g.set_occupied(4, y, true);
        }
        assert!(astar(&g, (0, 0), (7, 7), g.full_rect()).is_none());
        g.set_occupied(4, 7, false);
        assert!(astar(&g, (0, 0), (7, 7), g.full_rect()).is_some());
    }

    #[test]
    fn dijkstra_agrees_with_astar() {
        let g = open_grid(16, 16);
        let d = dijkstra_to(&g, (2, 3), &[(9, 14), (2, 3), (15, 0)], g.full_rect());
        assert_eq!(d[0], Some(astar(&g, (2, 3), (9, 14), g.full_rect()).unwrap().0));
        assert_eq!(d[1], Some(0));
        assert_eq!(d[2], Some(astar(&g, (2, 3), (15, 0), g.full_rect()).unwrap().0));
    }
}
