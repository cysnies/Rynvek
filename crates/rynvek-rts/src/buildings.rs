// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 八种工业网格建筑平衡表（项目文档 §5.3.1，数值逐项冻结）。
//!
//! 表中「造价 (Biomass/Neurogel)」「建造耗时」「HP / Armor」「Flux 收支」直接对应 [`BuildingSpec`] 字段；
//! 「核心产出、容量与战术参数」列拆为本模块的具名常量。`tests/balance_tables.rs` 解析设计圣经原文逐格比对。

use crate::combat::WeaponSpec;
use crate::ids::{BuildingKind, DamageType};
use crate::math::MILLI;
use crate::resources::ResourceAmounts;

/// 单种建筑的平衡参数
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingSpec {
    pub kind: BuildingKind,
    pub name_zh: &'static str,
    /// 正方形占地边长（Tiles）
    pub footprint: u32,
    /// 造价：只含 Biomass 与 Neurogel（Flux 恒为 0）
    pub cost: ResourceAmounts,
    pub build_ticks: u32,
    pub hp_milli: i64,
    pub armor: i64,
    /// Flux 收支（毫单位 / 秒；正为产出，负为运转能耗）
    pub flux_per_s_milli: i64,
    /// 视野（Tiles）；设计圣经只给出主脑 24，其余建筑见 [`DEFAULT_BUILDING_VISION_TILES`]
    pub vision_tiles: u32,
}

#[allow(clippy::too_many_arguments)] // 参数与平衡表列一一对应，便于逐格核对
const fn spec(
    kind: BuildingKind,
    name_zh: &'static str,
    footprint: u32,
    biomass: i64,
    neurogel: i64,
    build_ticks: u32,
    hp: i64,
    armor: i64,
    flux_per_s_milli: i64,
    vision_tiles: u32,
) -> BuildingSpec {
    BuildingSpec {
        kind,
        name_zh,
        footprint,
        cost: ResourceAmounts::new(biomass * MILLI, 0, neurogel * MILLI),
        build_ticks,
        hp_milli: hp * MILLI,
        armor,
        flux_per_s_milli,
        vision_tiles,
    }
}

/// 设计圣经未给出的非主脑建筑视野（Tiles）：取 8，脉冲防御塔取其射程 12（ERRATA E-023）
pub const DEFAULT_BUILDING_VISION_TILES: u32 = 8;

/// 八种建筑平衡表（顺序与设计圣经一致）
#[rustfmt::skip] // 保持「一行一建筑」的表格形态，便于与设计圣经逐格比对
pub const BUILDING_SPECS: [BuildingSpec; 8] = [
    spec(BuildingKind::NexusCore, "神经主脑核心", 4, 0, 0, 0, 4000, 15, 30_000, NEXUS_VISION_TILES),
    spec(BuildingKind::BiomassHarvester, "生物质萃取器", 2, 120, 0, 150, 450, 3, -6_000, DEFAULT_BUILDING_VISION_TILES),
    spec(BuildingKind::FluxReactor, "通量反应堆", 3, 200, 20, 240, 750, 5, 60_000, DEFAULT_BUILDING_VISION_TILES),
    spec(BuildingKind::SynapseRefinery, "突触精炼厂", 3, 250, 0, 300, 800, 5, -25_000, DEFAULT_BUILDING_VISION_TILES),
    spec(BuildingKind::SpawningVat, "神经孵化槽", 3, 180, 40, 210, 900, 6, -15_000, DEFAULT_BUILDING_VISION_TILES),
    spec(BuildingKind::SignalBeacon, "神经调制信标", 2, 100, 30, 120, 400, 2, -12_000, DEFAULT_BUILDING_VISION_TILES),
    spec(BuildingKind::PulseTurret, "脉冲防御塔", 2, 150, 15, 150, 650, 8, -10_000, TURRET_RANGE_TILES),
    spec(BuildingKind::MutationCrucible, "演化坩埚", 3, 300, 120, 360, 1000, 8, -40_000, DEFAULT_BUILDING_VISION_TILES),
];

/// 查询建筑平衡参数
pub const fn building_spec(kind: BuildingKind) -> &'static BuildingSpec {
    &BUILDING_SPECS[kind as usize]
}

// --- bld.nexus_core ---------------------------------------------------------

/// 主脑视野 `24 Tiles`
pub const NEXUS_VISION_TILES: u32 = 24;
/// 主脑脉冲自卫炮：射程 `10`、伤害 `20`、冷却 `15 Ticks`（伤害类型规范未给出，按「脉冲」取 Kinetic，ERRATA E-023）
pub const NEXUS_WEAPON: WeaponSpec = WeaponSpec::single(DamageType::Kinetic, 20 * MILLI, 10 * MILLI, 15);

