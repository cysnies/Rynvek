// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 基因组演化算子：精英选择、双亲均匀交叉、权重 / 拓扑突变与鲍德温同化（项目文档 §5.4 / §5.5.2）。
//!
//! 全部随机性来自 Philox（流 ID [`EVOLUTION_RNG_STREAM`]），密钥为 `(world_seed, tick, 实体序号)`，
//! 因此回放时同一时刻的同一次繁育得到逐位一致的后代。
//!
//! 规范给出的只有 `σ_mut = 0.08`、`mutation_rate ∈ [0.01, 0.25]` 与 `α_baldwin`；其余取舍（ERRATA E-029）：
//! - `mutation_rate` 解释为**每个非零突触**发生高斯扰动的概率（缺省 `0.05`）；
//! - 高斯噪声用 Irwin–Hall（12 个 16 位均匀数之和 − 6）近似 `N(0, 1)`，纯整数；
//! - `σ = 0.08` 以 Q8.8 表示为 `20`；突变后权重钳制到 `±8.0`（`±2048`）；
//! - 拓扑突变：每次繁育以 `mutation_rate` 概率新增一条突触（幅度 8..32），以同等概率剪除幅值最小的一条；
//! - 交叉只在同一原型（同模型、同隐藏层规模）之间进行；不同原型时后代继承适应度较高一方的原型。

use rynvek_core::RandomKey;
use rynvek_neural::TopologyStorageClass;
use rynvek_neural::storage::SYNAPSE_SLOTS_PER_NEURON;
use serde::{Deserialize, Serialize};

use crate::genome::{FILL_WEIGHT_RANGE_Q8, GENOME_INPUTS, GENOME_OUTPUTS, Genome};

/// 演化算子 Philox 流 ID
pub const EVOLUTION_RNG_STREAM: u16 = 0x0602;
/// `σ_mut = 0.08`（Q8.8）
pub const MUTATION_SIGMA_Q8: i32 = 20;
/// 缺省突变率（‰）
pub const DEFAULT_MUTATION_RATE_PERMILLE: u32 = 50;
/// 突变率合法区间（‰）：`[0.01, 0.25]`
pub const MUTATION_RATE_RANGE_PERMILLE: (u32, u32) = (10, 250);
/// 权重钳制幅度（Q8.8）
pub const WEIGHT_LIMIT_Q8: i16 = 2048;
/// 精英比例 10%，至少 2 个
pub const ELITE_PERCENT: usize = 10;
pub const ELITE_MIN: usize = 2;

/// 演化随机源
#[derive(Clone, Debug)]
pub struct EvolutionRng {
    key: RandomKey,
    buf: [u32; 4],
    used: usize,
}

impl EvolutionRng {
    pub fn new(world_seed: u64, tick: u64, entity_serial: u64) -> Self {
        Self {
            key: RandomKey {
                world_seed,
                tick,
                entity_serial,
                stream_and_phase: RandomKey::pack_stream_and_phase(EVOLUTION_RNG_STREAM, 0),
                sub_counter: 0,
            },
            buf: [0; 4],
            used: 4,
        }
    }

    /// 自然复制沿用同一Philox算子，但与定向繁育使用不同phase，避免同Tick键碰撞。
    pub fn for_mitosis(world_seed: u64, tick: u64, entity_serial: u64) -> Self {
        let mut rng = Self::new(world_seed, tick, entity_serial);
        rng.key.stream_and_phase = RandomKey::pack_stream_and_phase(EVOLUTION_RNG_STREAM, 1);
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.used == 4 {
            self.buf = self.key.generate_u32x4();
            self.key.sub_counter = self.key.sub_counter.wrapping_add(1);
            self.used = 0;
        }
        let v = self.buf[self.used];
        self.used += 1;
        v
    }

    /// `[0, n)` 均匀整数
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// 以 `permille / 1000` 概率返回 `true`
    pub fn chance_permille(&mut self, permille: u32) -> bool {
        self.below(1000) < permille
    }

    /// Irwin–Hall 近似标准正态，返回 `z × 65536`
    pub fn gaussian_q16(&mut self) -> i64 {
        let mut s = 0i64;
        for _ in 0..6 {
            let v = self.next_u32();
            s += (v & 0xFFFF) as i64 + (v >> 16) as i64;
        }
        s - 6 * 65536
    }
}

/// 个体适应度记录
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitnessRecord {
    /// 对玩家（或敌对阵营）造成的伤害（milli）
    pub damage_dealt_milli: i64,
    /// 存活 Tick 数
    pub survival_ticks: u64,
}

impl FitnessRecord {
    /// `F = 2.0 · Damage + 0.5 · SurvivalTicks`（milli 精度）
    pub const fn fitness_milli(&self) -> i64 {
        2 * self.damage_dealt_milli + 500 * self.survival_ticks as i64
    }
}

