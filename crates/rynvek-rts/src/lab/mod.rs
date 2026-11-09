// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek Lab：批量无头实验、统计指标与演化谱系采样（服务端 Lab 作业、CLI 与 Python 绑定共用）。
//!
//! - 实验规格 [`ExperimentSpec`] = 变体 × 种子；每个 run 是一局独立的确定性对局，
//!   同一规格永远得到逐字节相同的结果（Parquet 编码见 `arrow` 特性下的 [`arrow_io`]）。
//! - 谱系：[`crate::genome::Genome`] 不记录亲代链接，谱系表因此记录每个基因组 slug
//!   首次出现的 Tick、所属阵营与代数（ERRATA E-047）。
//! - 多个 run 用 `std::thread::scope` 并行执行，结果按 run 序号归并，与线程调度无关。

pub mod policy;

#[cfg(feature = "arrow")]
pub mod arrow_io;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{BuildingKind, FactionId, UnitKind};
use crate::world::{ArenaWorld, MatchConfig, MatchMode, MatchOutcome, VictoryReason};
pub use policy::{MacroAction, MacroOutcome, PolicyKind, apply_macro, run_policy};

/// 单个实验的 run 数上限（变体数 × 种子数；与服务端 `MAX_SEEDS` 一致）
pub const MAX_RUNS: usize = 256;
/// 单个 run 的 Tick 上限（30 Hz 下 1 小时）
pub const MAX_TICKS: u64 = 108_000;
/// 默认采样间隔（Ticks）
pub const DEFAULT_SAMPLE_EVERY: u64 = 30;

/// 对局模式（实验规格中的写法）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabMode {
    /// PvE 生存（1–2 名玩家对抗野生虫潮）
    #[default]
    Pve,
    /// 双人对局（双方均由脚本策略控制）
    Duel,
}

/// 实验变体（一组对照条件）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantSpec {
    /// 变体标签（1–64 字符）
    pub label: String,
    #[serde(default)]
    pub mode: LabMode,
    /// 玩家阵营数；缺省时 PvE 为 1、双人局为 2
    #[serde(default)]
    pub players: Option<u8>,
    /// 所有玩家阵营使用的脚本策略
    #[serde(default)]
    pub policy: PolicyKind,
}

impl VariantSpec {
    /// 基线变体：单人 PvE + 经济策略
    pub fn baseline() -> Self {
        Self {
            label: "baseline".into(),
            mode: LabMode::Pve,
            players: None,
            policy: PolicyKind::Economy,
        }
    }

    pub fn player_count(&self) -> u8 {
        self.players.unwrap_or(match self.mode {
            LabMode::Pve => 1,
            LabMode::Duel => 2,
        })
    }

    /// 该变体在给定种子下的对局配置
    pub fn match_config(&self, seed: u64) -> MatchConfig {
        match self.mode {
            LabMode::Pve => MatchConfig::pve(seed, self.player_count()),
            LabMode::Duel => MatchConfig::duel(seed),
        }
    }
}

/// 实验规格（`POST /api/v1/lab/experiments` 请求体中的实验部分）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentSpec {
    pub name: String,
    pub seeds: Vec<u64>,
    pub ticks: u64,
    #[serde(default = "default_sample_every")]
    pub sample_every: u64,
    /// 缺省为单个基线变体
    #[serde(default)]
    pub variants: Vec<VariantSpec>,
}

fn default_sample_every() -> u64 {
    DEFAULT_SAMPLE_EVERY
}

impl ExperimentSpec {
    /// 实际生效的变体列表
    pub fn effective_variants(&self) -> Vec<VariantSpec> {
        if self.variants.is_empty() {
            vec![VariantSpec::baseline()]
        } else {
            self.variants.clone()
        }
    }

    /// 校验规格；返回人类可读的拒绝原因
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 128 {
            return Err("name 必须为 1–128 字符".into());
        }
        if self.seeds.is_empty() {
            return Err("seeds 不能为空".into());
        }
        if self.ticks == 0 || self.ticks > MAX_TICKS {
            return Err(format!("ticks 必须在 1..={MAX_TICKS}"));
        }
        if self.sample_every == 0 || self.sample_every > self.ticks {
            return Err("sample_every 必须在 1..=ticks".into());
        }
        let variants = self.effective_variants();
        let runs = variants.len().saturating_mul(self.seeds.len());
        if runs > MAX_RUNS {
            return Err(format!("变体数 × 种子数 = {runs}，超过上限 {MAX_RUNS}"));
        }
        let mut labels = std::collections::BTreeSet::new();
        for v in &variants {
            if v.label.is_empty() || v.label.chars().count() > 64 {
                return Err("变体 label 必须为 1–64 字符".into());
            }
            if !labels.insert(v.label.as_str()) {
                return Err(format!("变体 label 重复：{}", v.label));
            }
            let p = v.player_count();
            let ok = match v.mode {
                LabMode::Pve => (1..=2).contains(&p),
                LabMode::Duel => p == 2,
            };
            if !ok {
                return Err(format!("变体 {} 的 players={p} 与模式不符", v.label));
            }
        }
        Ok(())
    }

    /// 展开为 run 列表（变体优先、种子次之）
    pub fn runs(&self) -> Vec<RunPlan> {
        let mut out = Vec::new();
        for v in self.effective_variants() {
            for &seed in &self.seeds {
                out.push(RunPlan {
                    run: out.len() as u32,
                    variant: v.clone(),
                    seed,
                });
            }
        }
        out
    }
}

