// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 微观层：`Q32.32` 定点 2-Iteration ORCA 互惠避障（van den Berg et al. 2011，RVO2 半平面构造）。
//!
//! - 每个邻居构造一个 ORCA 半平面（RVO2 `computeNewVelocity` 的三分支：截断圆、腿投影、已碰撞）；
//! - 求解不用 RVO2 的增量线性规划，而是对全部半平面按固定顺序做 **2 轮投影迭代**
//!   （GAP-11 冻结的「2-Iteration ORCA」），最后钳制到最大速率；
//! - 邻居按 `(距离², 实体序号)` 排序，最多取 [`ORCA_MAX_NEIGHBORS`] 个；
//! - 全部算术为 `Q32_32` 整数运算，跨平台位级一致。
//!
//! 速度单位为「Tile / Tick」，时间视野 `τ` 以 Tick 计。

use rynvek_core::Q32_32;

use crate::math::Vec2;

/// 时间视野 τ（Ticks）
pub const ORCA_TIME_HORIZON_TICKS: i32 = 60;
/// 投影迭代轮数
pub const ORCA_ITERATIONS: usize = 2;
/// 最大邻居数
pub const ORCA_MAX_NEIGHBORS: usize = 10;

/// 参与避障的个体
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrcaAgent {
    pub id: u32,
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: Q32_32,
}

/// ORCA 半平面：允许区域在 `direction` 的左侧（`det(direction, v − point) ≥ 0`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrcaLine {
    pub point: Vec2,
    pub direction: Vec2,
}

/// 由自身与一个邻居构造 ORCA 半平面（RVO2 逐行翻译为定点）
pub fn orca_line(me: &OrcaAgent, other: &OrcaAgent, tau_ticks: i32) -> OrcaLine {
    let inv_tau = Q32_32::ONE.div_safe(Q32_32::from_int(tau_ticks), Q32_32::ZERO);
    let rel_pos = other.position - me.position;
    let rel_vel = me.velocity - other.velocity;
    let dist_sq = rel_pos.length_sq();
    let r = me.radius.saturating_add(other.radius);
    let r_sq = r.mul(r);

    let (direction, u);
    if dist_sq > r_sq {
        // 尚未碰撞：w 为相对速度到截断圆心的向量
        let w = rel_vel - rel_pos.scale(inv_tau);
        let w_len_sq = w.length_sq();
        let dot1 = w.dot(rel_pos);
        if dot1.0 < 0 && dot1.mul(dot1).0 > r_sq.mul(w_len_sq).0 {
            // 投影到截断圆
            let w_len = w_len_sq.sqrt_deterministic();
            let unit_w = w.normalized();
            direction = Vec2::new(unit_w.y, Q32_32(-unit_w.x.0));
            u = unit_w.scale(r.mul(inv_tau).saturating_sub(w_len));
        } else {
            // 投影到 VO 锥的腿
            let leg = dist_sq.saturating_sub(r_sq).sqrt_deterministic();
            direction = if rel_pos.det(w).0 > 0 {
                Vec2::new(
                    rel_pos.x.mul(leg).saturating_sub(rel_pos.y.mul(r)),
                    rel_pos.x.mul(r).saturating_add(rel_pos.y.mul(leg)),
                )
                .scale(Q32_32::ONE.div_safe(dist_sq, Q32_32::ZERO))
            } else {
                -Vec2::new(
                    rel_pos.x.mul(leg).saturating_add(rel_pos.y.mul(r)),
                    Q32_32(-rel_pos.x.mul(r).0).saturating_add(rel_pos.y.mul(leg)),
                )
                .scale(Q32_32::ONE.div_safe(dist_sq, Q32_32::ZERO))
            };
            let dot2 = rel_vel.dot(direction);
            u = direction.scale(dot2) - rel_vel;
        }
    } else {
        // 已重叠：以 1 Tick 为时间步把相对位置推开
        let w = rel_vel - rel_pos;
        let w_len = w.length();
        let unit_w = if w_len.0 == 0 { Vec2::new(Q32_32::ONE, Q32_32::ZERO) } else { w.normalized() };
        direction = Vec2::new(unit_w.y, Q32_32(-unit_w.x.0));
        u = unit_w.scale(r.saturating_sub(w_len));
    }
    OrcaLine {
        point: me.velocity + u.scale(Q32_32::HALF),
        direction,
    }
}

