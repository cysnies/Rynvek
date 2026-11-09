// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Glicko-2 天梯评分（D328–D333，Glickman《Example of the Glicko-2 system》2013 版算法）。
//!
//! - 每条赛道（`human` / `script` / `agent_hybrid`）独立维护评分，初值与 DDL 缺省一致：`1500 / 350 / 0.06`；
//! - 每局结束即视为一个评分周期（实时天梯常见做法），输入为该周期内的全部对手与得分；
//! - 系统常数 `τ = 0.5`（规范未定，取论文推荐区间 `[0.3, 1.2]` 的中低值以抑制波动率暴涨，见 ERRATA）；
//! - 波动率迭代使用论文第 5.1 节的 Illinois 割线法，收敛阈值 `ε = 1e-6`。
//!
//! 评分只在服务端以 `f64` 计算，不进入确定性仿真，因此无需定点数。

/// 缺省评分（DDL `glicko2_ratings` 缺省值）
pub const DEFAULT_RATING: f64 = 1500.0;
pub const DEFAULT_DEVIATION: f64 = 350.0;
pub const DEFAULT_VOLATILITY: f64 = 0.06;
/// 系统常数 τ
pub const DEFAULT_TAU: f64 = 0.5;
/// Glicko 与 Glicko-2 刻度换算常数 `173.7178 = 400 / ln(10)`
pub const SCALE: f64 = 173.7178;
/// 波动率迭代收敛阈值
pub const CONVERGENCE_EPSILON: f64 = 1e-6;
/// RD 上限：长期不活跃也不超过初值
pub const MAX_DEVIATION: f64 = DEFAULT_DEVIATION;

/// 一名选手在某赛道上的评分
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rating {
    pub rating: f64,
    pub deviation: f64,
    pub volatility: f64,
}

impl Default for Rating {
    fn default() -> Self {
        Self {
            rating: DEFAULT_RATING,
            deviation: DEFAULT_DEVIATION,
            volatility: DEFAULT_VOLATILITY,
        }
    }
}

/// 一场对局的结果（对手评分 + 本方得分：胜 1、平 0.5、负 0）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameResult {
    pub opponent: Rating,
    pub score: f64,
}

fn g(phi: f64) -> f64 {
    1.0 / (1.0 + 3.0 * phi * phi / (std::f64::consts::PI * std::f64::consts::PI)).sqrt()
}

fn expected(mu: f64, mu_j: f64, phi_j: f64) -> f64 {
    1.0 / (1.0 + (-g(phi_j) * (mu - mu_j)).exp())
}

/// 论文第 5 步：求新波动率 σ'
fn new_volatility(sigma: f64, phi: f64, v: f64, delta: f64, tau: f64) -> f64 {
    let a = (sigma * sigma).ln();
    let f = |x: f64| {
        let ex = x.exp();
        let num = ex * (delta * delta - phi * phi - v - ex);
        let den = 2.0 * (phi * phi + v + ex).powi(2);
        num / den - (x - a) / (tau * tau)
    };
    let mut big_a = a;
    let mut big_b = if delta * delta > phi * phi + v {
        (delta * delta - phi * phi - v).ln()
    } else {
        let mut k = 1.0;
        while f(a - k * tau) < 0.0 {
            k += 1.0;
        }
        a - k * tau
    };
    let mut f_a = f(big_a);
    let mut f_b = f(big_b);
    // 上限 100 次迭代防御病态输入（正常 < 10 次收敛）
    for _ in 0..100 {
        if (big_b - big_a).abs() <= CONVERGENCE_EPSILON {
            break;
        }
        let c = big_a + (big_a - big_b) * f_a / (f_b - f_a);
        let f_c = f(c);
        if f_c * f_b <= 0.0 {
            big_a = big_b;
            f_a = f_b;
        } else {
            f_a /= 2.0;
        }
        big_b = c;
        f_b = f_c;
    }
    (big_a / 2.0).exp()
}