/// 精英选择：按 `(F 降序, 序号升序)` 取前 10%（至少 2 个，不超过总数），返回下标
pub fn select_elites(records: &[(u32, FitnessRecord)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(records[i].1.fitness_milli()), records[i].0));
    let k = (records.len() * ELITE_PERCENT)
        .div_ceil(100)
        .max(ELITE_MIN)
        .min(records.len());
    order.truncate(k);
    order
}

/// 双亲交叉：同原型时逐槽位均匀交叉，否则复制 `fitter`
pub fn crossover(fitter: &Genome, other: &Genome, rng: &mut EvolutionRng) -> Genome {
    let mut child = fitter.clone();
    child.generation = fitter.generation.max(other.generation) + 1;
    if fitter.unit != other.unit || fitter.model != other.model || fitter.n_hid != other.n_hid {
        return child;
    }
    for slot in 0..child.slot_count() {
        if rng.next_u32() & 1 == 1 {
            child.set_slot_weight(slot, other.slot_weight(slot));
        }
    }
    for (t, o) in child.tau_ticks.iter_mut().zip(&other.tau_ticks) {
        if rng.next_u32() & 1 == 1 {
            *t = *o;
        }
    }
    enforce_page_limit(&mut child);
    child
}

/// 权重与拓扑概率算子；自然复制允许0‰消融，定向繁育区间由其入口约束。
pub fn mutate(g: &mut Genome, rate_permille: u32, rng: &mut EvolutionRng) {
    let rate = rate_permille.min(1000);
    if rate == 0 {
        return;
    }
    for slot in 0..g.slot_count() {
        let w = g.slot_weight(slot);
        if w == 0 || !rng.chance_permille(rate) {
            continue;
        }
        let noise = (rng.gaussian_q16() * MUTATION_SIGMA_Q8 as i64) >> 16;
        let mut nw = (w as i64 + noise).clamp(-(WEIGHT_LIMIT_Q8 as i64), WEIGHT_LIMIT_Q8 as i64) as i16;
        if nw == 0 {
            nw = w.signum();
        }
        g.set_slot_weight(slot, nw);
    }
    // 拓扑：新增
    if rng.chance_permille(rate) {
        for _ in 0..32 {
            let slot = rng.below(g.slot_count() as u32) as usize;
            if g.slot_weight(slot) == 0 && g.slot_has_room(slot) {
                let (lo, hi) = FILL_WEIGHT_RANGE_Q8;
                let mag = lo + rng.below((hi - lo + 1) as u32) as i16;
                let sign = if rng.next_u32() & 1 == 0 { 1 } else { -1 };
                g.set_slot_weight(slot, sign * mag);
                break;
            }
        }
    }
    // 拓扑：剪除幅值最小者（并列取最小槽位）
    if rng.chance_permille(rate) {
        let victim = (0..g.slot_count())
            .filter(|&s| g.slot_weight(s) != 0)
            .min_by_key(|&s| (g.slot_weight(s).unsigned_abs(), s));
        if let Some(s) = victim {
            g.set_slot_weight(s, 0);
        }
    }
}

/// `PagedDynamicAdjacency` 每行最多 32 个非零循环突触：超出时剪除幅值最小者
pub fn enforce_page_limit(g: &mut Genome) {
    if g.storage != TopologyStorageClass::PagedDynamicAdjacency || g.w_rec.is_empty() {
        return;
    }
    let n = g.n_hid as usize;
    for post in 0..n {
        let row = &mut g.w_rec[post * n..(post + 1) * n];
        loop {
            let nz = row.iter().filter(|w| **w != 0).count();
            if nz <= SYNAPSE_SLOTS_PER_NEURON {
                break;
            }
            let victim = (0..n)
                .filter(|&p| row[p] != 0)
                .min_by_key(|&p| (row[p].unsigned_abs(), p))
                .expect("非空");
            row[victim] = 0;
        }
    }
}

/// 鲍德温同化：把 LIF 后天可塑性偏移 `δw` 按 `α` 写回先天权重
///
/// `delta` 布局与 `LifBrain` 冻结核一致：`[h*16 + i]` 后接 `[n*16 + h*6 + k]`。
pub fn baldwin_assimilate(g: &mut Genome, delta: &[i16], alpha_permille: i64) {
    let n = g.n_hid as usize;
    if delta.len() != n * (GENOME_INPUTS + GENOME_OUTPUTS) {
        return;
    }
    let apply = |w: &mut i16, d: i16| {
        if *w == 0 {
            return; // 同化只调节既有突触，不改变拓扑
        }
        let nw =
            (*w as i64 + d as i64 * alpha_permille / 1000).clamp(-(WEIGHT_LIMIT_Q8 as i64), WEIGHT_LIMIT_Q8 as i64);
        *w = if nw == 0 { w.signum() } else { nw as i16 };
    };
    for (i, d) in delta[..n * GENOME_INPUTS].iter().enumerate() {
        apply(&mut g.w_in[i], *d);
    }
    for h in 0..n {
        for k in 0..GENOME_OUTPUTS {
            apply(
                &mut g.w_out[k * n + h],
                delta[n * GENOME_INPUTS + h * GENOME_OUTPUTS + k],
            );
        }
    }
}

