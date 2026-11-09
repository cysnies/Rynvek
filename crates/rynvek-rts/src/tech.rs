// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三阶工业-生物协同科技与演化树（项目文档 §5.4，补全 GAP-20）。
//!
//! Tier 1 在 `Synapse Refinery` 研发，Tier 2 / Tier 3 在 `Mutation Crucible` 研发；全阵营共享一套确定性
//! 科技状态机（[`TechSet`] 位集）。各节点的数值加成汇总为 [`TechModifiers`]，由仿真逐处查询。

use serde::{Deserialize, Serialize};

use crate::ids::{BuildingKind, TechId, UnitKind};
use crate::math::MILLI;
use crate::resources::ResourceAmounts;

/// 单个科技节点
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechSpec {
    pub id: TechId,
    pub name_zh: &'static str,
    pub tier: u8,
    /// 前置建筑（须至少拥有一座已建成的）
    pub requires_buildings: &'static [BuildingKind],
    /// 前置科技
    pub requires_techs: &'static [TechId],
    pub cost: ResourceAmounts,
    pub research_ticks: u32,
}

impl TechSpec {
    /// 研发地点：Tier 1 → 突触精炼厂；Tier 2 / 3 → 演化坩埚
    pub const fn research_site(&self) -> BuildingKind {
        if self.tier == 1 {
            BuildingKind::SynapseRefinery
        } else {
            BuildingKind::MutationCrucible
        }
    }
}

/// 设计圣经列序 `Biomass / Neurogel / Flux`
const fn cost(bio: i64, gel: i64, flux: i64) -> ResourceAmounts {
    ResourceAmounts {
        biomass: bio * MILLI,
        flux: flux * MILLI,
        neurogel: gel * MILLI,
    }
}

/// 九项科技节点（顺序与设计圣经一致）
pub const TECH_SPECS: [TechSpec; 9] = [
    TechSpec {
        id: TechId::ChemotaxisAmp,
        name_zh: "趋化受体增敏",
        tier: 1,
        requires_buildings: &[BuildingKind::SpawningVat],
        requires_techs: &[],
        cost: cost(150, 30, 100),
        research_ticks: 450,
    },
    TechSpec {
        id: TechId::FluxCapacitance,
        name_zh: "超导通量电容",
        tier: 1,
        requires_buildings: &[BuildingKind::FluxReactor],
        requires_techs: &[],
        cost: cost(200, 40, 150),
        research_ticks: 600,
    },
    TechSpec {
        id: TechId::KineticResonance,
        name_zh: "高频脉冲聚焦",
        tier: 1,
        requires_buildings: &[BuildingKind::PulseTurret],
        requires_techs: &[],
        cost: cost(180, 50, 120),
        research_ticks: 600,
    },
    TechSpec {
        id: TechId::MulticellularChitin,
        name_zh: "多细胞几丁质装甲",
        tier: 2,
        requires_buildings: &[BuildingKind::MutationCrucible],
        requires_techs: &[],
        cost: cost(250, 80, 200),
        research_ticks: 750,
    },
    TechSpec {
        id: TechId::HebbianPlasticity,
        name_zh: "四因子可塑性激活",
        tier: 2,
        requires_buildings: &[BuildingKind::MutationCrucible],
        requires_techs: &[],
        cost: cost(300, 100, 250),
        research_ticks: 900,
    },
    TechSpec {
        id: TechId::GapJunctionSync,
        name_zh: "间隙连接电耦合",
        tier: 2,
        requires_buildings: &[],
        requires_techs: &[TechId::HebbianPlasticity],
        cost: cost(320, 120, 250),
        research_ticks: 900,
    },
    TechSpec {
        id: TechId::MorphogenesisApex,
        name_zh: "高阶形态发生学",
        tier: 3,
        requires_buildings: &[],
        requires_techs: &[TechId::GapJunctionSync],
        cost: cost(500, 220, 400),
        research_ticks: 1200,
    },
    TechSpec {
        id: TechId::MetabolicSuperconduct,
        name_zh: "超导代谢闭环",
        tier: 3,
        // 设计圣经：`tech.flux_capacitance` + `Crucible`
        requires_buildings: &[BuildingKind::MutationCrucible],
        requires_techs: &[TechId::FluxCapacitance],
        cost: cost(400, 180, 350),
        research_ticks: 1050,
    },
    TechSpec {
        id: TechId::DirectedSpeciation,
        name_zh: "定向物种跃迁",
        tier: 3,
        requires_buildings: &[],
        requires_techs: &[TechId::MorphogenesisApex],
        cost: cost(450, 250, 400),
        research_ticks: 1200,
    },
];

