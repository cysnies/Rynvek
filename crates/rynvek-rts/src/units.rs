// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 六种神经原生单位集群平衡表（项目文档 §5.3.2，数值逐项冻结）。
//!
//! 造价列在设计圣经中的顺序是 `Bio / Gel / Flux`，[`ResourceAmounts`] 字段顺序为 `biomass / flux / neurogel`，
//! 构造时按名称赋值以免错位。解锁科技取自 §5.4 科技树。

use crate::combat::WeaponSpec;
use crate::ids::{DamageType, TechId, UnitKind};
use crate::math::MILLI;
use crate::resources::ResourceAmounts;

/// 支援光环（`unit.weaver`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportAura {
    /// 半径（毫 Tile）
    pub radius_milli: i64,
    /// 友军治疗（毫 HP / 秒）
    pub heal_milli_per_s: i64,
    /// 脉冲相位同步：命中 / 闪避加成（百分点）
    pub sync_bonus_percent: i64,
}

/// 单种单位的平衡参数
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitSpec {
    pub kind: UnitKind,
    pub name_zh: &'static str,
    pub supply: u32,
    pub cost: ResourceAmounts,
    pub hatch_ticks: u32,
    pub hp_milli: i64,
    pub shield_milli: i64,
    pub armor: i64,
    pub radius_milli: i64,
    /// 移速（毫 Units / 秒）
    pub speed_milli_per_s: i64,
    pub vision_tiles: u32,
    pub weapon: Option<WeaponSpec>,
    /// 采集速率（毫 Biomass / 秒），仅 Forager
    pub harvest_milli_per_s: i64,
    pub support_aura: Option<SupportAura>,
    /// 移动调质光环半径（毫 Tile），仅 Leviathan
    pub modulation_aura_radius_milli: i64,
    /// 解锁所需科技（`None` 为开局即可孵化）
    pub unlocked_by: Option<TechId>,
}

const fn weapon(damage_type: DamageType, damage_milli: i64, range_milli: i64, cooldown_ticks: u32) -> WeaponSpec {
    WeaponSpec::single(damage_type, damage_milli, range_milli, cooldown_ticks)
}