/// 单个 run 的计划
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunPlan {
    pub run: u32,
    pub variant: VariantSpec,
    pub seed: u64,
}

/// run 汇总行
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunRecord {
    pub run: u32,
    pub variant: String,
    pub seed: u64,
    pub mode: String,
    pub players: u8,
    pub policy: String,
    pub final_tick: u64,
    /// 胜方阵营（未分胜负为 None）
    pub winner: Option<u8>,
    /// 结束原因（未结束为 "running"）
    pub end_reason: String,
    pub final_state_root: [u8; 32],
    pub commands_accepted: u64,
    pub commands_rejected: u64,
    pub genomes_bred: u32,
    pub waves_spawned: u32,
}

/// 指标采样行（每个采样 Tick × 玩家阵营一行）
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MetricRow {
    pub run: u32,
    pub tick: u64,
    pub faction: u8,
    pub biomass_milli: i64,
    pub flux_milli: i64,
    pub neurogel_milli: i64,
    pub biomass_harvested_total_milli: i64,
    pub supply_used: u32,
    pub supply_cap: u32,
    pub units: u32,
    pub buildings: u32,
    pub score_milli: i64,
    pub wild_units: u32,
    pub pve_wave: Option<u32>,
    pub state_root: [u8; 32],
}

/// 谱系行：基因组 slug 首次出现
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineageRow {
    pub run: u32,
    /// 所属阵营（野生虫潮为 255）
    pub faction: u8,
    pub slug: String,
    pub unit: String,
    pub generation: u32,
    pub first_seen_tick: u64,
}

/// 一个实验的全部结果
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExperimentResult {
    pub runs: Vec<RunRecord>,
    pub metrics: Vec<MetricRow>,
    pub lineage: Vec<LineageRow>,
}

/// 固定长度的阵营特征向量（定点整数，Python 侧零拷贝导出为 Int64Array）
pub const FEATURE_NAMES: [&str; 24] = [
    "tick",
    "biomass_milli",
    "flux_milli",
    "neurogel_milli",
    "biomass_harvested_total_milli",
    "supply_used",
    "supply_cap",
    "score_milli",
    "own_forager",
    "own_striker",
    "own_carapace",
    "own_spitter",
    "own_weaver",
    "own_leviathan",
    "own_buildings",
    "own_spawning_vats",
    "own_harvesters",
    "own_turrets",
    "visible_enemy_units",
    "visible_enemy_buildings",
    "nexus_hp_milli",
    "pve_wave",
    "outcome",
    "genomes_known",
];

/// 计算阵营特征（只读取该阵营可见的信息，遵守战争迷雾）
pub fn faction_features(world: &ArenaWorld, faction: FactionId) -> Option<[i64; 24]> {
    let obs = world.observe(faction)?;
    let own_kind = |k: UnitKind| obs.units.iter().filter(|u| u.faction == faction && u.kind == k).count() as i64;
    let own_bld = |k: BuildingKind| {
        obs.buildings
            .iter()
            .filter(|b| b.faction == faction && b.kind == k)
            .count() as i64
    };
    let nexus_hp = obs
        .buildings
        .iter()
        .find(|b| b.faction == faction && b.kind == BuildingKind::NexusCore)
        .map_or(0, |b| b.hp_milli);
    let outcome = match outcome_parts(obs.outcome.as_ref()) {
        (None, reason) if reason == "running" => 0,
        (Some(w), _) if w == faction.0 => 1,
        (Some(_), _) => 2,
        (None, reason) if reason == "defeat" => 2,
        (None, _) => 3,
    };
    Some([
        obs.tick as i64,
        obs.ledger.balance.biomass,
        obs.ledger.balance.flux,
        obs.ledger.balance.neurogel,
        obs.ledger.biomass_harvested_total,
        i64::from(obs.supply.0),
        i64::from(obs.supply.1),
        obs.score_milli,
        own_kind(UnitKind::Forager),
        own_kind(UnitKind::Striker),
        own_kind(UnitKind::Carapace),
        own_kind(UnitKind::Spitter),
        own_kind(UnitKind::Weaver),
        own_kind(UnitKind::Leviathan),
        obs.buildings.iter().filter(|b| b.faction == faction).count() as i64,
        own_bld(BuildingKind::SpawningVat),
        own_bld(BuildingKind::BiomassHarvester),
        own_bld(BuildingKind::PulseTurret),
        obs.units.iter().filter(|u| u.faction != faction).count() as i64,
        obs.buildings.iter().filter(|b| b.faction != faction).count() as i64,
        nexus_hp,
        obs.pve_wave.map_or(-1, i64::from),
        outcome,
        obs.genomes.len() as i64,
    ])
}

