// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 竞技地图：四种标准尺寸、出生锚点、180° 对称性校验与双出生点连通性验证（§5.5.1）。
//!
//! 地形由规范冻结的 [`generate_symmetric_arena_map`] 生成；本模块只做包装与校验，不改变生成结果。
//!
//! **连通性验证的形式化（ERRATA E-022）**：设计圣经要求「双出生点之间至少存在 2 条宽度 ≥ 6 Tiles
//! 的连通路径」。这里按 Menger 定理把「路径宽度之和」形式化为 4-邻接可通行图上两出生点之间的
//! **最小顶点割** `κ(A, B)`：`κ ≥ 2 × 6 = 12` 当且仅当任意一条把两出生点分隔开的截面上至少有
//! 12 个可通行 Tile，即至少容纳两条互不重叠、各宽 6 Tiles 的通路。最小顶点割用单位顶点容量的
//! 最大流（BFS 增广，至多 12 轮）精确求出。采用 4-邻接而非 8-邻接，保证不会把单位无法挤过的
//! 对角墙缝计入宽度（保守）。

use rynvek_core::Blake3Hash;
use serde::{Deserialize, Serialize};

use crate::map_gen::{TerrainTileKind, generate_symmetric_arena_map};
use crate::math::Vec2;
use crate::terrain::is_passable;

/// 规范冻结的出生锚点偏移：左下角出生点位于 Tile `(32, 32)`
pub const SPAWN_ANCHOR_TILE: u32 = 32;
/// 出生安全平地半径（Tiles）
pub const SPAWN_SAFE_RADIUS: u32 = 20;
/// 连通性验证：每条通路的最小宽度（Tiles）
pub const MIN_CORRIDOR_WIDTH: u32 = 6;
/// 连通性验证：双出生点之间要求的通路条数
pub const REQUIRED_CORRIDORS: u32 = 2;

/// 四种标准地图尺寸（§5.5.1 第 1 条）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArenaMapSize {
    /// `256 × 256 Tiles`（1×1 Chunk）
    #[serde(rename = "Duel_1v1")]
    Duel1v1,
    /// `512 × 512 Tiles`（2×2 Chunks）
    #[serde(rename = "Skirmish_2v2")]
    Skirmish2v2,
    /// `768 × 768 Tiles`（3×3 Chunks）
    #[serde(rename = "Arena_4P")]
    Arena4P,
    /// `2048 × 2048 Tiles`（8×8 Chunks，每 Partition 4×4 Chunks）
    #[serde(rename = "Persistent_Sector")]
    PersistentSector,
}

impl ArenaMapSize {
    pub const ALL: [Self; 4] = [Self::Duel1v1, Self::Skirmish2v2, Self::Arena4P, Self::PersistentSector];

    /// 边长（Tiles）
    pub const fn side_tiles(self) -> u32 {
        match self {
            Self::Duel1v1 => 256,
            Self::Skirmish2v2 => 512,
            Self::Arena4P => 768,
            Self::PersistentSector => 2048,
        }
    }

    /// 每边 Chunk 数（`side / 256`）
    pub const fn side_chunks(self) -> u32 {
        self.side_tiles() / rynvek_core::constants::CHUNK_SIZE_UNITS
    }

    /// 规范名称
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Duel1v1 => "Duel_1v1",
            Self::Skirmish2v2 => "Skirmish_2v2",
            Self::Arena4P => "Arena_4P",
            Self::PersistentSector => "Persistent_Sector",
        }
    }
}

/// 一张已生成的竞技地图
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaMap {
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    pub tiles: Vec<TerrainTileKind>,
}

impl ArenaMap {
    /// 按标准尺寸生成
    pub fn generate(size: ArenaMapSize, seed: u64) -> Self {
        Self::generate_custom(size.side_tiles(), size.side_tiles(), seed)
    }

    /// 按任意偶数尺寸生成（测试与自定义房间）；尺寸必须容纳两处出生安全区
    pub fn generate_custom(width: u32, height: u32, seed: u64) -> Self {
        assert!(
            width >= 2 * (SPAWN_ANCHOR_TILE + SPAWN_SAFE_RADIUS) && height >= 2 * (SPAWN_ANCHOR_TILE + SPAWN_SAFE_RADIUS),
            "地图过小，无法容纳两处出生安全区"
        );
        let tiles = generate_symmetric_arena_map(width as usize, height as usize, seed);
        Self {
            width,
            height,
            seed,
            tiles,
        }
    }