/// 六种单位平衡表（顺序与设计圣经一致）
pub const UNIT_SPECS: [UnitSpec; 6] = [
    UnitSpec {
        kind: UnitKind::Forager,
        name_zh: "采集工蜂簇",
        supply: 1,
        cost: cost(40, 0, 20),
        hatch_ticks: 90,
        hp_milli: 120 * MILLI,
        shield_milli: 0,
        armor: 1,
        radius_milli: 450,
        speed_milli_per_s: 4_500,
        vision_tiles: 8,
        weapon: Some(weapon(DamageType::Kinetic, 6 * MILLI, 1_200, 20)),
        harvest_milli_per_s: 4_000,
        support_aura: None,
        modulation_aura_radius_milli: 0,
        unlocked_by: None,
    },
    UnitSpec {
        kind: UnitKind::Striker,
        name_zh: "突击刺胞簇",
        supply: 2,
        cost: cost(65, 10, 30),
        hatch_ticks: 120,
        hp_milli: 200 * MILLI,
        shield_milli: 0,
        armor: 3,
        radius_milli: 550,
        speed_milli_per_s: 5_200,
        vision_tiles: 10,
        weapon: Some(weapon(DamageType::Kinetic, 16 * MILLI, 6_500, 12)),
        harvest_milli_per_s: 0,
        support_aura: None,
        modulation_aura_radius_milli: 0,
        unlocked_by: None,
    },
    UnitSpec {
        kind: UnitKind::Carapace,
        name_zh: "重甲盾簇",
        supply: 4,
        cost: cost(130, 25, 50),
        hatch_ticks: 180,
        hp_milli: 620 * MILLI,
        shield_milli: 200 * MILLI,
        armor: 12,
        radius_milli: 850,
        speed_milli_per_s: 2_800,
        vision_tiles: 9,
        // 前方 90° 扇形 AOE → 半角 45°
        weapon: Some(WeaponSpec {
            damage_type: DamageType::BioElectric,
            damage_milli: 24 * MILLI,
            range_milli: 3_000,
            cooldown_ticks: 30,
            arc_half_angle_deg: Some(45),
            hazard_deposit_milli: 0,
            flux_per_shot_milli: 0,
        }),
        harvest_milli_per_s: 0,
        support_aura: None,
        modulation_aura_radius_milli: 0,
        unlocked_by: Some(TechId::MulticellularChitin),
    },
    UnitSpec {
        kind: UnitKind::Spitter,
        name_zh: "腐蚀孢子簇",
        supply: 3,
        cost: cost(90, 30, 40),
        hatch_ticks: 150,
        hp_milli: 160 * MILLI,
        shield_milli: 0,
        armor: 2,
        radius_milli: 550,
        speed_milli_per_s: 3_600,
        vision_tiles: 12,
        // +沉积 15.0 Hazard
        weapon: Some(WeaponSpec {
            damage_type: DamageType::Corrosive,
            damage_milli: 22 * MILLI,
            range_milli: 11_000,
            cooldown_ticks: 30,
            arc_half_angle_deg: None,
            hazard_deposit_milli: 15 * MILLI,
            flux_per_shot_milli: 0,
        }),
        harvest_milli_per_s: 0,
        support_aura: None,
        modulation_aura_radius_milli: 0,
        unlocked_by: Some(TechId::MulticellularChitin),
    },
    UnitSpec {
        kind: UnitKind::Weaver,
        name_zh: "突触编织者",
        supply: 4,
        cost: cost(110, 55, 60),
        hatch_ticks: 180,
        hp_milli: 240 * MILLI,
        shield_milli: 80 * MILLI,
        armor: 4,
        radius_milli: 600,
        speed_milli_per_s: 4_000,
        vision_tiles: 11,
        weapon: None,
        harvest_milli_per_s: 0,
        support_aura: Some(SupportAura {
            radius_milli: 5_000,
            heal_milli_per_s: 12 * MILLI,
            sync_bonus_percent: 15,
        }),
        modulation_aura_radius_milli: 0,
        unlocked_by: Some(TechId::GapJunctionSync),
    },
    UnitSpec {
        kind: UnitKind::Leviathan,
        name_zh: "巨型神经母体",
        supply: 10,
        cost: cost(350, 140, 150),
        hatch_ticks: 360,
        hp_milli: 1500 * MILLI,
        shield_milli: 400 * MILLI,
        armor: 16,
        radius_milli: 1_400,
        speed_milli_per_s: 2_200,
        vision_tiles: 15,
        weapon: Some(weapon(DamageType::BioElectric, 75 * MILLI, 13_500, 45)),
        harvest_milli_per_s: 0,
        support_aura: None,
        modulation_aura_radius_milli: 8_000,
        unlocked_by: Some(TechId::MorphogenesisApex),
    },
];

/// 按设计圣经列序 `Bio / Gel / Flux` 构造造价
const fn cost(bio: i64, gel: i64, flux: i64) -> ResourceAmounts {
    ResourceAmounts {
        biomass: bio * MILLI,
        flux: flux * MILLI,
        neurogel: gel * MILLI,
    }
}

/// 查询单位平衡参数
pub const fn unit_spec(kind: UnitKind) -> &'static UnitSpec {
    &UNIT_SPECS[kind as usize]
}

/// `tech.chemotaxis_amp` 后 Forager 采集速率 `5.2 Bio/s`
pub const FORAGER_HARVEST_MILLI_PER_S_CHEMOTAXIS: i64 = 5_200;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_indexed_by_kind() {
        for kind in UnitKind::ALL {
            assert_eq!(unit_spec(*kind).kind, *kind);
        }
    }

    #[test]
    fn cost_columns_are_not_swapped() {
        // Weaver：110 Bio / 55 Gel / 60 Flux
        let w = unit_spec(UnitKind::Weaver).cost;
        assert_eq!((w.biomass, w.neurogel, w.flux), (110_000, 55_000, 60_000));
    }
}