/// 拆解对局结果为（胜方阵营, 原因）；原因取值 running / draw / defeat / 胜利原因的 snake_case
pub fn outcome_parts(outcome: Option<&MatchOutcome>) -> (Option<u8>, String) {
    match outcome {
        None => (None, "running".into()),
        Some(MatchOutcome::Draw { .. }) => (None, "draw".into()),
        Some(MatchOutcome::Defeat { .. }) => (None, "defeat".into()),
        Some(MatchOutcome::Victory { winner, reason, .. }) => {
            let r = match reason {
                VictoryReason::Elimination => "elimination",
                VictoryReason::Surrender => "surrender",
                VictoryReason::TieBreakerScore => "tie_breaker_score",
                VictoryReason::SuddenDeath => "sudden_death",
                VictoryReason::WavesCleared => "waves_cleared",
            };
            (Some(winner.0), r.to_owned())
        }
    }
}

/// 执行单个 run
pub fn run_one(plan: &RunPlan, ticks: u64, sample_every: u64) -> ExperimentResult {
    let mut world = ArenaWorld::new(plan.variant.match_config(plan.seed));
    let players: Vec<FactionId> = world.factions.iter().map(|f| f.id).collect();
    let mut accepted = 0u64;
    let mut rejected = 0u64;
    let mut bred = 0u32;
    let mut waves = 0u32;
    let mut metrics = Vec::new();
    let mut seen: BTreeMap<String, LineageRow> = BTreeMap::new();
    let observe_lineage = |world: &ArenaWorld, seen: &mut BTreeMap<String, LineageRow>| {
        let mut note = |faction: u8, g: &crate::genome::Genome| {
            seen.entry(g.slug.clone()).or_insert_with(|| LineageRow {
                run: plan.run,
                faction,
                slug: g.slug.clone(),
                unit: g.unit.as_str().to_owned(),
                generation: g.generation,
                first_seen_tick: world.tick,
            });
        };
        for f in &world.factions {
            for g in f.genomes.values() {
                note(f.id.0, g);
            }
        }
        for u in world.units.values().filter(|u| u.faction == FactionId::WILD) {
            note(FactionId::WILD.0, &u.genome);
        }
    };
    observe_lineage(&world, &mut seen);
    while world.tick < ticks && !world.is_over() {
        for &f in &players {
            let o = run_policy(&mut world, f, plan.variant.policy);
            accepted += u64::from(o.accepted);
            rejected += u64::from(o.rejected);
        }
        world.step();
        for ev in world.drain_events() {
            match ev {
                crate::world::RtsEvent::GenomeBred { .. } => bred += 1,
                crate::world::RtsEvent::WaveSpawned { .. } => waves += 1,
                _ => {}
            }
        }
        if world.tick % sample_every == 0 || world.is_over() || world.tick == ticks {
            sample(&world, plan.run, &players, &mut metrics);
            observe_lineage(&world, &mut seen);
        }
    }
    let outcome = world.outcome.clone();
    let mode = match plan.variant.mode {
        LabMode::Pve => "pve",
        LabMode::Duel => "duel",
    };
    let policy = match plan.variant.policy {
        PolicyKind::Idle => "idle",
        PolicyKind::Economy => "economy",
    };
    let mut lineage: Vec<LineageRow> = seen.into_values().collect();
    lineage.sort_by(|a, b| (a.first_seen_tick, &a.slug).cmp(&(b.first_seen_tick, &b.slug)));
    ExperimentResult {
        runs: vec![RunRecord {
            run: plan.run,
            variant: plan.variant.label.clone(),
            seed: plan.seed,
            mode: mode.into(),
            players: plan.variant.player_count(),
            policy: policy.into(),
            final_tick: world.tick,
            winner: outcome_parts(outcome.as_ref()).0,
            end_reason: outcome_parts(outcome.as_ref()).1,
            final_state_root: world.state_hash().0,
            commands_accepted: accepted,
            commands_rejected: rejected,
            genomes_bred: bred,
            waves_spawned: waves,
        }],
        metrics,
        lineage,
    }
}

