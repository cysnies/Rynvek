// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 确定性战斗伤害与护甲克制公式（D261–D266）。
//!
//! 设计圣经给出了每种武器的伤害类型、单发伤害、射程与冷却，但没有给出伤害类型与护盾 / 护甲之间的
//! 克制系数。以下按 RTS 最佳实践补全（ERRATA E-024），全部为整数毫单位运算：
//!
//! 1. **护盾先吸收**：对护盾的有效伤害 = 原始伤害 × 护盾系数（Kinetic 100%、BioElectric 150%、
//!    Corrosive 50%）；护盾耗尽后剩余部分按「护盾系数的倒数」换算回原始伤害继续结算。
//! 2. **护甲减免**：对生命值的伤害 = `max(原始伤害 − 护甲 × 护甲系数, 原始伤害 × 10%)`，
//!    护甲系数 Kinetic / BioElectric 为 1.0，Corrosive 为 0.5（腐蚀穿甲）；10% 保底防止高甲单位免疫。
//! 3. **真实伤害**（环境危害场、裂隙灼烧）绕过护盾与护甲。

use serde::{Deserialize, Serialize};

use crate::ids::DamageType;
use crate::math::MILLI;

/// 伤害对护盾的系数（百分比）
pub const fn shield_multiplier_percent(t: DamageType) -> i64 {
    match t {
        DamageType::Kinetic => 100,
        DamageType::BioElectric => 150,
        DamageType::Corrosive => 50,
    }
}

/// 护甲对该伤害类型的生效比例（百分比）
pub const fn armor_effectiveness_percent(t: DamageType) -> i64 {
    match t {
        DamageType::Kinetic | DamageType::BioElectric => 100,
        DamageType::Corrosive => 50,
    }
}

/// 护甲减免后的保底伤害比例（百分比）
pub const MIN_DAMAGE_PERCENT: i64 = 10;

/// 武器参数（毫单位）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponSpec {
    pub damage_type: DamageType,
    /// 单发伤害（毫单位）
    pub damage_milli: i64,
    /// 射程（毫 Tile）
    pub range_milli: i64,
    /// 冷却（Ticks）
    pub cooldown_ticks: u32,
    /// 前方扇形 AOE 的半角（度）；`None` 为单体
    pub arc_half_angle_deg: Option<u32>,
    /// 命中时在目标 Tile 沉积的 `field.hazard`（毫单位）
    pub hazard_deposit_milli: i64,
    /// 单发消耗的 Flux（毫单位）
    pub flux_per_shot_milli: i64,
}

impl WeaponSpec {
    /// 单体武器
    pub const fn single(damage_type: DamageType, damage_milli: i64, range_milli: i64, cooldown_ticks: u32) -> Self {
        Self {
            damage_type,
            damage_milli,
            range_milli,
            cooldown_ticks,
            arc_half_angle_deg: None,
            hazard_deposit_milli: 0,
            flux_per_shot_milli: 0,
        }
    }
}

/// 可受伤目标的防御属性（毫单位，护甲为整数点数）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defense {
    pub hp_milli: i64,
    pub shield_milli: i64,
    pub armor: i64,
}

/// 一次伤害结算的结果
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageOutcome {
    pub shield_damage_milli: i64,
    pub hp_damage_milli: i64,
}

impl DamageOutcome {
    pub const fn total(&self) -> i64 {
        self.shield_damage_milli + self.hp_damage_milli
    }
}

/// 按克制公式结算一次命中，原地更新 `target`
pub fn apply_damage(target: &mut Defense, damage_type: DamageType, raw_milli: i64) -> DamageOutcome {
    let mut out = DamageOutcome::default();
    if raw_milli <= 0 || target.hp_milli <= 0 {
        return out;
    }
    let mult = shield_multiplier_percent(damage_type);
    let mut remaining_raw = raw_milli;
    if target.shield_milli > 0 {
        let vs_shield = raw_milli * mult / 100;
        if vs_shield <= target.shield_milli {
            target.shield_milli -= vs_shield;
            out.shield_damage_milli = vs_shield;
            return out;
        }
        out.shield_damage_milli = target.shield_milli;
        // 护盾吸收部分折算回原始伤害后扣除
        let absorbed_raw = target.shield_milli * 100 / mult;
        target.shield_milli = 0;
        remaining_raw = (raw_milli - absorbed_raw).max(0);
    }
    if remaining_raw == 0 {
        return out;
    }
    let armor_milli = target.armor * MILLI * armor_effectiveness_percent(damage_type) / 100;
    let floor = remaining_raw * MIN_DAMAGE_PERCENT / 100;
    let hp_damage = (remaining_raw - armor_milli).max(floor).min(target.hp_milli);
    target.hp_milli -= hp_damage;
    out.hp_damage_milli = hp_damage;
    out
}