/// 查询科技节点
pub const fn tech_spec(id: TechId) -> &'static TechSpec {
    &TECH_SPECS[id as usize]
}

/// 已研发科技集合（9 位位集）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TechSet(pub u16);

impl TechSet {
    pub const fn contains(self, id: TechId) -> bool {
        self.0 & (1 << id as u16) != 0
    }

    pub const fn with(self, id: TechId) -> Self {
        Self(self.0 | (1 << id as u16))
    }

    pub fn iter(self) -> impl Iterator<Item = TechId> {
        TechId::ALL.iter().copied().filter(move |t| self.contains(*t))
    }

    /// 前置科技是否满足（建筑前置由仿真按已建成建筑另行检查）
    pub fn techs_satisfied(self, spec: &TechSpec) -> bool {
        spec.requires_techs.iter().all(|t| self.contains(*t))
    }

    /// 汇总数值修正
    pub fn modifiers(self) -> TechModifiers {
        TechModifiers::from_set(self)
    }

    /// 单位是否已解锁
    pub fn unit_unlocked(self, kind: UnitKind) -> bool {
        crate::units::unit_spec(kind).unlocked_by.is_none_or(|t| self.contains(t))
    }
}

/// 科技带来的确定性数值修正（全部以整数千分比 / 毫单位表示）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechModifiers {
    /// Forager / Striker 对 signal_a/b 与营养梯度的感知增益（千分比）：`1000 → 1300`
    pub chemotaxis_gain_permille: i64,
    /// Forager 采集速率（毫 Biomass / 秒）：`4000 → 5200`
    pub forager_harvest_milli_per_s: i64,
    /// Flux Reactor 发电倍率（千分比）：`1000 → 1150`
    pub reactor_output_permille: i64,
    /// 阵营基础通量电容附加（毫单位）：`0 → +1000`
    pub flux_capacity_bonus_milli: i64,
    /// Striker 与 Pulse Turret 的 Kinetic 伤害倍率（千分比）：`1000 → 1200`
    pub kinetic_damage_permille: i64,
    /// Striker 与 Pulse Turret 射程附加（毫 Tile）：`0 → +1000`
    pub kinetic_range_bonus_milli: i64,
    /// 所有友方单位护甲附加：`0 → +2`
    pub unit_armor_bonus: i64,
    /// 战地四因子 STDP（η_learn：`0 → 0.02`）
    pub plasticity_enabled: bool,
    /// 老兵门槛（存活 Tick 数）；`None` 为未解锁
    pub veteran_threshold_ticks: Option<u64>,
    /// 老兵射速 / 闪避加成上限（百分点）
    pub veteran_bonus_percent: i64,
    /// 信标调制广播半径（Tiles）：`16 → 22`
    pub beacon_radius_tiles: u32,
    /// 单脑最大神经元数 N_max：`128 → 256`
    pub max_brain_neurons: u32,
    /// 每次脉冲代谢能耗 c_spike（微 Energy）：`800 → 480`（即 `0.80 → 0.48 milli-Energy`）
    pub spike_cost_micro_energy: i64,
    /// 死亡残骸生物质回收率（百分比）：`75 → 90`
    pub corpse_recycle_percent: i64,
    /// 精英繁育耗时倍率（百分比）：`100 → 50`
    pub breeding_time_percent: i64,
    /// 鲍德温同化率 α_baldwin（千分比）：`50 → 150`
    pub baldwin_alpha_permille: i64,
}

/// η_learn 基础值（千分比）：未研发时为 0
pub const HEBBIAN_LEARNING_RATE_PERMILLE: i64 = 20;
/// 老兵门槛：存活超过 `900 Ticks`
pub const VETERAN_THRESHOLD_TICKS: u64 = 900;
/// 研发 `tech.morphogenesis_apex` 前的单脑神经元上限（Leviathan 种子脑 N = 128）
pub const BASE_MAX_BRAIN_NEURONS: u32 = 128;