    /// 由已有 Tile 数组构造（测试与 Studio 自定义地形）
    pub fn from_tiles(width: u32, height: u32, tiles: Vec<TerrainTileKind>) -> Self {
        assert_eq!(tiles.len(), (width * height) as usize);
        Self {
            width,
            height,
            seed: 0,
            tiles,
        }
    }

    #[inline]
    pub fn index(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    #[inline]
    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64
    }

    #[inline]
    pub fn tile(&self, x: u32, y: u32) -> TerrainTileKind {
        self.tiles[self.index(x, y)]
    }

    /// 180° 中心对称点 `(W − 1 − x, H − 1 − y)`
    #[inline]
    pub const fn mirror_tile(&self, x: u32, y: u32) -> (u32, u32) {
        (self.width - 1 - x, self.height - 1 - y)
    }

    /// 连续坐标的中心反演 `(W − px, H − py)`（与 Tile 镜像一致：Tile 中心 `x + 0.5 ↦ W − x − 0.5`）
    pub fn mirror_point(&self, p: Vec2) -> Vec2 {
        Vec2::from_int(self.width as i32, self.height as i32) - p
    }

    /// 是否 100% 中心旋转对称
    pub fn is_point_symmetric(&self) -> bool {
        (0..self.height).all(|y| {
            (0..self.width).all(|x| {
                let (mx, my) = self.mirror_tile(x, y);
                self.tile(x, y) == self.tile(mx, my)
            })
        })
    }

    /// 两处出生锚点（世界坐标）：`(32, 32)` 与其中心反演点 `(W − 32, H − 32)`
    pub fn spawn_anchors(&self) -> [Vec2; 2] {
        let a = Vec2::from_int(SPAWN_ANCHOR_TILE as i32, SPAWN_ANCHOR_TILE as i32);
        [a, self.mirror_point(a)]
    }

    /// 两处出生锚点所在的出生 Tile（规范生成器的安全区圆心 `(32, 32)` 及其镜像）
    pub fn spawn_tiles(&self) -> [(u32, u32); 2] {
        let a = (SPAWN_ANCHOR_TILE, SPAWN_ANCHOR_TILE);
        [a, self.mirror_tile(a.0, a.1)]
    }

    /// 各地形 Tile 数量（按 `repr(u8)` 顺序）
    pub fn terrain_histogram(&self) -> [u32; 5] {
        let mut h = [0u32; 5];
        for t in &self.tiles {
            h[*t as usize] += 1;
        }
        h
    }

    /// 地形字节（每 Tile 1 字节 `repr(u8)`，行主序）
    pub fn tile_bytes(&self) -> Vec<u8> {
        self.tiles.iter().map(|t| *t as u8).collect()
    }

    /// 地形内容哈希（跨语言 Golden 比对用）
    pub fn tiles_hash(&self) -> Blake3Hash {
        Blake3Hash::of(&self.tile_bytes())
    }

    /// PvE 野生虫潮的四个外围边缘刷新锚点（上、下、左、右边缘中点附近最近的可通行非裂隙 Tile）
    ///
    /// 规范只说「地图外围四个边缘裂隙」，未给坐标；这里取四条边的中点并向内做确定性最近搜索
    /// （按切比雪夫距离、再按 `(y, x)` 升序打破平局），保证单位刷新点可通行且不在灼烧地形上（ERRATA E-026）。
    pub fn edge_rift_anchors(&self) -> [(u32, u32); 4] {
        let (w, h) = (self.width, self.height);
        [(w / 2, 0), (w / 2, h - 1), (0, h / 2), (w - 1, h / 2)].map(|(x, y)| self.nearest_open_tile(x, y))
    }