/// 真实伤害：绕过护盾与护甲
pub fn apply_true_damage(target: &mut Defense, raw_milli: i64) -> i64 {
    let d = raw_milli.clamp(0, target.hp_milli.max(0));
    target.hp_milli -= d;
    d
}

/// 危害场每 Tick 的真实伤害：`0.08 × F_hazard`（`F` 为 Q8.8 原始值 / 256），返回毫单位
///
/// `0.08 × (raw / 256) × 1000 = raw × 80 / 256 = raw × 5 / 16`（整数向下取整）。
pub const fn hazard_damage_milli_per_tick(hazard_q8_raw: i16) -> i64 {
    if hazard_q8_raw <= 0 {
        return 0;
    }
    (hazard_q8_raw as i64) * 5 / 16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(hp: i64, shield: i64, armor: i64) -> Defense {
        Defense {
            hp_milli: hp * MILLI,
            shield_milli: shield * MILLI,
            armor,
        }
    }

    #[test]
    fn kinetic_vs_armor_subtracts_armor() {
        // Striker 16 Kinetic 打 Striker（护甲 3）→ 13
        let mut t = def(200, 0, 3);
        let o = apply_damage(&mut t, DamageType::Kinetic, 16_000);
        assert_eq!(o.hp_damage_milli, 13_000);
        assert_eq!(t.hp_milli, 187_000);
    }

    #[test]
    fn minimum_damage_floor_applies_against_heavy_armor() {
        // Forager 6 Kinetic 打 Leviathan 本体（护甲 16）→ 保底 10% = 0.6
        let mut t = def(1500, 0, 16);
        assert_eq!(apply_damage(&mut t, DamageType::Kinetic, 6_000).hp_damage_milli, 600);
    }

    #[test]
    fn bioelectric_overloads_shields_and_spills_over() {
        // Carapace 护盾 200：Leviathan 75 BioElectric → 护盾受 112.5
        let mut t = def(620, 200, 12);
        let o = apply_damage(&mut t, DamageType::BioElectric, 75_000);
        assert_eq!(o.shield_damage_milli, 112_500);
        assert_eq!(t.shield_milli, 87_500);
        // 再打两发：第二发护盾剩 87.5 只吸收 58.333 原始伤害，余 16.667 − 护甲 12 = 4.667
        let o2 = apply_damage(&mut t, DamageType::BioElectric, 75_000);
        assert_eq!(o2.shield_damage_milli, 87_500);
        assert_eq!(o2.hp_damage_milli, 75_000 - 58_333 - 12_000);
        assert_eq!(t.shield_milli, 0);
    }

    #[test]
    fn corrosive_halves_armor_and_is_weak_vs_shields() {
        let mut t = def(160, 0, 8);
        assert_eq!(
            apply_damage(&mut t, DamageType::Corrosive, 22_000).hp_damage_milli,
            18_000
        );
        let mut s = def(240, 80, 4);
        assert_eq!(
            apply_damage(&mut s, DamageType::Corrosive, 22_000).shield_damage_milli,
            11_000
        );
    }

    #[test]
    fn hazard_true_damage_matches_formula() {
        // F = 100.0 → raw 25600 → 8.0 HP/Tick
        assert_eq!(hazard_damage_milli_per_tick(25_600), 8_000);
        assert_eq!(hazard_damage_milli_per_tick(256 * 15), 1_200);
        assert_eq!(hazard_damage_milli_per_tick(-3), 0);
        let mut t = def(10, 50, 99);
        assert_eq!(apply_true_damage(&mut t, 12_000), 10_000);
        assert_eq!(t.shield_milli, 50_000);
    }
}
