// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `PvE_Survival` 自适应自然选择虫潮（项目文档 §5.5.2）。
//!
//! - 地图外围四个边缘裂隙每 `1,800 Ticks` 刷新第 `w` 波（`w = 1..=30`）；
//! - 波次生物质总预算 `B_w = ⌊150 · w^1.35⌋`：以大整数精确判定 `B^20 ≤ 150^20 · w^27`，零浮点；
//! - 每波结束按 `F = 2·Damage + 0.5·Survival` 取前 10% 精英双亲交叉 + 突变（`σ = 0.08`）作为下一波基因池。
//!
//! 未规定细节（ERRATA E-026）：基因池规模 20；初始配比 Forager 30% / Striker 40% / Carapace 10% /
//! Spitter 15% / Leviathan 5%；预算按池内顺序循环花费各原型的 Biomass 造价，直到任何原型都买不起；
//! 后代继承适应度较高一方的原型，并有 5% 概率「原型跃迁」为随机其他原型的种子基因组变体。

use crate::evolution::{DEFAULT_MUTATION_RATE_PERMILLE, EvolutionRng, FitnessRecord, breed, mutate, select_elites};
use crate::genome::Genome;
use crate::ids::UnitKind;
use crate::units::unit_spec;

/// 波次间隔
pub const PVE_WAVE_INTERVAL_TICKS: u64 = 1_800;
/// 总波数
pub const PVE_MAX_WAVES: u32 = 30;
/// 基因池规模
pub const PVE_POOL_SIZE: usize = 20;
/// 原型跃迁概率（‰）
pub const PVE_ARCHETYPE_JUMP_PERMILLE: u32 = 50;
/// 可出现在虫潮中的原型（Weaver 为玩家支援单位，不进入野生虫潮）
pub const PVE_ARCHETYPES: [UnitKind; 5] = [
    UnitKind::Forager,
    UnitKind::Striker,
    UnitKind::Carapace,
    UnitKind::Spitter,
    UnitKind::Leviathan,
];
/// 初始配比（每 20 个池位）
pub const PVE_INITIAL_MIX: [(UnitKind, usize); 5] = [
    (UnitKind::Forager, 6),
    (UnitKind::Striker, 8),
    (UnitKind::Carapace, 2),
    (UnitKind::Spitter, 3),
    (UnitKind::Leviathan, 1),
];

/// 极简无符号大整数（小端 32 位分块），只为精确比较 `B^20` 与 `150^20 · w^27`
#[derive(Clone, Debug, PartialEq, Eq)]
struct BigUint(Vec<u32>);

impl BigUint {
    fn from_u64(v: u64) -> Self {
        let mut b = Self(vec![v as u32, (v >> 32) as u32]);
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.0.len() > 1 && *self.0.last().unwrap_or(&1) == 0 {
            self.0.pop();
        }
    }

    fn mul(&self, o: &Self) -> Self {
        let mut out = vec![0u64; self.0.len() + o.0.len() + 1];
        for (i, a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, b) in o.0.iter().enumerate() {
                let cur = out[i + j] + (*a as u64) * (*b as u64) + carry;
                out[i + j] = cur & 0xFFFF_FFFF;
                carry = cur >> 32;
            }
            let mut k = i + o.0.len();
            while carry != 0 {
                let cur = out[k] + carry;
                out[k] = cur & 0xFFFF_FFFF;
                carry = cur >> 32;
                k += 1;
            }
        }
        let mut b = Self(out.into_iter().map(|v| v as u32).collect());
        b.trim();
        b
    }

    fn pow(base: u64, exp: u32) -> Self {
        let mut acc = Self::from_u64(1);
        let b = Self::from_u64(base);
        for _ in 0..exp {
            acc = acc.mul(&b);
        }
        acc
    }

    fn le(&self, o: &Self) -> bool {
        if self.0.len() != o.0.len() {
            return self.0.len() < o.0.len();
        }
        for (a, b) in self.0.iter().rev().zip(o.0.iter().rev()) {
            if a != b {
                return a < b;
            }
        }
        true
    }
}

