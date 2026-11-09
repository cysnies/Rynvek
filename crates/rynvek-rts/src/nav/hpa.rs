// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 宏观层：`16×16` 扇区分层寻路 HPA*（Botea, Müller & Schaeffer 2004）。
//!
//! - **入口检测**：相邻扇区共享边上两侧都可通行的最大连续段；段长 `≥ 6` 时在两端各放一对过渡点，
//!   否则在中点放一对（原论文的标准规则）。过渡点之间以直线一步相连（扇区间边）。
//! - **扇区内边**：同一扇区内全部过渡点两两之间以「限定在扇区内的 Dijkstra」求精确代价。
//! - **查询**：把起终点临时接入所在扇区的过渡点，在抽象图上 A*，再用扇区内 A* 逐段细化为 Tile 路径；
//!   起终扇区相同或相邻时，还会与「外扩一圈扇区」矩形内的精确 A* 比较取优。
//! - **增量更新**：建筑落成 / 拆除只重建受影响扇区（及其四邻）的入口与扇区内边。
//!
//! 全部容器为 `BTreeMap` / 有序 `Vec`，结果与平台和插入顺序无关。

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use super::grid::{NavGrid, Rect, astar, dijkstra_to, octile};

/// 扇区边长（Tiles）
pub const SECTOR_SIZE: u32 = 16;
/// 入口段长阈值：`≥ 6` 时放两对过渡点
pub const ENTRANCE_SPLIT_LENGTH: u32 = 6;

/// 一条 HPA* 路径
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HpaPath {
    pub cost: u64,
    pub tiles: Vec<(u32, u32)>,
}

impl HpaPath {
    /// 路径经过的扇区（流场走廊）
    pub fn sectors(&self, grid_width: u32) -> BTreeSet<u32> {
        let sx = grid_width.div_ceil(SECTOR_SIZE);
        self.tiles.iter().map(|(x, y)| (y / SECTOR_SIZE) * sx + x / SECTOR_SIZE).collect()
    }
}

/// HPA* 抽象图
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HpaGraph {
    width: u32,
    height: u32,
    sectors_x: u32,
    sectors_y: u32,
    /// 扇区 `s` 与 `+x` 邻居之间的过渡点对
    east: Vec<Vec<((u32, u32), (u32, u32))>>,
    /// 扇区 `s` 与 `+y` 邻居之间的过渡点对
    north: Vec<Vec<((u32, u32), (u32, u32))>>,
    /// 扇区内边 `(a, b, cost)`，`a < b`（按 Tile 线性下标）
    intra: Vec<Vec<(u32, u32, u64)>>,
    /// 合成后的无向邻接表
    adj: BTreeMap<u32, Vec<(u32, u64)>>,
}

impl HpaGraph {
    /// 由导航网格完整构建
    pub fn build(grid: &NavGrid) -> Self {
        let sectors_x = grid.width.div_ceil(SECTOR_SIZE);
        let sectors_y = grid.height.div_ceil(SECTOR_SIZE);
        let n = (sectors_x * sectors_y) as usize;
        let mut g = Self {
            width: grid.width,
            height: grid.height,
            sectors_x,
            sectors_y,
            east: vec![Vec::new(); n],
            north: vec![Vec::new(); n],
            intra: vec![Vec::new(); n],
            adj: BTreeMap::new(),
        };
        for s in 0..n as u32 {
            g.rebuild_borders(grid, s);
        }
        for s in 0..n as u32 {
            g.rebuild_intra(grid, s);
        }
        g.rebuild_adjacency();
        g
    }

    pub const fn sector_count(&self) -> u32 {
        self.sectors_x * self.sectors_y
    }

    /// Tile 所在扇区
    #[inline]
    pub const fn sector_of(&self, x: u32, y: u32) -> u32 {
        (y / SECTOR_SIZE) * self.sectors_x + x / SECTOR_SIZE
    }

    /// 扇区矩形
    pub fn sector_rect(&self, s: u32) -> Rect {
        let (sx, sy) = (s % self.sectors_x, s / self.sectors_x);
        Rect {
            x0: sx * SECTOR_SIZE,
            y0: sy * SECTOR_SIZE,
            x1: ((sx + 1) * SECTOR_SIZE).min(self.width),
            y1: ((sy + 1) * SECTOR_SIZE).min(self.height),
        }
    }

