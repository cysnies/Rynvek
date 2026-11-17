// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 战斗、环境伤害、护盾再生 / 治疗与死亡结算（含 `ContactGrid` $O(N)$ 索敌与光环预缓存优化，审计报告 §10）。
//!
//! 设计圣经只冻结了武器数值与伤害克制，以下为确定性实现（ERRATA E-032）：
//! - **索敌**：射程内（到建筑取占地最近点）、直线弹道不被墙体阻挡的最近敌方，按 `(距离², 序号)` 取；
//! - **开火门控**：单位脑攻击读出 `< −0.5` 时不开火（5-HT 抑制）；`field.signal_b` 正值按
//!   `+20% × S_B / 50` 提升射速，老兵（`tech.hebbian_plasticity` 且存活 > 900 Tick）再 `+20%`；
//! - **命中**：闪避率 = 老兵 20% + 目标处于友方 Weaver 同步光环 15% − 攻击者处于 Weaver 光环 15%，
//!   钳制到 `[0, 90%]`，以 Philox（流 `0x0603`，`entity = 攻击者`，`sub_counter = 目标`）判定；
//! - **Carapace**：对主目标方向 ±45° 扇形、射程内全部敌方单位造成伤害；
//! - **Spitter**：命中格沉积 `15.0 field.hazard`；
//! - **防御塔** 每发消耗 `4 Flux`，余额不足不开火；主脑自带 Kinetic 20 / 射程 10 / 冷却 15；
//! - **环境**：危害场对非腐蚀抗性单位（Spitter 自身免疫）每 Tick `0.08 × F` 真实伤害；
//!   裂隙每 Tick `1.5` 灼烧；能量场 `≥ 20` 处护盾再生 `+25%`（设计圣经中「内部能量」的映射）；
//! - **护盾再生**：每秒最大护盾 2% × (1 + 防御读出)；
//! - **死亡**：单位残骸按 75%（研发超导代谢后 90%）生物质造价化为死亡格营养。

use super::ecology::ActivityKind;
use rynvek_core::{Q32_32, RandomKey};

use crate::buildings::{NEXUS_WEAPON, TURRET_WEAPON};
use crate::combat::{WeaponSpec, apply_damage, apply_true_damage, hazard_damage_milli_per_tick};
use crate::genome::action;
use crate::ids::{BuildingKind, DamageType, EntitySerial, FactionId, FieldId, ResourceKind, UnitKind};
use crate::map_gen::TerrainTileKind;
use crate::math::{COS_45, Vec2, per_tick_share};
use crate::terrain::{FISSURE_BURN_MILLI_PER_TICK, blocks_line_of_sight};
use crate::units::unit_spec;

use super::{ArenaWorld, RtsEvent, SUDDEN_DEATH_NEXUS_LOSS_MILLI_PER_S};

/// 闪避判定随机流
pub const COMBAT_RNG_STREAM: u16 = 0x0603;
/// 闪避上限
pub const MAX_DODGE_PERCENT: i64 = 90;
/// 护盾每秒再生（最大护盾的百分比）
pub const SHIELD_REGEN_PERCENT_PER_S: i64 = 2;

/// 一名射手的快照
struct Shooter {
    id: EntitySerial,
    faction: FactionId,
    position: Vec2,
    weapon: WeaponSpec,
    is_building: bool,
}