fn sample(world: &ArenaWorld, run: u32, players: &[FactionId], out: &mut Vec<MetricRow>) {
    let root = world.state_hash().0;
    let wild_units = world.units.values().filter(|u| u.faction == FactionId::WILD).count() as u32;
    for &f in players {
        let Some(obs) = world.observe(f) else { continue };
        out.push(MetricRow {
            run,
            tick: world.tick,
            faction: f.0,
            biomass_milli: obs.ledger.balance.biomass,
            flux_milli: obs.ledger.balance.flux,
            neurogel_milli: obs.ledger.balance.neurogel,
            biomass_harvested_total_milli: obs.ledger.biomass_harvested_total,
            supply_used: obs.supply.0,
            supply_cap: obs.supply.1,
            units: world.units.values().filter(|u| u.faction == f).count() as u32,
            buildings: world.buildings.values().filter(|b| b.faction == f).count() as u32,
            score_milli: obs.score_milli,
            wild_units,
            pve_wave: obs.pve_wave,
            state_root: root,
        });
    }
}

/// 执行整个实验（多线程并行；`on_run_done(完成数, 总数)` 用于进度上报）
pub fn run_experiment(spec: &ExperimentSpec, on_run_done: &(dyn Fn(usize, usize) + Sync)) -> ExperimentResult {
    let plans = spec.runs();
    let total = plans.len();
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(total.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<ExperimentResult>>> =
        (0..total).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(plan) = plans.get(i) else { break };
                    let r = run_one(plan, spec.ticks, spec.sample_every);
                    *slots[i].lock().expect("结果槽锁未中毒") = Some(r);
                    let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    on_run_done(d, total);
                }
            });
        }
    });
    let mut out = ExperimentResult::default();
    for slot in slots {
        let r = slot.into_inner().expect("结果槽锁未中毒").expect("每个 run 都已完成");
        out.runs.extend(r.runs);
        out.metrics.extend(r.metrics);
        out.lineage.extend(r.lineage);
    }
    out
}

/// `MatchMode` 的 Lab 写法
pub fn lab_mode_of(mode: MatchMode) -> LabMode {
    match mode {
        MatchMode::PveSurvival => LabMode::Pve,
        _ => LabMode::Duel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ExperimentSpec {
        serde_json::from_value(serde_json::json!({
            "name": "smoke",
            "seeds": [1, 2],
            "ticks": 900,
            "sample_every": 300,
            "variants": [
                {"label": "eco", "policy": "economy"},
                {"label": "idle", "policy": "idle"},
                {"label": "duel", "mode": "duel"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn spec_validation() {
        let s = spec();
        s.validate().unwrap();
        assert_eq!(s.runs().len(), 6);
        let mut bad = s.clone();
        bad.ticks = MAX_TICKS + 1;
        assert!(bad.validate().is_err());
        let mut dup = s.clone();
        dup.variants[1].label = "eco".into();
        assert!(dup.validate().is_err());
        let mut p = s;
        p.variants[2].players = Some(1);
        assert!(p.validate().is_err());
    }

    #[test]
    fn experiment_is_deterministic_and_matches_serial_runs() {
        let s = spec();
        let a = run_experiment(&s, &|_, _| {});
        let b = run_experiment(&s, &|_, _| {});
        assert_eq!(a, b);
        assert_eq!(a.runs.len(), 6);
        // 并行结果与串行单跑一致
        let plan = &s.runs()[4];
        assert_eq!(run_one(plan, s.ticks, s.sample_every).runs[0], a.runs[4]);
        // 每个 run 采样 3 次（300/600/900）× 玩家阵营数
        assert_eq!(a.metrics.iter().filter(|m| m.run == 0).count(), 3);
        assert_eq!(a.metrics.iter().filter(|m| m.run == 4).count(), 6);
        // 种子基因组全部进入谱系表
        assert!(a.lineage.iter().filter(|l| l.run == 0).count() >= 6);
        // 经济策略确实下达了命令，空闲策略没有
        assert!(a.runs[0].commands_accepted > 0);
        assert_eq!(a.runs[2].commands_accepted, 0);
    }

    #[test]
    fn features_have_fixed_shape() {
        let w = ArenaWorld::new(MatchConfig::duel(3));
        let f = faction_features(&w, FactionId(0)).unwrap();
        assert_eq!(f.len(), FEATURE_NAMES.len());
        assert_eq!(f[8], 3, "开局 3 只采集工蜂");
        assert_eq!(f[9], 1, "开局 1 只突击虫");
        assert!(faction_features(&w, FactionId(9)).is_none());
    }
}