    /// 抽象节点数
    pub fn node_count(&self) -> usize {
        self.adj.len()
    }

    fn rebuild_borders(&mut self, grid: &NavGrid, s: u32) {
        let r = self.sector_rect(s);
        let (sx, sy) = (s % self.sectors_x, s / self.sectors_x);
        self.east[s as usize].clear();
        self.north[s as usize].clear();
        if sx + 1 < self.sectors_x {
            let (xa, xb) = (r.x1 - 1, r.x1);
            let pairs = entrances(r.y0, r.y1, |y| grid.passable(xa as i64, y as i64) && grid.passable(xb as i64, y as i64));
            self.east[s as usize] = pairs.into_iter().map(|y| ((xa, y), (xb, y))).collect();
        }
        if sy + 1 < self.sectors_y {
            let (ya, yb) = (r.y1 - 1, r.y1);
            let pairs = entrances(r.x0, r.x1, |x| grid.passable(x as i64, ya as i64) && grid.passable(x as i64, yb as i64));
            self.north[s as usize] = pairs.into_iter().map(|x| ((x, ya), (x, yb))).collect();
        }
    }

    /// 扇区内全部过渡点（有序去重）
    fn sector_nodes(&self, s: u32) -> Vec<(u32, u32)> {
        let (sx, sy) = (s % self.sectors_x, s / self.sectors_x);
        let mut set = BTreeSet::new();
        for (a, _) in &self.east[s as usize] {
            set.insert((a.1, a.0));
        }
        for (a, _) in &self.north[s as usize] {
            set.insert((a.1, a.0));
        }
        if sx > 0 {
            for (_, b) in &self.east[(s - 1) as usize] {
                set.insert((b.1, b.0));
            }
        }
        if sy > 0 {
            for (_, b) in &self.north[(s - self.sectors_x) as usize] {
                set.insert((b.1, b.0));
            }
        }
        set.into_iter().map(|(y, x)| (x, y)).collect()
    }

    fn rebuild_intra(&mut self, grid: &NavGrid, s: u32) {
        let nodes = self.sector_nodes(s);
        let rect = self.sector_rect(s);
        let mut edges = Vec::new();
        for (i, a) in nodes.iter().enumerate() {
            let rest = &nodes[i + 1..];
            if rest.is_empty() {
                continue;
            }
            for (b, d) in rest.iter().zip(dijkstra_to(grid, *a, rest, rect)) {
                if let Some(d) = d {
                    let (ia, ib) = (grid.index(a.0, a.1) as u32, grid.index(b.0, b.1) as u32);
                    edges.push((ia.min(ib), ia.max(ib), d));
                }
            }
        }
        edges.sort_unstable();
        self.intra[s as usize] = edges;
    }

    fn rebuild_adjacency(&mut self) {
        let w = self.width;
        let idx = |t: (u32, u32)| t.1 * w + t.0;
        let mut adj: BTreeMap<u32, Vec<(u32, u64)>> = BTreeMap::new();
        let mut link = |a: u32, b: u32, c: u64| {
            adj.entry(a).or_default().push((b, c));
            adj.entry(b).or_default().push((a, c));
        };
        for list in self.east.iter().chain(&self.north) {
            for (a, b) in list {
                // 过渡点对为正交相邻，代价 = 两格均值 × 1000‰（构建时两格都可通行）
                link(idx(*a), idx(*b), 0);
            }
        }
        for list in &self.intra {
            for (a, b, c) in list {
                link(*a, *b, *c);
            }
        }
        for v in adj.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        self.adj = adj;
    }

    /// 建筑落成 / 拆除后，增量重建覆盖 `changed` 的扇区及其四邻
    pub fn update_region(&mut self, grid: &NavGrid, changed: Rect) {
        let sx0 = changed.x0.saturating_sub(1) / SECTOR_SIZE;
        let sy0 = changed.y0.saturating_sub(1) / SECTOR_SIZE;
        let sx1 = (changed.x1.min(self.width - 1) / SECTOR_SIZE).min(self.sectors_x - 1);
        let sy1 = (changed.y1.min(self.height - 1) / SECTOR_SIZE).min(self.sectors_y - 1);
        let mut border = BTreeSet::new();
        let mut intra = BTreeSet::new();
        for sy in sy0..=sy1 {
            for sx in sx0..=sx1 {
                let s = sy * self.sectors_x + sx;
                border.insert(s);
                if sx > 0 {
                    border.insert(s - 1);
                }
                if sy > 0 {
                    border.insert(s - self.sectors_x);
                }
            }
        }
        for &s in &border {
            self.rebuild_borders(grid, s);
            let (sx, sy) = (s % self.sectors_x, s / self.sectors_x);
            intra.insert(s);
            if sx + 1 < self.sectors_x {
                intra.insert(s + 1);
            }
            if sy + 1 < self.sectors_y {
                intra.insert(s + self.sectors_x);
            }
        }
        for s in intra {
            self.rebuild_intra(grid, s);
        }
        self.rebuild_adjacency();
    }