/// 用一个评分周期内的全部结果更新评分；`results` 为空时只增加 RD（论文第 6 步的特例）
pub fn update(player: Rating, results: &[GameResult], tau: f64) -> Rating {
    let mu = (player.rating - DEFAULT_RATING) / SCALE;
    let phi = player.deviation / SCALE;
    let sigma = player.volatility;
    if results.is_empty() {
        let phi_star = (phi * phi + sigma * sigma).sqrt();
        return Rating {
            deviation: (phi_star * SCALE).min(MAX_DEVIATION),
            ..player
        };
    }
    // 第 3、4 步：估计方差 v 与改进量 Δ
    let mut v_inv = 0.0;
    let mut delta_sum = 0.0;
    for r in results {
        let mu_j = (r.opponent.rating - DEFAULT_RATING) / SCALE;
        let phi_j = r.opponent.deviation / SCALE;
        let e = expected(mu, mu_j, phi_j);
        let gj = g(phi_j);
        v_inv += gj * gj * e * (1.0 - e);
        delta_sum += gj * (r.score - e);
    }
    let v = 1.0 / v_inv;
    let delta = v * delta_sum;
    // 第 5–7 步
    let sigma_new = new_volatility(sigma, phi, v, delta, tau);
    let phi_star = (phi * phi + sigma_new * sigma_new).sqrt();
    let phi_new = 1.0 / (1.0 / (phi_star * phi_star) + 1.0 / v).sqrt();
    let mu_new = mu + phi_new * phi_new * delta_sum;
    Rating {
        rating: SCALE * mu_new + DEFAULT_RATING,
        deviation: (SCALE * phi_new).min(MAX_DEVIATION),
        volatility: sigma_new,
    }
}

/// 两人对局：`score_a` 为 A 的得分（1 / 0.5 / 0），返回双方新评分（同一周期内同时更新）
pub fn update_pair(a: Rating, b: Rating, score_a: f64, tau: f64) -> (Rating, Rating) {
    let new_a = update(
        a,
        &[GameResult {
            opponent: b,
            score: score_a,
        }],
        tau,
    );
    let new_b = update(
        b,
        &[GameResult {
            opponent: a,
            score: 1.0 - score_a,
        }],
        tau,
    );
    (new_a, new_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(rating: f64, deviation: f64) -> Rating {
        Rating {
            rating,
            deviation,
            volatility: DEFAULT_VOLATILITY,
        }
    }

    /// Glickman 论文示例：1500/200 对 1400/30（胜）、1550/100（负）、1700/300（负），τ = 0.5
    #[test]
    fn matches_glickman_paper_example() {
        let out = update(
            r(1500.0, 200.0),
            &[
                GameResult {
                    opponent: r(1400.0, 30.0),
                    score: 1.0,
                },
                GameResult {
                    opponent: r(1550.0, 100.0),
                    score: 0.0,
                },
                GameResult {
                    opponent: r(1700.0, 300.0),
                    score: 0.0,
                },
            ],
            0.5,
        );
        assert!((out.rating - 1464.06).abs() < 0.01, "r' = {}", out.rating);
        assert!((out.deviation - 151.52).abs() < 0.01, "RD' = {}", out.deviation);
        assert!((out.volatility - 0.059_99).abs() < 1e-5, "σ' = {}", out.volatility);
    }

    #[test]
    fn winner_gains_loser_loses_symmetrically_for_equal_players() {
        let (a, b) = update_pair(Rating::default(), Rating::default(), 1.0, DEFAULT_TAU);
        assert!(a.rating > 1500.0 && b.rating < 1500.0);
        assert!(((a.rating - 1500.0) + (b.rating - 1500.0)).abs() < 1e-9);
        assert!(a.deviation < 350.0 && b.deviation < 350.0);
        // 新手 1500/350 赢下一局约 +162
        assert!((a.rating - 1662.31).abs() < 0.05, "{}", a.rating);
    }

    #[test]
    fn draw_between_equals_changes_nothing_but_deviation() {
        let (a, b) = update_pair(r(1600.0, 80.0), r(1600.0, 80.0), 0.5, DEFAULT_TAU);
        assert!((a.rating - 1600.0).abs() < 1e-9 && (b.rating - 1600.0).abs() < 1e-9);
        assert!(a.deviation < 80.0);
    }

    #[test]
    fn upset_moves_more_than_expected_win() {
        let strong = r(1900.0, 60.0);
        let weak = r(1500.0, 60.0);
        let (s_win, _) = update_pair(strong, weak, 1.0, DEFAULT_TAU);
        let (s_loss, w_win) = update_pair(strong, weak, 0.0, DEFAULT_TAU);
        assert!(s_win.rating - 1900.0 < 1900.0 - s_loss.rating);
        assert!(w_win.rating > 1500.0);
    }

    #[test]
    fn inactivity_only_grows_deviation_and_is_capped() {
        let p = r(1700.0, 50.0);
        let idle = update(p, &[], DEFAULT_TAU);
        assert_eq!(idle.rating, 1700.0);
        assert!(idle.deviation > 50.0);
        let mut q = r(1700.0, 349.0);
        for _ in 0..10 {
            q = update(q, &[], DEFAULT_TAU);
        }
        assert_eq!(q.deviation, MAX_DEVIATION);
    }
}