/// 一次完整繁育：交叉 → 突变 → 命名
pub fn breed(fitter: &Genome, other: &Genome, rate_permille: u32, rng: &mut EvolutionRng, slug: String) -> Genome {
    let mut child = crossover(fitter, other, rng);
    mutate(
        &mut child,
        rate_permille.clamp(MUTATION_RATE_RANGE_PERMILLE.0, MUTATION_RATE_RANGE_PERMILLE.1),
        rng,
    );
    enforce_page_limit(&mut child);
    child.slug = slug;
    child
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::UnitKind;

    #[test]
    fn fitness_formula_matches_bible() {
        let r = FitnessRecord {
            damage_dealt_milli: 120_000,
            survival_ticks: 600,
        };
        // 2 × 120 + 0.5 × 600 = 540
        assert_eq!(r.fitness_milli(), 540_000);
    }

    #[test]
    fn elites_take_top_ten_percent_with_minimum_two() {
        let recs: Vec<(u32, FitnessRecord)> = (0..25)
            .map(|i| {
                (
                    i,
                    FitnessRecord {
                        damage_dealt_milli: (i as i64 % 7) * 1000,
                        survival_ticks: 0,
                    },
                )
            })
            .collect();
        let e = select_elites(&recs);
        assert_eq!(e.len(), 3);
        assert_eq!(e, vec![6, 13, 20], "同分按序号升序");
        assert_eq!(select_elites(&recs[..5]).len(), 2);
        assert_eq!(select_elites(&recs[..1]).len(), 1);
    }

    #[test]
    fn gaussian_is_centered_with_unit_variance() {
        let mut rng = EvolutionRng::new(1, 2, 3);
        let n = 4000i64;
        let xs: Vec<i64> = (0..n).map(|_| rng.gaussian_q16()).collect();
        let mean = xs.iter().sum::<i64>() / n;
        let var = xs.iter().map(|x| ((x - mean) * (x - mean)) >> 16).sum::<i64>() / n;
        assert!(mean.abs() < 65536 / 10, "mean {mean}");
        assert!((var - 65536).abs() < 65536 / 10, "var {var}");
    }

    #[test]
    fn breeding_is_deterministic_and_preserves_invariants() {
        for unit in UnitKind::ALL {
            let a = Genome::seed(*unit);
            let mut b = a.clone();
            mutate(&mut b, 250, &mut EvolutionRng::new(9, 0, 1));
            let c1 = breed(&a, &b, 100, &mut EvolutionRng::new(5, 100, 7), "bred.0.1".into());
            let c2 = breed(&a, &b, 100, &mut EvolutionRng::new(5, 100, 7), "bred.0.1".into());
            assert_eq!(c1, c2);
            assert_eq!(c1.generation, 1);
            assert_eq!(c1.slot_count(), a.slot_count());
            assert!(
                c1.w_in
                    .iter()
                    .chain(&c1.w_rec)
                    .chain(&c1.w_out)
                    .all(|w| w.abs() <= WEIGHT_LIMIT_Q8)
            );
            if c1.storage == TopologyStorageClass::PagedDynamicAdjacency {
                let n = c1.n_hid as usize;
                assert!((0..n).all(|p| c1.w_rec[p * n..(p + 1) * n].iter().filter(|w| **w != 0).count() <= 32));
            }
            let c3 = breed(&a, &b, 100, &mut EvolutionRng::new(5, 101, 7), "bred.0.1".into());
            assert_ne!(c1.digest(), c3.digest(), "{unit}");
        }
    }

    #[test]
    fn cross_archetype_offspring_inherits_fitter_parent() {
        let a = Genome::seed(UnitKind::Spitter);
        let b = Genome::seed(UnitKind::Striker);
        let c = crossover(&a, &b, &mut EvolutionRng::new(0, 0, 0));
        assert_eq!(c.unit, UnitKind::Spitter);
        assert_eq!(c.w_in, a.w_in);
    }

    #[test]
    fn baldwin_moves_existing_weights_only() {
        let mut g = Genome::seed(UnitKind::Striker);
        let n = g.n_hid as usize;
        let mut delta = vec![0i16; n * 22];
        delta[0] = 100; // h0 ← i0
        delta[1] = 100;
        let before = g.clone();
        baldwin_assimilate(&mut g, &delta, 150);
        if before.w_in[0] != 0 {
            assert_eq!(g.w_in[0], before.w_in[0] + 15);
        }
        if before.w_in[1] == 0 {
            assert_eq!(g.w_in[1], 0);
        }
        assert_eq!(g.synapse_count(), before.synapse_count());
    }
}