impl ArenaWorld {
    /// Bresenham 直线弹道是否被墙体阻挡（端点格不计）
    pub fn line_of_fire_clear(&self, a: Vec2, b: Vec2) -> bool {
        let (Some((x0, y0)), Some((x1, y1))) = (a.tile(), b.tile()) else {
            return false;
        };
        let (mut x, mut y) = (x0 as i64, y0 as i64);
        let (dx, dy) = ((x1 as i64 - x).abs(), -(y1 as i64 - y).abs());
        let (sx, sy) = (if x < x1 as i64 { 1 } else { -1 }, if y < y1 as i64 { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            if (x, y) == (x1 as i64, y1 as i64) {
                return true;
            }
            if (x, y) != (x0 as i64, y0 as i64) && blocks_line_of_sight(self.map.tile(x as u32, y as u32)) {
                return false;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// 单位所在格是否处于本阵营 Weaver 同步光环内
    fn in_weaver_aura(&self, faction: FactionId, p: Vec2) -> bool {
        let aura = unit_spec(UnitKind::Weaver).support_aura.expect("Weaver 光环");
        let r = Q32_32::from_milli(aura.radius_milli);
        let r2 = r.mul(r);
        self.units.values().any(|w| {
            w.faction == faction
                && w.kind == UnitKind::Weaver
                && self.can_perform_activity(w.id)
                && w.position.distance_sq(p) <= r2
        })
    }

    fn is_veteran(&self, id: EntitySerial) -> bool {
        let Some(u) = self.units.get(&id) else { return false };
        self.faction(u.faction)
            .and_then(|f| f.modifiers().veteran_threshold_ticks)
            .is_some_and(|t| self.tick.saturating_sub(u.born_tick) > t)
    }

    /// 阵营对武器的科技修正（Striker 与防御塔的 Kinetic 武器）
    fn modified_weapon(
        &self,
        faction: FactionId,
        base: WeaponSpec,
        kind: Option<UnitKind>,
        building: Option<BuildingKind>,
    ) -> WeaponSpec {
        let mut w = base;
        if let Some(f) = self.faction(faction) {
            let m = f.modifiers();
            let eligible = kind == Some(UnitKind::Striker) || building == Some(BuildingKind::PulseTurret);
            if eligible && w.damage_type == DamageType::Kinetic {
                w.damage_milli = w.damage_milli * m.kinetic_damage_permille / 1000;
                w.range_milli += m.kinetic_range_bonus_milli;
            }
        }
        w
    }

    fn shooters(&self) -> Vec<Shooter> {
        let mut out = Vec::new();
        for u in self.units.values() {
            let Some(w) = unit_spec(u.kind).weapon else { continue };
            if !self.can_perform_activity(u.id) || u.weapon_cooldown > 0 || u.actions[action::ATTACK] < -128 {
                continue;
            }
            out.push(Shooter {
                id: u.id,
                faction: u.faction,
                position: u.position,
                weapon: self.modified_weapon(u.faction, w, Some(u.kind), None),
                is_building: false,
            });
        }
        for b in self.buildings.values() {
            let base = match b.kind {
                BuildingKind::NexusCore => NEXUS_WEAPON,
                BuildingKind::PulseTurret => TURRET_WEAPON,
                _ => continue,
            };
            if b.defense.hp_milli <= 0 || !b.complete || !b.powered || b.weapon_cooldown > 0 {
                continue;
            }
            out.push(Shooter {
                id: b.id,
                faction: b.faction,
                position: b.center(),
                weapon: self.modified_weapon(b.faction, base, None, Some(b.kind)),
                is_building: true,
            });
        }
        out.sort_by_key(|s| s.id);
        out
    }

    pub(super) fn step_combat(&mut self) {
        for u in self.units.values_mut() {
            u.weapon_cooldown = u.weapon_cooldown.saturating_sub(1);
        }
        for b in self.buildings.values_mut() {
            b.weapon_cooldown = b.weapon_cooldown.saturating_sub(1);
        }
        let grid = self.build_contact_grid();
        for s in self.shooters() {
            // 射手可能已在本 Tick 早些时候阵亡
            if !(self.can_perform_activity(s.id) || self.buildings.get(&s.id).is_some_and(|b| b.defense.hp_milli > 0)) {
                continue;
            }
            let range = Q32_32::from_milli(s.weapon.range_milli);
            // 考虑最大单位半径（Leviathan 1.4 Tile），在空间分桶中查询 `ceil(range) + 2` Tile 邻域
            let search_radius_tiles = (s.weapon.range_milli + 2_999) / 1_000;
            let mut best: Option<(Q32_32, EntitySerial, Vec2, bool)> = None;

            grid.for_each_unit_in_radius(s.position, search_radius_tiles, |c| {
                if !Self::hostile(s.faction, c.faction) {
                    return;
                }
                let Some(u) = self.units.get(&c.id) else {
                    return;
                };
                if u.defense.hp_milli <= 0 {
                    return;
                }
                let (point, extra) = (u.position, u.radius());
                let reach = range.saturating_add(extra);
                let d2 = s.position.distance_sq(point);
                if d2 > reach.mul(reach) {
                    return;
                }
                if best.is_some_and(|(bd, bid, _, _)| (bd, bid) <= (d2, c.id)) {
                    return;
                }
                if !self.line_of_fire_clear(s.position, point) {
                    return;
                }
                best = Some((d2, c.id, point, false));
            });

            for c in &grid.buildings {
                if !Self::hostile(s.faction, c.faction) {
                    continue;
                }
                let Some(b) = self.buildings.get(&c.id) else {
                    continue;
                };
                if b.defense.hp_milli <= 0 {
                    continue;
                }
                let point = b.nearest_point(s.position);
                let d2 = s.position.distance_sq(point);
                if d2 > range.mul(range) {
                    continue;
                }
                if best.is_some_and(|(bd, bid, _, _)| (bd, bid) <= (d2, c.id)) {
                    continue;
                }
                if !self.line_of_fire_clear(s.position, point) {
                    continue;
                }
                best = Some((d2, c.id, point, true));
            }

            let Some((_, target, point, _)) = best else { continue };
            // 同一权威调用内预检身体费用，再扣弹药；两者均可支付才发射。
            let attack_q = if s.is_building {
                0
            } else {
                self.activity_price_q(s.id, ActivityKind::Attack)
            };
            if !s.is_building && self.affordable_activity(s.id, 1, attack_q) == 0 {
                continue;
            }
            // 防御塔弹药
            if s.weapon.flux_per_shot_milli > 0 {
                let paid = self
                    .faction_mut(s.faction)
                    .is_some_and(|f| f.ledger.try_consume(ResourceKind::Flux, s.weapon.flux_per_shot_milli));
                if !paid {
                    continue;
                }
            }
            assert!(self.charge_activity(s.id, ActivityKind::Attack, attack_q));
            // 射速
            let mut bonus = 0i64;
            if !s.is_building {
                let (tx, ty) = s.position.tile().unwrap_or((0, 0));
                let sb = self.fields.get(FieldId::SignalB, tx as i64, ty as i64).max(0) as i64;
                bonus += 20 * sb / 12_800;
                if self.is_veteran(s.id) {
                    bonus += 20;
                }
            }
            let cooldown = (s.weapon.cooldown_ticks as i64 * 100 / (100 + bonus)).max(1) as u32;
            if s.is_building {
                self.buildings.get_mut(&s.id).expect("存在").weapon_cooldown = cooldown;
            } else {
                self.units.get_mut(&s.id).expect("存在").weapon_cooldown = cooldown;
            }
            // 目标集合：Carapace 扇形 AOE，否则单体
            let mut victims = vec![target];
            if let Some(half) = s.weapon.arc_half_angle_deg {
                debug_assert_eq!(half, 45);
                let axis = (point - s.position).normalized();
                let reach2 = range.mul(range);
                let mut arc_hits = Vec::new();
                grid.for_each_unit_in_radius(s.position, search_radius_tiles, |c| {
                    if c.id == target || !Self::hostile(s.faction, c.faction) {
                        return;
                    }
                    let Some(u) = self.units.get(&c.id) else {
                        return;
                    };
                    let rel = u.position - s.position;
                    if rel.length_sq() > reach2 {
                        return;
                    }
                    if rel.normalized().dot(axis) >= COS_45 {
                        arc_hits.push(u.id);
                    }
                });
                arc_hits.sort_unstable();
                victims.extend(arc_hits);
            }
            for v in victims {
                self.resolve_hit(&s, v);
            }
            if s.weapon.hazard_deposit_milli > 0 {
                if let Some((tx, ty)) = point
                    .tile()
                    .filter(|(x, y)| *x < self.map.width && *y < self.map.height)
                {
                    // 只为实际可容纳的整数Q8.8分泌付费；饱和不收费，缺钱降量且保留余款。
                    let limit =
                        i32::from(crate::fields::ARENA_FIELD_CHANNELS[FieldId::Hazard.channel().index()].max_val_q8);
                    let room =
                        (limit - i32::from(self.fields.get(FieldId::Hazard, tx as i64, ty as i64))).max(0) as u64;
                    let requested = (s.weapon.hazard_deposit_milli * 256 / 1000).max(0) as u64;
                    let price = self.activity_price_q(s.id, ActivityKind::Secretion);
                    let accepted = self.affordable_activity(s.id, requested.min(room), price);
                    assert!(self.charge_activity(s.id, ActivityKind::Secretion, accepted * price));
                    assert_eq!(
                        self.fields.add(FieldId::Hazard, tx, ty, accepted as i32),
                        accepted as i32
                    );
                }
            }
        }
    }

    fn resolve_hit(&mut self, s: &Shooter, target: EntitySerial) {
        if self.units.get(&target).is_some_and(|u| u.defense.hp_milli <= 0)
            || self.buildings.get(&target).is_some_and(|b| b.defense.hp_milli <= 0)
        {
            return;
        }
        // 闪避（只对单位目标）
        if let Some(t) = self.units.get(&target) {
            let mut dodge = 0i64;
            if self.is_veteran(target) {
                dodge += 20;
            }
            let aura = unit_spec(UnitKind::Weaver)
                .support_aura
                .expect("光环")
                .sync_bonus_percent;
            if self.in_weaver_aura(t.faction, t.position) {
                dodge += aura;
            }
            if !s.is_building && self.in_weaver_aura(s.faction, s.position) {
                dodge -= aura;
            }
            let dodge = dodge.clamp(0, MAX_DODGE_PERCENT);
            if dodge > 0 {
                let r = RandomKey {
                    world_seed: self.config.world_seed,
                    tick: self.tick,
                    entity_serial: s.id.0 as u64,
                    stream_and_phase: RandomKey::pack_stream_and_phase(COMBAT_RNG_STREAM, 0),
                    sub_counter: target.0,
                }
                .generate_u32x4();
                if (r[0] as u64 * 100) >> 32 < dodge as u64 {
                    return;
                }
            }
        }
        let dealt = if let Some(u) = self.units.get_mut(&target) {
            apply_damage(&mut u.defense, s.weapon.damage_type, s.weapon.damage_milli).total()
        } else if let Some(b) = self.buildings.get_mut(&target) {
            apply_damage(&mut b.defense, s.weapon.damage_type, s.weapon.damage_milli).total()
        } else {
            0
        };
        if let Some(u) = self.units.get_mut(&s.id) {
            u.fitness.damage_dealt_milli += dealt;
        }
    }

    pub(super) fn step_environment(&mut self) {
        let tick = self.tick;
        let ids: Vec<EntitySerial> = self.units.keys().copied().collect();
        // 先结算所有环境伤害，死亡/耗尽的Weaver不能在同一环境阶段继续治疗。
        for &id in &ids {
            let (tx, ty) = self.units[&id].tile();
            let tile = self.map.tile(tx.min(self.map.width - 1), ty.min(self.map.height - 1));
            let hazard = self.fields.get(FieldId::Hazard, tx as i64, ty as i64);
            let u = self.units.get_mut(&id).expect("存在");
            let resistant = unit_spec(u.kind)
                .weapon
                .is_some_and(|w| w.damage_type == DamageType::Corrosive);
            if !resistant {
                apply_true_damage(&mut u.defense, hazard_damage_milli_per_tick(hazard));
            }
            if tile == TerrainTileKind::AcidicFissure {
                apply_true_damage(&mut u.defense, FISSURE_BURN_MILLI_PER_TICK);
            }
        }
        let weavers: Vec<_> = self
            .units
            .values()
            .filter(|u| u.kind == UnitKind::Weaver)
            .map(|u| u.id)
            .collect();
        let aura = unit_spec(UnitKind::Weaver).support_aura.expect("光环");
        let ar = Q32_32::from_milli(aura.radius_milli);
        let heal = per_tick_share(aura.heal_milli_per_s, tick);
        for id in ids {
            if !self.can_perform_activity(id) {
                continue;
            }
            let u = &self.units[&id];
            let (tx, ty) = u.tile();
            let energy = self.fields.get(FieldId::Energy, tx as i64, ty as i64);
            if u.max_shield_milli > 0 {
                let defense = u.actions[action::DEFENSE].clamp(0, 256) as i64;
                let mut rate = u.max_shield_milli * SHIELD_REGEN_PERCENT_PER_S / 100 * (256 + defense) / 256;
                if energy >= 20 * 256 {
                    rate = rate * 125 / 100;
                }
                let requested =
                    per_tick_share(rate, tick).min((u.max_shield_milli - u.defense.shield_milli).max(0)) as u64;
                let price = self.activity_price_q(id, ActivityKind::Repair);
                let actual = self.affordable_activity(id, requested, price);
                assert!(self.charge_activity(id, ActivityKind::Repair, actual * price));
                self.units.get_mut(&id).expect("存在").defense.shield_milli += actual as i64;
            }
            if !self.can_perform_activity(id) || self.units[&id].kind == UnitKind::Weaver {
                continue;
            }
            let u = &self.units[&id];
            let (faction, position) = (u.faction, u.position);
            let mut remaining = heal.min((u.max_hp_milli - u.defense.hp_milli).max(0)) as u64;
            for &donor in &weavers {
                if remaining == 0 {
                    break;
                }
                let w = &self.units[&donor];
                if w.faction != faction || w.position.distance_sq(position) > ar.mul(ar) {
                    continue;
                }
                let price = self.activity_price_q(donor, ActivityKind::Repair);
                let actual = self.affordable_activity(donor, remaining, price);
                assert!(self.charge_activity(donor, ActivityKind::Repair, actual * price));
                self.units.get_mut(&id).expect("真实受体").defense.hp_milli += actual as i64;
                remaining -= actual;
            }
        }
        // 突然死亡：主脑每秒流失 20 HP
        if self.sudden_death_until.is_some() {
            let loss = per_tick_share(SUDDEN_DEATH_NEXUS_LOSS_MILLI_PER_S, tick);
            let nexuses: Vec<EntitySerial> = self.factions.iter().filter_map(|f| f.nexus).collect();
            for id in nexuses {
                if let Some(b) = self.buildings.get_mut(&id) {
                    apply_true_damage(&mut b.defense, loss);
                }
            }
        }
    }

    /// 战斗死亡与显式代谢死亡共用唯一退役/遗骸/事件路径。
    pub(super) fn retire_dead_unit(&mut self, id: EntitySerial) {
        let u = self.units.remove(&id).expect("存在");
        let recycle = self
            .faction(u.faction)
            .map(|f| f.modifiers().corpse_recycle_percent)
            .unwrap_or(75);
        if let Some(ecology) = &mut self.ecology {
            let body = ecology.bodies.remove(&id).expect("死亡前存在个体账本");
            let mut account = body.account;
            account.prepare_remains(recycle as u32);
            let (x, y) = u.tile();
            ecology.remains.insert(
                id,
                super::ecology::RemainsEnergy {
                    cell: (x.min(self.map.width - 1), y.min(self.map.height - 1)),
                    account,
                    activity: body.activity,
                },
            );
        }
        let nutrient_milli = unit_spec(u.kind).cost.biomass * recycle / 100;
        if let Some((tx, ty)) = u.position.tile().filter(|_| self.ecology.is_none()) {
            let (tx, ty) = (tx.min(self.map.width - 1), ty.min(self.map.height - 1));
            self.fields
                .add(FieldId::Nutrient, tx, ty, (nutrient_milli * 256 / 1000) as i32);
        }
        if u.faction.is_wild() {
            let mut fit = u.fitness;
            fit.survival_ticks = self.tick - u.born_tick;
            self.pve_records.push((u.id.0, u.genome.clone(), fit));
        }
        self.events.push(RtsEvent::UnitDied {
            id,
            faction: u.faction,
            kind: u.kind,
        });
    }

    pub(super) fn step_deaths(&mut self) {
        let dead_units: Vec<EntitySerial> = self
            .units
            .values()
            .filter(|u| u.defense.hp_milli <= 0)
            .map(|u| u.id)
            .collect();
        for id in dead_units {
            self.retire_dead_unit(id);
        }
        let dead_buildings: Vec<EntitySerial> = self
            .buildings
            .values()
            .filter(|b| b.defense.hp_milli <= 0)
            .map(|b| b.id)
            .collect();
        for id in dead_buildings {
            self.remove_building(id, true);
        }
    }
}