/// 第 `w` 波生物质预算（整数 Biomass）：`⌊150 · w^1.35⌋`
pub fn wave_budget(w: u32) -> u64 {
    if w == 0 {
        return 0;
    }
    // B ≤ 150·w^1.35 ⇔ B^20 ≤ 150^20 · w^27
    let rhs = BigUint::pow(150, 20).mul(&BigUint::pow(w as u64, 27));
    let (mut lo, mut hi) = (0u64, 150 * (w as u64) * (w as u64) + 1);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if BigUint::pow(mid, 20).le(&rhs) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// 第 `w` 波刷新的 Tick（第 1 波在 1800）
pub const fn wave_spawn_tick(w: u32) -> u64 {
    w as u64 * PVE_WAVE_INTERVAL_TICKS
}

/// 虫潮导演：维护跨波次演化的基因池
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PveDirector {
    pub world_seed: u64,
    pub pool: Vec<Genome>,
    /// 已刷新的最后一波（0 = 尚未开始）
    pub wave: u32,
}

impl PveDirector {
    /// 初始基因池：按配比取种子基因组，并以 Philox 轻度突变使池内个体各不相同
    pub fn new(world_seed: u64) -> Self {
        let mut pool = Vec::with_capacity(PVE_POOL_SIZE);
        for (kind, count) in PVE_INITIAL_MIX {
            for _ in 0..count {
                let idx = pool.len();
                let mut g = Genome::seed(kind);
                mutate(&mut g, DEFAULT_MUTATION_RATE_PERMILLE, &mut EvolutionRng::new(world_seed, 0, idx as u64));
                g.slug = format!("wild.w0.{idx}");
                pool.push(g);
            }
        }
        Self {
            world_seed,
            pool,
            wave: 0,
        }
    }

    /// 规划第 `w` 波：返回要孵化的基因池下标序列（按预算循环花费）
    pub fn plan_wave(&self, w: u32) -> Vec<usize> {
        let mut budget = wave_budget(w) as i64 * 1000;
        let mut out = Vec::new();
        let cheapest = self.pool.iter().map(|g| unit_spec(g.unit).cost.biomass).min().unwrap_or(i64::MAX);
        let mut i = 0usize;
        while budget >= cheapest && !self.pool.is_empty() {
            let g = &self.pool[i % self.pool.len()];
            let cost = unit_spec(g.unit).cost.biomass;
            if cost <= budget {
                budget -= cost;
                out.push(i % self.pool.len());
            }
            i += 1;
            // 一整圈都买不起任何个体时 cheapest 判定已保证退出；此处防御性上限
            if i > 100_000 {
                break;
            }
        }
        out
    }

    /// 一波结束：以本波个体的适应度繁育下一波基因池
    ///
    /// `records` 为 `(实体序号, 基因组, 适应度)`；为空时基因池保持不变。
    pub fn evolve(&mut self, records: &[(u32, Genome, FitnessRecord)], tick: u64) {
        if records.is_empty() {
            return;
        }
        let keyed: Vec<(u32, FitnessRecord)> = records.iter().map(|(id, _, f)| (*id, *f)).collect();
        let elites = select_elites(&keyed);
        let k = elites.len();
        let next_wave = self.wave + 1;
        let mut pool = Vec::with_capacity(PVE_POOL_SIZE);
        for i in 0..PVE_POOL_SIZE {
            let mut rng = EvolutionRng::new(self.world_seed, tick, (next_wave as u64) << 32 | i as u64);
            let a = elites[i % k];
            let b = elites[rng.below(k as u32) as usize];
            let (fa, fb) = (records[a].2.fitness_milli(), records[b].2.fitness_milli());
            let (fitter, other) = if fa >= fb { (a, b) } else { (b, a) };
            let slug = format!("wild.w{next_wave}.{i}");
            let child = if rng.chance_permille(PVE_ARCHETYPE_JUMP_PERMILLE) {
                let others: Vec<UnitKind> = PVE_ARCHETYPES.into_iter().filter(|u| *u != records[fitter].1.unit).collect();
                let jump = others[rng.below(others.len() as u32) as usize];
                let mut g = Genome::seed(jump);
                mutate(&mut g, DEFAULT_MUTATION_RATE_PERMILLE, &mut rng);
                g.generation = records[fitter].1.generation + 1;
                g.slug = slug;
                g
            } else {
                breed(&records[fitter].1, &records[other].1, DEFAULT_MUTATION_RATE_PERMILLE, &mut rng, slug)
            };
            pool.push(child);
        }
        self.pool = pool;
    }

    /// 基因池原型构成（按 [`PVE_ARCHETYPES`] 顺序计数）
    pub fn archetype_mix(&self) -> [usize; 5] {
        let mut out = [0; 5];
        for g in &self.pool {
            if let Some(i) = PVE_ARCHETYPES.iter().position(|u| *u == g.unit) {
                out[i] += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_budget_matches_exact_floor_formula() {
        let expected = [
            150u64, 382, 661, 974, 1317, 1684, 2074, 2484, 2912, 3358, 3819, 4295, 4785, 5288, 5805, 6333, 6873, 7425,
            7987, 8560, 9142, 9735, 10337, 10949, 11569, 12198, 12836, 13482, 14136, 14798,
        ];
        let got: Vec<u64> = (1..=30).map(wave_budget).collect();
        assert_eq!(got, expected);
        assert_eq!(wave_budget(0), 0);
        assert_eq!(wave_spawn_tick(1), 1800);
    }

    #[test]
    fn initial_pool_follows_mix() {
        let d = PveDirector::new(42);
        assert_eq!(d.pool.len(), PVE_POOL_SIZE);
        assert_eq!(d.archetype_mix(), [6, 8, 2, 3, 1]);
        assert_eq!(d, PveDirector::new(42));
        assert_ne!(d.pool[0].digest(), d.pool[1].digest());
    }

    #[test]
    fn wave_plan_spends_budget_without_overdraft() {
        let d = PveDirector::new(1);
        for w in [1u32, 5, 30] {
            let plan = d.plan_wave(w);
            let spent: i64 = plan.iter().map(|&i| unit_spec(d.pool[i].unit).cost.biomass).sum();
            let budget = wave_budget(w) as i64 * 1000;
            assert!(spent <= budget);
            let cheapest = d.pool.iter().map(|g| unit_spec(g.unit).cost.biomass).min().unwrap();
            assert!(budget - spent < cheapest, "w{w} 剩余预算仍可再买");
            assert!(!plan.is_empty());
        }
    }

    #[test]
    fn selection_pressure_shifts_pool_toward_successful_archetype() {
        // 合成适应度：Spitter 造成大量伤害（模拟玩家只造近战盾簇时远程风筝占优）
        let mut d = PveDirector::new(7);
        let start = d.archetype_mix()[3];
        for wave in 1..=4u32 {
            d.wave = wave - 1;
            let records: Vec<(u32, Genome, FitnessRecord)> = d
                .pool
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    let dmg = if g.unit == UnitKind::Spitter { 500_000 } else { 10_000 };
                    (i as u32, g.clone(), FitnessRecord {
                        damage_dealt_milli: dmg,
                        survival_ticks: 100,
                    })
                })
                .collect();
            d.evolve(&records, wave_spawn_tick(wave) + 1);
            d.wave = wave;
        }
        let end = d.archetype_mix()[3];
        assert!(end >= 15 && end > start, "Spitter 占比 {start} → {end}");
        assert!(d.pool.iter().all(|g| g.generation >= 1));
    }
}