    /// 查询路径
    pub fn find_path(&self, grid: &NavGrid, start: (u32, u32), goal: (u32, u32)) -> Option<HpaPath> {
        if !grid.passable(start.0 as i64, start.1 as i64) || !grid.passable(goal.0 as i64, goal.1 as i64) {
            return None;
        }
        let w = self.width;
        let idx = |t: (u32, u32)| t.1 * w + t.0;
        let tile = |i: u32| (i % w, i / w);
        let (ss, gs) = (self.sector_of(start.0, start.1), self.sector_of(goal.0, goal.1));

        // 近距直连候选：起终扇区相同或相邻（切比雪夫距离 ≤ 1）时，在两扇区外扩一圈扇区的矩形内
        // 直接求精确 A*，消除「短路径被迫绕经过渡点」的 HPA* 固有偏差
        let mut best: Option<HpaPath> = None;
        let (ssx, ssy, gsx, gsy) = (ss % self.sectors_x, ss / self.sectors_x, gs % self.sectors_x, gs / self.sectors_x);
        if ssx.abs_diff(gsx) <= 1 && ssy.abs_diff(gsy) <= 1 {
            let rect = Rect {
                x0: ssx.min(gsx).saturating_sub(1) * SECTOR_SIZE,
                y0: ssy.min(gsy).saturating_sub(1) * SECTOR_SIZE,
                x1: ((ssx.max(gsx) + 2) * SECTOR_SIZE).min(self.width),
                y1: ((ssy.max(gsy) + 2) * SECTOR_SIZE).min(self.height),
            };
            if let Some((cost, tiles)) = astar(grid, start, goal, rect) {
                best = Some(HpaPath { cost, tiles });
            }
        }

        // 起终点接入
        const START: u32 = u32::MAX;
        const GOAL: u32 = u32::MAX - 1;
        let s_nodes = self.sector_nodes(ss);
        let g_nodes = self.sector_nodes(gs);
        let s_links: Vec<(u32, u64)> = s_nodes
            .iter()
            .zip(dijkstra_to(grid, start, &s_nodes, self.sector_rect(ss)))
            .filter_map(|(n, d)| d.map(|d| (idx(*n), d)))
            .collect();
        let g_links: BTreeMap<u32, u64> = g_nodes
            .iter()
            .zip(dijkstra_to(grid, goal, &g_nodes, self.sector_rect(gs)))
            .filter_map(|(n, d)| d.map(|d| (idx(*n), d)))
            .collect();

        // 抽象图 A*
        let h = |i: u32| if i == START { octile(start, goal) } else { octile(tile(i), goal) };
        let mut dist: BTreeMap<u32, u64> = BTreeMap::new();
        let mut parent: BTreeMap<u32, u32> = BTreeMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(START, 0);
        heap.push(Reverse((h(START), 0u64, START)));
        while let Some(Reverse((_, d, u))) = heap.pop() {
            if dist.get(&u).is_some_and(|&b| d > b) {
                continue;
            }
            if u == GOAL {
                break;
            }
            if best.as_ref().is_some_and(|b| d >= b.cost) {
                break;
            }
            let mut relax = |v: u32, c: u64, hv: u64, heap: &mut BinaryHeap<_>| {
                let nd = d + c;
                if dist.get(&v).is_none_or(|&b| nd < b) {
                    dist.insert(v, nd);
                    parent.insert(v, u);
                    heap.push(Reverse((nd + hv, nd, v)));
                }
            };
            if u == START {
                for &(v, c) in &s_links {
                    relax(v, c, h(v), &mut heap);
                }
                continue;
            }
            if let Some(&c) = g_links.get(&u) {
                relax(GOAL, c, 0, &mut heap);
            }
            if let Some(edges) = self.adj.get(&u) {
                for &(v, c) in edges {
                    let c = if c == 0 { step_between(grid, tile(u), tile(v)) } else { c };
                    relax(v, c, h(v), &mut heap);
                }
            }
        }

        if let Some(&total) = dist.get(&GOAL) {
            if best.as_ref().is_none_or(|b| total < b.cost) {
                // 回溯抽象节点序列并逐段细化
                let mut chain = vec![goal];
                let mut cur = parent[&GOAL];
                while cur != START {
                    chain.push(tile(cur));
                    cur = parent[&cur];
                }
                chain.push(start);
                chain.reverse();
                let mut tiles = vec![start];
                let mut cost = 0u64;
                for pair in chain.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let (sa, sb) = (self.sector_of(a.0, a.1), self.sector_of(b.0, b.1));
                    let rect = if sa == sb { self.sector_rect(sa) } else { union(self.sector_rect(sa), self.sector_rect(sb)) };
                    let (c, seg) = astar(grid, a, b, rect)?;
                    cost += c;
                    tiles.extend_from_slice(&seg[1..]);
                }
                best = Some(HpaPath { cost, tiles });
            }
        }
        best
    }
}