    /// 由 `(x, y)` 出发按切比雪夫环逐层搜索最近的可通行、非裂隙 Tile
    pub fn nearest_open_tile(&self, x: u32, y: u32) -> (u32, u32) {
        let max_r = self.width.max(self.height) as i64;
        for r in 0..=max_r {
            let mut best: Option<(u32, u32)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if !self.in_bounds(nx, ny) {
                        continue;
                    }
                    let (nx, ny) = (nx as u32, ny as u32);
                    let t = self.tile(nx, ny);
                    if is_passable(t) && t != TerrainTileKind::AcidicFissure {
                        let cand = (nx, ny);
                        if best.is_none_or(|b| (cand.1, cand.0) < (b.1, b.0)) {
                            best = Some(cand);
                        }
                    }
                }
            }
            if let Some(b) = best {
                return b;
            }
        }
        (x, y)
    }

    /// 双出生点连通性验证（见模块文档）
    pub fn validate_connectivity(&self) -> ConnectivityReport {
        let [a, b] = self.spawn_tiles();
        let required = MIN_CORRIDOR_WIDTH * REQUIRED_CORRIDORS;
        let min_cut = min_vertex_cut_4(self, a, b, SPAWN_SAFE_RADIUS, required);
        ConnectivityReport {
            min_vertex_cut: min_cut,
            required,
            ok: min_cut >= required,
        }
    }
}

/// 连通性验证结果
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectivityReport {
    /// 两出生点之间的最小顶点割（不超过 `required` 时为精确值，否则截断为 `required`）
    pub min_vertex_cut: u32,
    /// 要求值 `2 × 6 = 12`
    pub required: u32,
    pub ok: bool,
}

