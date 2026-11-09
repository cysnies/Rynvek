// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端战争迷雾（GAP-12）：Tile 分辨率的对称阴影投射（Symmetric Shadowcasting, A. Ford 2021）
//! 与 `64×64`/Chunk 的 `VisibilityGrid` 聚合。
//!
//! - **对称性**：A 能看见 B 当且仅当 B 能看见 A（同半径时），杜绝「单向透视」不公；
//! - **全整数**：斜率以有理数 `(num, den)` 表示，取整按 Ford 原文 `round_ties_up / round_ties_down`
//!   用整数地板除实现，零浮点；
//! - **半径**：只揭示欧氏距离 `dx² + dy² ≤ r²` 的格；墙格本身可见（看见山体表面）但阻挡其后视线；
//! - **聚合**：每 `4×4` Tiles 合成一个可见性格（每 Chunk `64×64`），任一 Tile 可见即该格可见；
//!   网络层（M7）据此在序列化阶段剔除不可见敌方移动单位，并对敌方建筑下发静态幽灵快照。

use serde::{Deserialize, Serialize};

use crate::ids::{BuildingKind, EntitySerial, FactionId};

/// 可见性聚合格边长（Tiles）：`256 / 4 = 64` → 每 Chunk `64×64`
pub const VISIBILITY_CELL_TILES: u32 = 4;

/// 有理斜率
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slope {
    num: i64,
    den: i64,
}

#[inline]
fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) { q - 1 } else { q }
}

#[inline]
fn ceil_div(a: i64, b: i64) -> i64 {
    -floor_div(-a, b)
}

/// `round_ties_up(depth · s) = floor(depth·s + 1/2)`
#[inline]
fn round_ties_up(depth: i64, s: Slope) -> i64 {
    floor_div(2 * depth * s.num + s.den, 2 * s.den)
}

/// `round_ties_down(depth · s) = ceil(depth·s − 1/2)`
#[inline]
fn round_ties_down(depth: i64, s: Slope) -> i64 {
    ceil_div(2 * depth * s.num - s.den, 2 * s.den)
}

/// 计算视野：对每个可见格调用 `reveal(x, y)`（可能重复调用同一格）
///
/// `blocks(x, y)` 对越界坐标应返回 `true`。
pub fn shadowcast(
    origin: (i64, i64),
    radius: u32,
    blocks: impl Fn(i64, i64) -> bool,
    mut reveal: impl FnMut(i64, i64),
) {
    reveal(origin.0, origin.1);
    let r = radius as i64;
    let r2 = r * r;
    for quadrant in 0..4u8 {
        let transform = |depth: i64, col: i64| -> (i64, i64) {
            match quadrant {
                0 => (origin.0 + col, origin.1 - depth),
                1 => (origin.0 + col, origin.1 + depth),
                2 => (origin.0 + depth, origin.1 + col),
                _ => (origin.0 - depth, origin.1 + col),
            }
        };
        // 行栈：(depth, start_slope, end_slope)
        let mut stack = vec![(1i64, Slope { num: -1, den: 1 }, Slope { num: 1, den: 1 })];
        while let Some((depth, mut start, end)) = stack.pop() {
            if depth > r {
                continue;
            }
            let min_col = round_ties_up(depth, start);
            let max_col = round_ties_down(depth, end);
            let mut prev: Option<bool> = None; // Some(is_wall)
            for col in min_col..=max_col {
                let (x, y) = transform(depth, col);
                let wall = blocks(x, y);
                // 对称判定：col ∈ [depth·start, depth·end]
                let symmetric = col * start.den >= depth * start.num && col * end.den <= depth * end.num;
                if (wall || symmetric) && depth * depth + col * col <= r2 {
                    reveal(x, y);
                }
                if prev == Some(true) && !wall {
                    start = Slope {
                        num: 2 * col - 1,
                        den: 2 * depth,
                    };
                }
                if prev == Some(false) && wall {
                    stack.push((
                        depth + 1,
                        start,
                        Slope {
                            num: 2 * col - 1,
                            den: 2 * depth,
                        },
                    ));
                }
                prev = Some(wall);
            }
            if prev == Some(false) {
                stack.push((depth + 1, start, end));
            }
        }
    }
}

/// 单阵营的可见 / 已探索状态
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactionFog {
    pub width: u32,
    pub height: u32,
    visible: Vec<u64>,
    explored: Vec<u64>,
}

impl FactionFog {
    pub fn new(width: u32, height: u32) -> Self {
        let words = ((width * height) as usize).div_ceil(64);
        Self {
            width,
            height,
            visible: vec![0; words],
            explored: vec![0; words],
        }
    }

    /// 新一轮视野计算前清空可见位（已探索保留）
    pub fn begin_frame(&mut self) {
        self.visible.fill(0);
    }

    #[inline]
    fn bit(&self, x: u32, y: u32) -> (usize, u64) {
        let i = (y * self.width + x) as usize;
        (i / 64, 1u64 << (i % 64))
    }

    #[inline]
    pub fn mark(&mut self, x: u32, y: u32) {
        let (w, b) = self.bit(x, y);
        self.visible[w] |= b;
        self.explored[w] |= b;
    }