/// 两个正交相邻过渡点之间的单步代价
fn step_between(grid: &NavGrid, a: (u32, u32), b: (u32, u32)) -> u64 {
    grid.step_cost(a.0, a.1, b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32).unwrap_or(u64::MAX / 4)
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// 在 `[lo, hi)` 上按「可通行段」规则放置过渡点坐标
fn entrances(lo: u32, hi: u32, open: impl Fn(u32) -> bool) -> Vec<u32> {
    let mut out = Vec::new();
    let mut k = lo;
    while k < hi {
        if !open(k) {
            k += 1;
            continue;
        }
        let start = k;
        while k < hi && open(k) {
            k += 1;
        }
        let len = k - start;
        if len >= ENTRANCE_SPLIT_LENGTH {
            out.push(start);
            out.push(k - 1);
        } else {
            out.push(start + len / 2);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_gen::TerrainTileKind;

    #[test]
    fn entrance_rule_follows_botea() {
        assert_eq!(entrances(0, 16, |_| true), vec![0, 15]);
        assert_eq!(entrances(0, 16, |k| (3..8).contains(&k)), vec![5]);
        assert_eq!(entrances(0, 16, |k| k < 2 || k >= 10), vec![1, 10, 15]);
    }

    #[test]
    fn open_field_path_is_near_optimal_and_valid() {
        let grid = NavGrid::from_terrain(64, 64, &vec![TerrainTileKind::OpenSubstrate; 64 * 64]);
        let hpa = HpaGraph::build(&grid);
        let p = hpa.find_path(&grid, (1, 2), (60, 50)).unwrap();
        let (opt, _) = astar(&grid, (1, 2), (60, 50), grid.full_rect()).unwrap();
        assert!(p.cost * 100 <= opt * 125, "{} vs {opt}", p.cost);
        assert_eq!(p.tiles.first(), Some(&(1, 2)));
        assert_eq!(p.tiles.last(), Some(&(60, 50)));
        for w in p.tiles.windows(2) {
            assert!(w[0].0.abs_diff(w[1].0) <= 1 && w[0].1.abs_diff(w[1].1) <= 1);
        }
    }

    #[test]
    fn incremental_update_reroutes_around_new_walls() {
        let mut grid = NavGrid::from_terrain(64, 64, &vec![TerrainTileKind::OpenSubstrate; 64 * 64]);
        let mut hpa = HpaGraph::build(&grid);
        // 在 x = 20 处立起一堵只留 y ∈ [60, 64) 缺口的墙
        for y in 0..60 {
            grid.set_occupied(20, y, true);
        }
        hpa.update_region(&grid, Rect { x0: 20, y0: 0, x1: 21, y1: 60 });
        assert_eq!(hpa, HpaGraph::build(&grid), "增量重建必须与完整重建一致");
        let p = hpa.find_path(&grid, (5, 5), (40, 5)).unwrap();
        assert!(p.tiles.iter().all(|t| grid.passable(t.0 as i64, t.1 as i64)));
        assert!(p.tiles.iter().any(|t| t.1 >= 60));
    }
}