/// 4-邻接可通行图上两处出生区之间的最小顶点割，至多计算到 `limit`
///
/// 出生区为以 `a` / `b` 为圆心、半径 `radius` 的可通行 Tile 集合（容量无穷，等价于超级源 / 汇；
/// 若只用单个 Tile 作源，割值会被其 4 个邻居截断为 4）。
///
/// 拆点最大流：每个 Tile `v` 拆为入点 / 出点，`in(v) → out(v)` 容量 1；相邻可通行 Tile 之间
/// `out(v) → in(u)` 容量无穷。逐轮 BFS 找增广路（邻居按 东、西、北、南 的固定顺序扩展，结果确定）。
pub fn min_vertex_cut_4(map: &ArenaMap, a: (u32, u32), b: (u32, u32), radius: u32, limit: u32) -> u32 {
    let w = map.width as usize;
    let n = map.tiles.len();
    let passable: Vec<bool> = map.tiles.iter().map(|t| is_passable(*t)).collect();
    // 区域标记：1 = 源区，2 = 汇区（均为无穷容量顶点）
    let mut region = vec![0u8; n];
    let r2 = (radius as i64) * (radius as i64);
    for (tag, c) in [(1u8, a), (2u8, b)] {
        for y in 0..map.height {
            for x in 0..map.width {
                let (dx, dy) = (x as i64 - c.0 as i64, y as i64 - c.1 as i64);
                let i = map.index(x, y);
                if dx * dx + dy * dy <= r2 && passable[i] {
                    region[i] = tag;
                }
            }
        }
    }
    let sources: Vec<usize> = (0..n).filter(|&i| region[i] == 1).collect();
    if sources.is_empty() || !region.contains(&2) {
        return 0;
    }
    // 顶点流 f_v ∈ {0, 1}；边净流 g[v][d]（v 向方向 d 的邻居的净流量，反对称）
    let mut vertex_flow = vec![false; n];
    let mut edge_flow = vec![[0i32; 4]; n];
    let neighbors = |v: usize, d: usize| -> Option<usize> {
        let (x, y) = (v % w, v / w);
        match d {
            0 if x + 1 < w => Some(v + 1),
            1 if x > 0 => Some(v - 1),
            2 if y + 1 < n / w => Some(v + w),
            3 if y > 0 => Some(v - w),
            _ => None,
        }
    };
    const OPPOSITE: [usize; 4] = [1, 0, 3, 2];
    // BFS 状态编码：2v 为入点，2v+1 为出点；parent 记录前驱状态
    let mut flow = 0u32;
    let mut parent = vec![u32::MAX; 2 * n];
    let mut queue = std::collections::VecDeque::new();
    while flow < limit {
        parent.fill(u32::MAX);
        queue.clear();
        // 多源 BFS：所有源区 Tile 的出点作为起点（前驱指向自身）
        for &s in &sources {
            parent[2 * s + 1] = (2 * s + 1) as u32;
            queue.push_back(2 * s + 1);
        }
        let mut sink_hit = None;
        while let Some(s) = queue.pop_front() {
            let v = s / 2;
            if s % 2 == 0 {
                // 入点：抵达汇区即找到增广路
                if region[v] == 2 {
                    sink_hit = Some(s);
                    break;
                }
                let out = 2 * v + 1;
                if (!vertex_flow[v]) && parent[out] == u32::MAX {
                    parent[out] = s as u32;
                    queue.push_back(out);
                }
                // 反向残量：若邻居 u 曾经向 v 输送流量（g[u→v] > 0），可退回到 out(u)
                for d in 0..4 {
                    if let Some(u) = neighbors(v, d) {
                        if edge_flow[u][OPPOSITE[d]] > 0 && parent[2 * u + 1] == u32::MAX {
                            parent[2 * u + 1] = s as u32;
                            queue.push_back(2 * u + 1);
                        }
                    }
                }
            } else {
                // 出点：流向任意可通行邻居的入点（容量无穷）
                for d in 0..4 {
                    if let Some(u) = neighbors(v, d) {
                        if passable[u] && parent[2 * u] == u32::MAX {
                            parent[2 * u] = s as u32;
                            queue.push_back(2 * u);
                        }
                    }
                }
                // 反向残量：已占用的顶点可退回到自身入点
                if vertex_flow[v] && parent[2 * v] == u32::MAX {
                    parent[2 * v] = s as u32;
                    queue.push_back(2 * v);
                }
            }
        }
        let Some(mut s) = sink_hit else {
            break;
        };
        // 沿前驱回溯增广 1 单位流量（直到某个源区出点，其前驱为自身）
        while parent[s] as usize != s {
            let p = parent[s] as usize;
            let (pv, sv) = (p / 2, s / 2);
            if pv == sv {
                // 同一 Tile 的入点 ↔ 出点
                if p % 2 == 0 {
                    vertex_flow[pv] = true;
                } else {
                    vertex_flow[pv] = false;
                }
            } else {
                // 跨 Tile：p 为 out(pv) → s 为 in(sv) 正向；或 p 为 in(pv) → s 为 out(sv) 反向退流
                let d = (0..4).find(|&d| neighbors(pv, d) == Some(sv)).expect("相邻 Tile");
                edge_flow[pv][d] += 1;
                edge_flow[sv][OPPOSITE[d]] -= 1;
            }
            s = p;
        }
        flow += 1;
    }
    flow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_sizes_match_design_bible() {
        let sides: Vec<_> = ArenaMapSize::ALL.iter().map(|s| (s.as_str(), s.side_tiles(), s.side_chunks())).collect();
        assert_eq!(
            sides,
            [("Duel_1v1", 256, 1), ("Skirmish_2v2", 512, 2), ("Arena_4P", 768, 3), ("Persistent_Sector", 2048, 8)]
        );
    }

    fn corridor_map(gap: u32) -> ArenaMap {
        // 160×160 全开阔，中间一道横墙，只留宽度 `gap` 的缺口
        let (w, h) = (160u32, 160u32);
        let mut tiles = vec![TerrainTileKind::OpenSubstrate; (w * h) as usize];
        for x in 0..w {
            if !(10..10 + gap).contains(&x) {
                tiles[(80 * w + x) as usize] = TerrainTileKind::ImperviousWall;
            }
        }
        ArenaMap::from_tiles(w, h, tiles)
    }

    #[test]
    fn min_vertex_cut_equals_gap_width() {
        for gap in [0u32, 1, 5, 11, 12, 20] {
            let m = corridor_map(gap);
            let report = m.validate_connectivity();
            assert_eq!(report.min_vertex_cut, gap.min(12), "缺口宽度 {gap}");
            assert_eq!(report.ok, gap >= 12);
        }
    }

    #[test]
    fn two_separate_six_wide_corridors_pass() {
        let (w, h) = (160u32, 160u32);
        let mut tiles = vec![TerrainTileKind::OpenSubstrate; (w * h) as usize];
        for x in 0..w {
            let open = (10..16).contains(&x) || (120..126).contains(&x);
            if !open {
                tiles[(80 * w + x) as usize] = TerrainTileKind::ImperviousWall;
            }
        }
        let m = ArenaMap::from_tiles(w, h, tiles);
        assert!(m.validate_connectivity().ok);
        // 只剩一条 6 宽通路时不达标
        let mut single = m.clone();
        for x in 120..126 {
            let i = single.index(x, 80);
            single.tiles[i] = TerrainTileKind::ImperviousWall;
        }
        assert_eq!(single.validate_connectivity().min_vertex_cut, 6);
    }
}
