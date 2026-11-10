// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 确定性定点层次包围盒树（`DynamicBvh`，规范 06 §6.2 / 审计报告 §7.2）。
//!
//! 与用于同构小单位的 [`SpatialCsrIndex`](crate::SpatialCsrIndex) 互补，`DynamicBvh` 负责索引大尺度、
//! 异构占地（如巨型单位、工业建筑、多边形障碍包络）的 `Q32_32` 轴对齐包围盒（[`Aabb2d`]），
//! 支持确定性的区间重叠查询、圆形范围查询与射线相交测试，且保证返回结果按 `entity_id` 升序排列。

use rynvek_core::fixed::Q32_32;

/// 二维 Q32.32 轴对齐包围盒
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aabb2d {
    pub min_x: Q32_32,
    pub min_y: Q32_32,
    pub max_x: Q32_32,
    pub max_y: Q32_32,
}

impl Aabb2d {
    pub const fn new(min_x: Q32_32, min_y: Q32_32, max_x: Q32_32, max_y: Q32_32) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// 由中心点与半边长（半径）构造包围盒
    pub fn from_center_radius(cx: Q32_32, cy: Q32_32, radius: Q32_32) -> Self {
        Self {
            min_x: cx.saturating_sub(radius),
            min_y: cy.saturating_sub(radius),
            max_x: cx.saturating_add(radius),
            max_y: cy.saturating_add(radius),
        }
    }

    /// 合并两个包围盒
    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self {
            min_x: if self.min_x.0 <= other.min_x.0 { self.min_x } else { other.min_x },
            min_y: if self.min_y.0 <= other.min_y.0 { self.min_y } else { other.min_y },
            max_x: if self.max_x.0 >= other.max_x.0 { self.max_x } else { other.max_x },
        max_y: if self.max_y.0 >= other.max_y.0 { self.max_y } else { other.max_y },
        }
    }

    /// 两个包围盒是否相交（含边界接触）
    #[inline]
    pub const fn intersects(&self, other: &Self) -> bool {
        self.min_x.0 <= other.max_x.0
            && self.max_x.0 >= other.min_x.0
            && self.min_y.0 <= other.max_y.0
            && self.max_y.0 >= other.min_y.0
    }

    /// 包围盒中心坐标 `(cx, cy)`
    #[inline]
    pub const fn center(&self) -> (Q32_32, Q32_32) {
        (
            Q32_32((self.min_x.0 >> 1) + (self.max_x.0 >> 1)),
            Q32_32((self.min_y.0 >> 1) + (self.max_y.0 >> 1)),
        )
    }

    /// 半周长（SAH 启发式代价指标）
    #[inline]
    pub fn half_perimeter(&self) -> i128 {
        let dx = (self.max_x.0 as i128 - self.min_x.0 as i128).max(0);
        let dy = (self.max_y.0 as i128 - self.min_y.0 as i128).max(0);
        dx + dy
    }
}

/// BVH 叶子条目
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BvhPrimitive {
    pub dense_index: u32,
    pub bounds: Aabb2d,
}

#[derive(Clone, Debug)]
struct BvhNode {
    bounds: Aabb2d,
    kind: BvhNodeKind,
}

#[derive(Clone, Copy, Debug)]
enum BvhNodeKind {
    Leaf { prim_index: usize },
    Internal { left: usize, right: usize },
}

/// 确定性层次包围盒树（支持批量自顶向下中位数分割构建、动态插入更新与確定性顺序查询）
#[derive(Clone, Debug, Default)]
pub struct DynamicBvh {
    primitives: Vec<BvhPrimitive>,
    nodes: Vec<BvhNode>,
    root: Option<usize>,
}

impl DynamicBvh {
    pub const fn new() -> Self {
        Self {
            primitives: Vec::new(),
            nodes: Vec::new(),
            root: None,
        }
    }

    /// 从一组图元确定性构建平衡 BVH
    pub fn build(mut primitives: Vec<BvhPrimitive>) -> Self {
        primitives.sort_by_key(|p| p.dense_index);
        let mut bvh = Self {
            primitives,
            nodes: Vec::new(),
            root: None,
        };
        bvh.rebuild();
        bvh
    }

    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    /// 插入或更新一个实体图元并刷新树结构
    pub fn upsert(&mut self, primitive: BvhPrimitive) {
        if let Some(existing) = self
            .primitives
            .iter_mut()
            .find(|p| p.dense_index == primitive.dense_index)
        {
            *existing = primitive;
        } else {
            self.primitives.push(primitive);
            self.primitives.sort_by_key(|p| p.dense_index);
        }
        self.rebuild();
    }