impl TechModifiers {
    pub fn from_set(set: TechSet) -> Self {
        let has = |t| set.contains(t);
        Self {
            chemotaxis_gain_permille: if has(TechId::ChemotaxisAmp) { 1300 } else { 1000 },
            forager_harvest_milli_per_s: if has(TechId::ChemotaxisAmp) {
                crate::units::FORAGER_HARVEST_MILLI_PER_S_CHEMOTAXIS
            } else {
                crate::units::unit_spec(UnitKind::Forager).harvest_milli_per_s
            },
            reactor_output_permille: if has(TechId::FluxCapacitance) { 1150 } else { 1000 },
            flux_capacity_bonus_milli: if has(TechId::FluxCapacitance) { 1000 * MILLI } else { 0 },
            kinetic_damage_permille: if has(TechId::KineticResonance) { 1200 } else { 1000 },
            kinetic_range_bonus_milli: if has(TechId::KineticResonance) { MILLI } else { 0 },
            unit_armor_bonus: if has(TechId::MulticellularChitin) { 2 } else { 0 },
            plasticity_enabled: has(TechId::HebbianPlasticity),
            veteran_threshold_ticks: has(TechId::HebbianPlasticity).then_some(VETERAN_THRESHOLD_TICKS),
            veteran_bonus_percent: if has(TechId::HebbianPlasticity) { 20 } else { 0 },
            beacon_radius_tiles: if has(TechId::GapJunctionSync) {
                crate::buildings::BEACON_RADIUS_TILES_GAP_JUNCTION
            } else {
                crate::buildings::BEACON_RADIUS_TILES
            },
            max_brain_neurons: if has(TechId::MorphogenesisApex) { 256 } else { BASE_MAX_BRAIN_NEURONS },
            spike_cost_micro_energy: if has(TechId::MetabolicSuperconduct) { 480 } else { 800 },
            corpse_recycle_percent: if has(TechId::MetabolicSuperconduct) { 90 } else { 75 },
            breeding_time_percent: if has(TechId::DirectedSpeciation) { 50 } else { 100 },
            baldwin_alpha_permille: if has(TechId::DirectedSpeciation) { 150 } else { 50 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_indexed_by_id_and_sites_follow_tiers() {
        for id in TechId::ALL {
            let s = tech_spec(*id);
            assert_eq!(s.id, *id);
            let site = s.research_site();
            assert_eq!(site == BuildingKind::SynapseRefinery, s.tier == 1);
        }
    }

    #[test]
    fn prerequisite_graph_is_acyclic_and_topologically_ordered() {
        // 表格顺序本身即拓扑序：每个前置科技都出现在节点之前
        for (i, s) in TECH_SPECS.iter().enumerate() {
            for p in s.requires_techs {
                assert!((*p as usize) < i, "{} 的前置 {} 应在其之前", s.id, p);
            }
        }
    }

    #[test]
    fn modifiers_reflect_bible_numbers() {
        let none = TechSet::default().modifiers();
        assert_eq!(none.forager_harvest_milli_per_s, 4_000);
        assert_eq!(none.beacon_radius_tiles, 16);
        assert_eq!(none.corpse_recycle_percent, 75);
        let all = TechId::ALL.iter().fold(TechSet::default(), |s, t| s.with(*t)).modifiers();
        assert_eq!(all.forager_harvest_milli_per_s, 5_200);
        // 60 Flux/s × 1.15 = 69 Flux/s
        assert_eq!(60_000 * all.reactor_output_permille / 1000, 69_000);
        assert_eq!(all.flux_capacity_bonus_milli, 1_000_000);
        assert_eq!(all.beacon_radius_tiles, 22);
        assert_eq!(all.max_brain_neurons, 256);
        assert_eq!(all.spike_cost_micro_energy, 480);
        assert_eq!(all.corpse_recycle_percent, 90);
        assert_eq!(all.breeding_time_percent, 50);
        assert_eq!(all.baldwin_alpha_permille, 150);
        assert!(all.plasticity_enabled);
    }

    #[test]
    fn unit_unlocks_follow_tech_tree() {
        let s = TechSet::default();
        assert!(s.unit_unlocked(UnitKind::Forager) && s.unit_unlocked(UnitKind::Striker));
        assert!(!s.unit_unlocked(UnitKind::Carapace) && !s.unit_unlocked(UnitKind::Spitter));
        let s = s.with(TechId::MulticellularChitin);
        assert!(s.unit_unlocked(UnitKind::Carapace) && s.unit_unlocked(UnitKind::Spitter));
        assert!(!s.unit_unlocked(UnitKind::Weaver));
        assert!(s.with(TechId::GapJunctionSync).unit_unlocked(UnitKind::Weaver));
        assert!(s.with(TechId::MorphogenesisApex).unit_unlocked(UnitKind::Leviathan));
    }
}