// --- bld.biomass_harvester -------------------------------------------------

/// 萃取器每 Tick 从覆盖的 2×2 网格最多抽取 `0.4` 营养（= `+12.0 Biomass/s`）
pub const HARVESTER_MAX_DRAW_MILLI_PER_TICK: i64 = 400;

// --- bld.flux_reactor ------------------------------------------------------

/// 反应堆消耗 `1.5 Biomass/s`
pub const REACTOR_BIOMASS_UPKEEP_MILLI_PER_S: i64 = 1_500;
/// 反应堆增加通量电容上限 `+500`
pub const REACTOR_FLUX_CAPACITY_BONUS_MILLI: i64 = 500 * MILLI;
/// 热液裂隙上发电 `+90 Flux/s`
pub const REACTOR_FISSURE_FLUX_PER_S_MILLI: i64 = 90_000;

// --- bld.synapse_refinery --------------------------------------------------

/// 精炼周期 `60 Ticks`（2.0 s）
pub const REFINERY_CYCLE_TICKS: u32 = 60;
/// 每周期消耗 `10 Biomass + 50 Flux`
pub const REFINERY_CYCLE_INPUT: ResourceAmounts = ResourceAmounts::whole(10, 50, 0);
/// 每周期产出 `5.0 Neurogel`（= `+2.5 Neurogel/s`）
pub const REFINERY_CYCLE_OUTPUT_NEUROGEL_MILLI: i64 = 5 * MILLI;
/// 精炼厂增加胶质上限 `+250`
pub const REFINERY_NEUROGEL_CAPACITY_BONUS_MILLI: i64 = 250 * MILLI;

// --- bld.spawning_vat ------------------------------------------------------

/// 孵化槽生产队列上限 `5`
pub const VAT_QUEUE_LIMIT: usize = 5;
/// 每座孵化槽提供神经人口容量 `+24 Supply`
pub const VAT_SUPPLY_BONUS: u32 = 24;

// --- bld.signal_beacon -----------------------------------------------------

/// 信标作用 / 调制广播半径 `16 Tiles`（`tech.gap_junction_sync` 后为 `22`）
pub const BEACON_RADIUS_TILES: u32 = 16;
/// 研发间隙连接电耦合后的广播半径
pub const BEACON_RADIUS_TILES_GAP_JUNCTION: u32 = 22;
/// 反隐脉冲侦测半径 `14 Tiles`
pub const BEACON_DETECTION_RADIUS_TILES: u32 = 14;
/// 信号注入强度量程 `[-50, +50]`（毫单位）
pub const BEACON_SIGNAL_STRENGTH_RANGE_MILLI: (i64, i64) = (-50 * MILLI, 50 * MILLI);

// --- bld.pulse_turret ------------------------------------------------------

/// 防御塔射程 `12 Tiles`
pub const TURRET_RANGE_TILES: u32 = 12;
/// 防御塔武器：`28` 点 Kinetic、射程 `12`、冷却 `15 Ticks`、单发消耗 `4 Flux`
pub const TURRET_WEAPON: WeaponSpec = WeaponSpec {
    damage_type: DamageType::Kinetic,
    damage_milli: 28 * MILLI,
    range_milli: TURRET_RANGE_TILES as i64 * MILLI,
    cooldown_ticks: 15,
    arc_half_angle_deg: None,
    hazard_deposit_milli: 0,
    flux_per_shot_milli: 4 * MILLI,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_indexed_by_kind() {
        for kind in BuildingKind::ALL {
            assert_eq!(building_spec(*kind).kind, *kind);
            assert_eq!(building_spec(*kind).cost.flux, 0);
        }
    }

    #[test]
    fn derived_rates_match_bible_annotations() {
        // 萃取器 0.4/Tick × 30 = 12.0 Biomass/s；精炼厂 5 / 2 s = 2.5 Neurogel/s；防御塔 30 / 15 = 2.0 发/秒
        assert_eq!(HARVESTER_MAX_DRAW_MILLI_PER_TICK * 30, 12_000);
        assert_eq!(
            REFINERY_CYCLE_OUTPUT_NEUROGEL_MILLI * 30 / REFINERY_CYCLE_TICKS as i64,
            2_500
        );
        assert_eq!(30 / TURRET_WEAPON.cooldown_ticks, 2);
        assert_eq!(
            REACTOR_FISSURE_FLUX_PER_S_MILLI,
            building_spec(BuildingKind::FluxReactor).flux_per_s_milli * 3 / 2
        );
    }
}