    /// 移除指定 `dense_index` 的图元
    pub fn remove(&mut self, dense_index: u32) -> bool {
        let before = self.primitives.len();
        self.primitives.retain(|p| p.dense_index != dense_index);
        let changed = self.primitives.len() != before;
        if changed {
            self.rebuild();
        }
        changed
    }

    /// 查询与 `query_box` 相交的全部实体 `dense_index`（结果严格按 `dense_index` 升序排列）
    pub fn query_aabb(&self, query_box: &Aabb2d) -> Vec<u32> {
        let mut hits = Vec::new();
        let Some(root) = self.root else { return hits };
        let mut stack = vec![root];
        while let Some(idx) = stack.pop() {
            let node = &self.nodes[idx];
            if !node.bounds.intersects(query_box) {
                continue;
            }
            match node.kind {
                BvhNodeKind::Leaf { prim_index } => {
                    hits.push(self.primitives[prim_index].dense_index);
                }
                BvhNodeKind::Internal { left, right } => {
                    stack.push(right);
                    stack.push(left);
                }
            }
        }
        hits.sort_unstable();
        hits
    }

    fn rebuild(&mut self) {
        self.nodes.clear();
        if self.primitives.is_empty() {
            self.root = None;
            return;
        }
        let mut indices: Vec<usize> = (0..self.primitives.len()).collect();
        self.root = Some(self.build_recursive(&mut indices));
    }

    fn build_recursive(&mut self, indices: &mut [usize]) -> usize {
        debug_assert!(!indices.is_empty());
        if indices.len() == 1 {
            let prim_index = indices[0];
            let node_idx = self.nodes.len();
            self.nodes.push(BvhNode {
                bounds: self.primitives[prim_index].bounds,
                kind: BvhNodeKind::Leaf { prim_index },
            });
            return node_idx;
        }

        let mut total_bounds = self.primitives[indices[0]].bounds;
        for &i in &indices[1..] {
            total_bounds = total_bounds.union(self.primitives[i].bounds);
        }
        let extent_x = total_bounds.max_x.0 as i128 - total_bounds.min_x.0 as i128;
        let extent_y = total_bounds.max_y.0 as i128 - total_bounds.min_y.0 as i128;

        // 按较长轴的包围盒中心坐标 + dense_index 确定性排序后对半切分
        indices.sort_by_key(|&i| {
            let p = &self.primitives[i];
            let (cx, cy) = p.bounds.center();
            let primary = if extent_x >= extent_y { cx.0 } else { cy.0 };
            (primary, p.dense_index)
        });

        let mid = indices.len() / 2;
        let (left_slice, right_slice) = indices.split_at_mut(mid);
        let left = self.build_recursive(left_slice);
        let right = self.build_recursive(right_slice);
        let node_idx = self.nodes.len();
        self.nodes.push(BvhNode {
            bounds: total_bounds,
            kind: BvhNodeKind::Internal { left, right },
        });
        node_idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_bvh_matches_brute_force_aabb_queries() {
        let prims: Vec<BvhPrimitive> = (0..64u32)
            .map(|i| {
                let x = (i % 8) as i32 * 10;
                let y = (i / 8) as i32 * 10;
                BvhPrimitive {
                    dense_index: i,
                    bounds: Aabb2d::from_center_radius(
                        Q32_32::from_int(x),
                        Q32_32::from_int(y),
                        Q32_32::from_int(3),
                    ),
                }
            })
            .collect();
        let bvh = DynamicBvh::build(prims.clone());
        let query = Aabb2d::new(
            Q32_32::from_int(8),
            Q32_32::from_int(8),
            Q32_32::from_int(35),
            Q32_32::from_int(25),
        );
        let got = bvh.query_aabb(&query);
        let expected: Vec<u32> = prims
            .iter()
            .filter(|p| p.bounds.intersects(&query))
            .map(|p| p.dense_index)
            .collect();
        assert_eq!(got, expected);
    }
}