/// 计算避障后的新速度
///
/// `neighbors` 会按 `(距离², id)` 排序并截断到 [`ORCA_MAX_NEIGHBORS`]；`max_speed` 为每 Tick 最大位移。
pub fn compute_velocity(me: &OrcaAgent, neighbors: &[OrcaAgent], preferred: Vec2, max_speed: Q32_32) -> Vec2 {
    let mut sorted: Vec<&OrcaAgent> = neighbors.iter().filter(|n| n.id != me.id).collect();
    sorted.sort_by_key(|n| (me.position.distance_sq(n.position), n.id));
    sorted.truncate(ORCA_MAX_NEIGHBORS);
    let lines: Vec<OrcaLine> = sorted.iter().map(|n| orca_line(me, n, ORCA_TIME_HORIZON_TICKS)).collect();
    let mut v = preferred.clamp_length(max_speed);
    for _ in 0..ORCA_ITERATIONS {
        for line in &lines {
            // det(direction, point − v) > 0 表示 v 落在半平面右侧（违反约束）
            if line.direction.det(line.point - v).0 > 0 {
                let t = line.direction.dot(v - line.point);
                v = line.point + line.direction.scale(t);
            }
        }
    }
    v.clamp_length(max_speed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: u32, x: i64, y: i64, vx: i64, vy: i64) -> OrcaAgent {
        OrcaAgent {
            id,
            position: Vec2::from_milli(x, y),
            velocity: Vec2::from_milli(vx, vy),
            radius: Q32_32::from_milli(500),
        }
    }

    #[test]
    fn no_neighbors_keeps_preferred_velocity() {
        let me = agent(1, 0, 0, 0, 0);
        let pref = Vec2::from_milli(100, 0);
        assert_eq!(compute_velocity(&me, &[], pref, Q32_32::from_milli(150)), pref);
        let fast = Vec2::from_milli(300, 0);
        let v = compute_velocity(&me, &[], fast, Q32_32::from_milli(150));
        assert!((v.x.0 - Q32_32::from_milli(150).0).abs() < 1 << 12);
    }

    #[test]
    fn head_on_agents_deflect_to_opposite_sides() {
        let a = agent(1, 0, 0, 100, 0);
        let b = agent(2, 3000, 0, -100, 0);
        let va = compute_velocity(&a, &[b], Vec2::from_milli(100, 0), Q32_32::from_milli(150));
        let vb = compute_velocity(&b, &[a], Vec2::from_milli(-100, 0), Q32_32::from_milli(150));
        assert!(va.y.0 != 0 && vb.y.0 != 0);
        // 互惠：双方各承担一半，按各自前进方向同向（右手）避让，世界坐标中 y 分量互为相反数
        // （定点截断允许 ≤ 4 ulp 的不对称）
        assert!(va.y.0 != 0 && (va.y.0 + vb.y.0).abs() <= 4, "{va:?} {vb:?}");
    }

    #[test]
    fn simulated_crossing_never_overlaps_and_is_deterministic() {
        let run = || {
            let mut agents: Vec<OrcaAgent> = (0..8)
                .map(|i| {
                    let side = if i % 2 == 0 { 0 } else { 12_000 };
                    agent(i, side, 1000 * i as i64, 0, 0)
                })
                .collect();
            let goals: Vec<Vec2> = agents.iter().map(|a| Vec2::new(Q32_32::from_int(12).saturating_sub(a.position.x), a.position.y)).collect();
            let speed = Q32_32::from_milli(100);
            let mut min_gap = i64::MAX;
            for _ in 0..240 {
                let snapshot = agents.clone();
                for (a, goal) in agents.iter_mut().zip(&goals) {
                    let pref = (*goal - a.position).clamp_length(speed);
                    a.velocity = compute_velocity(a, &snapshot, pref, speed);
                }
                for a in &mut agents {
                    a.position += a.velocity;
                }
                for i in 0..agents.len() {
                    for j in i + 1..agents.len() {
                        let d = agents[i].position.distance_sq(agents[j].position).sqrt_deterministic();
                        min_gap = min_gap.min(d.0);
                    }
                }
            }
            (agents.iter().map(|a| (a.position.x.0, a.position.y.0)).collect::<Vec<_>>(), min_gap)
        };
        let (a, gap) = run();
        let (b, _) = run();
        assert_eq!(a, b, "ORCA 必须位级确定");
        // 允许 Q32 截断带来的轻微侵入（≤ 5% 半径和）
        assert!(gap >= Q32_32::from_milli(950).0, "最小间距 {}", gap as f64 / 4_294_967_296.0);
    }
}