    #[inline]
    pub fn is_visible(&self, x: u32, y: u32) -> bool {
        let (w, b) = self.bit(x, y);
        self.visible[w] & b != 0
    }

    #[inline]
    pub fn is_explored(&self, x: u32, y: u32) -> bool {
        let (w, b) = self.bit(x, y);
        self.explored[w] & b != 0
    }

    /// 以 `origin` 为中心、`radius` 为半径揭示视野
    pub fn reveal_from(&mut self, origin: (u32, u32), radius: u32, blocks: impl Fn(i64, i64) -> bool) {
        let (w, h) = (self.width as i64, self.height as i64);
        let mut hits = Vec::new();
        shadowcast(
            (origin.0 as i64, origin.1 as i64),
            radius,
            |x, y| x < 0 || y < 0 || x >= w || y >= h || blocks(x, y),
            |x, y| {
                if x >= 0 && y >= 0 && x < w && y < h {
                    hits.push((x as u32, y as u32));
                }
            },
        );
        for (x, y) in hits {
            self.mark(x, y);
        }
    }

    /// 矩形内是否有任一可见 Tile
    pub fn any_visible(&self, x0: u32, y0: u32, w: u32, h: u32) -> bool {
        (y0..(y0 + h).min(self.height)).any(|y| (x0..(x0 + w).min(self.width)).any(|x| self.is_visible(x, y)))
    }

    /// 可见 Tile 数
    pub fn visible_count(&self) -> u32 {
        self.visible.iter().map(|w| w.count_ones()).sum()
    }

    /// `4×4` 聚合可见性格（行优先，`1` = 可见，`2` = 仅已探索，`0` = 未探索）
    pub fn visibility_cells(&self) -> Vec<u8> {
        let (cw, ch) = (
            self.width.div_ceil(VISIBILITY_CELL_TILES),
            self.height.div_ceil(VISIBILITY_CELL_TILES),
        );
        let mut out = vec![0u8; (cw * ch) as usize];
        for y in 0..self.height {
            for x in 0..self.width {
                let c = ((y / VISIBILITY_CELL_TILES) * cw + x / VISIBILITY_CELL_TILES) as usize;
                if self.is_visible(x, y) {
                    out[c] = 1;
                } else if self.is_explored(x, y) && out[c] == 0 {
                    out[c] = 2;
                }
            }
        }
        out
    }
}

/// 离开视野后保留的敌方建筑静态幽灵快照（`EntityVisibilityLost`）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingGhost {
    pub entity_id: EntitySerial,
    pub faction: FactionId,
    pub kind: BuildingKind,
    pub origin: (u32, u32),
    pub last_seen_tick: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn fov(walls: &BTreeSet<(i64, i64)>, o: (i64, i64), r: u32) -> BTreeSet<(i64, i64)> {
        let mut seen = BTreeSet::new();
        shadowcast(
            o,
            r,
            |x, y| walls.contains(&(x, y)) || !(0..40).contains(&x) || !(0..40).contains(&y),
            |x, y| {
                seen.insert((x, y));
            },
        );
        seen
    }

    #[test]
    fn open_field_reveals_exact_disc() {
        let seen = fov(&BTreeSet::new(), (20, 20), 5);
        let expect: BTreeSet<_> = (15..=25)
            .flat_map(|x| (15..=25).map(move |y| (x, y)))
            .filter(|(x, y)| (x - 20) * (x - 20) + (y - 20) * (y - 20) <= 25)
            .collect();
        assert_eq!(seen, expect);
    }

    #[test]
    fn walls_cast_shadows_and_are_themselves_visible() {
        let walls: BTreeSet<_> = (18..=22).map(|x| (x, 17)).collect();
        let seen = fov(&walls, (20, 20), 10);
        assert!(seen.contains(&(20, 17)), "墙面可见");
        assert!(!seen.contains(&(20, 14)), "墙后被遮挡");
        assert!(seen.contains(&(28, 20)));
    }

    #[test]
    fn visibility_is_symmetric() {
        let walls: BTreeSet<_> = [(10, 10), (11, 10), (12, 12), (15, 9), (9, 14), (13, 15), (16, 13)]
            .into_iter()
            .collect();
        let r = 9;
        for a in [(8i64, 8i64), (12, 11), (14, 14)] {
            let sa = fov(&walls, a, r);
            for b in &sa {
                if walls.contains(b) {
                    continue;
                }
                assert!(fov(&walls, *b, r).contains(&a), "{a:?} 看见 {b:?} 但反之不成立");
            }
        }
    }

    #[test]
    fn faction_fog_tracks_visible_explored_and_cells() {
        let mut f = FactionFog::new(64, 64);
        f.reveal_from((10, 10), 4, |_, _| false);
        assert!(f.is_visible(10, 14) && f.is_explored(10, 14));
        f.begin_frame();
        assert!(!f.is_visible(10, 14) && f.is_explored(10, 14));
        f.reveal_from((50, 50), 2, |_, _| false);
        let cells = f.visibility_cells();
        assert_eq!(cells.len(), 16 * 16);
        assert_eq!(cells[(50 / 4) * 16 + 50 / 4], 1);
        assert_eq!(cells[(10 / 4) * 16 + 10 / 4], 2);
        assert_eq!(cells[0], 0);
        assert!(f.any_visible(48, 48, 4, 4));
    }
}
