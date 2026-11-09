// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 单位感知、神经推理、运动与采集。
//!
//! **感知编码**（16 维 Q8.8，`±256 = ±1.0`，ERRATA E-025）：
//! - 场梯度（营养 / 信号 A）：中心差分 × 8 后钳制，Forager / Striker 研发趋化增敏后再 × 1.3；
//! - 场强：营养 / 危害 / 能量按 `[0, 100] → [0, 1]`，信号 B 按 `[-50, 50] → [-1, 1]`；
//! - 最近敌方 / 友方：视野内单位方向（单位向量）；`ENEMY_IN_RANGE` = 武器射程内为 1，否则按距离衰减；
//! - `HP_FRACTION` = 当前生命 / 最大生命；通道 14 / 15 由调质 DA / NE 注入（见 [`crate::brain`]）；
//! - 奇数号玩家阵营使用 180° 旋转参考系（见 [`mirrored_frame`]）。
//!
//! **期望速度**：`v_pref = normalize(2·d_order + a_move) · v_max`；无指令方向时 `v_pref = a_move · v_max`；
//! 驻守（HoldPosition）时为零。`v_max` 乘以所在地形的移速系数。

use std::collections::BTreeMap;

use rynvek_core::Q32_32;

use crate::genome::{GENOME_INPUTS, action, sensor};
use crate::ids::{EntitySerial, FactionId, FieldId, OrderKind, ResourceKind, TechId, UnitKind};
use crate::math::{Vec2, fractional_share, speed_per_tick};
use crate::modulators::{ModulatorSource, Modulators, received_at};
use crate::nav::orca::{OrcaAgent, compute_velocity};
use crate::terrain::speed_factor_milli;
use crate::units::unit_spec;

use super::{ArenaWorld, Order};

/// 邻居分桶边长（Tiles）
const BUCKET: i64 = 4;
/// ORCA 邻居搜索半径（Tiles）
const NEIGHBOR_RADIUS: i32 = 3;
/// 到达判定半径（milli Tile）
const ARRIVE_MILLI: i64 = 600;
/// 采集指令的作业半径（milli Tile）
const HARVEST_ZONE_MILLI: i64 = 2_500;

/// 感知中的二维向量通道（梯度与方向）
const VECTOR_CHANNELS: [usize; 8] = [
    sensor::NUTRIENT_GRAD_X,
    sensor::NUTRIENT_GRAD_Y,
    sensor::SIGNAL_A_GRAD_X,
    sensor::SIGNAL_A_GRAD_Y,
    sensor::ENEMY_DIR_X,
    sensor::ENEMY_DIR_Y,
    sensor::ALLY_DIR_X,
    sensor::ALLY_DIR_Y,
];

/// 阵营参考系：180° 中心对称地图上，奇数号玩家阵营的感知向量与运动读出整体取反，
/// 使双方单位脑看到的是同一个「以己方出生点为原点」的世界（PvP 绝对公平，ERRATA E-025）
pub const fn mirrored_frame(f: FactionId) -> bool {
    !f.is_wild() && f.0 % 2 == 1
}

/// 一个可被感知 / 攻击的目标快照
#[derive(Clone, Copy, Debug)]
pub(super) struct Contact {
    pub id: EntitySerial,
    pub faction: FactionId,
    pub position: Vec2,
    pub is_building: bool,
}

impl ArenaWorld {
    /// 全部单位与建筑的位置快照（按序号）
    pub(super) fn contacts(&self) -> Vec<Contact> {
        let mut out: Vec<Contact> = self
            .units
            .values()
            .map(|u| Contact {
                id: u.id,
                faction: u.faction,
                position: u.position,
                is_building: false,
            })
            .collect();
        out.extend(self.buildings.values().map(|b| Contact {
            id: b.id,
            faction: b.faction,
            position: b.center(),
            is_building: true,
        }));
        out
    }

    /// 某单位视野内最近的敌方 / 友方（友方只计单位）
    fn nearest(
        &self,
        me: EntitySerial,
        faction: FactionId,
        p: Vec2,
        vision: u32,
        contacts: &[Contact],
    ) -> (Option<Contact>, Option<Contact>) {
        let r2 = Q32_32::from_int(vision as i32).mul(Q32_32::from_int(vision as i32));
        let mut enemy: Option<(Q32_32, Contact)> = None;
        let mut ally: Option<(Q32_32, Contact)> = None;
        for c in contacts {
            if c.id == me {
                continue;
            }
            let target = if c.is_building {
                self.buildings[&c.id].nearest_point(p)
            } else {
                c.position
            };
            let d2 = p.distance_sq(target);
            if d2 > r2 {
                continue;
            }
            if Self::hostile(faction, c.faction) {
                if enemy.is_none_or(|(b, e)| (d2, c.id) < (b, e.id)) {
                    enemy = Some((d2, Contact { position: target, ..*c }));
                }
            } else if !c.is_building && ally.is_none_or(|(b, a)| (d2, c.id) < (b, a.id)) {
                ally = Some((d2, *c));
            }
        }
        (enemy.map(|e| e.1), ally.map(|a| a.1))
    }

    /// 调质广播源：已供电信标（作用半径）与 Leviathan 光环（转发其接收值）
    fn modulator_sources(&self, faction: FactionId) -> Vec<ModulatorSource> {
        let radius = self
            .faction(faction)
            .map(|f| f.modifiers().beacon_radius_tiles)
            .unwrap_or(16);
        let mut beacons: Vec<ModulatorSource> = self
            .buildings
            .values()
            .filter(|b| b.faction == faction && b.kind == crate::ids::BuildingKind::SignalBeacon && b.powered)
            .map(|b| ModulatorSource {
                position: b.center(),
                radius: Q32_32::from_int(radius as i32),
                modulators: b.beacon.modulators,
            })
            .collect();
        let relays: Vec<ModulatorSource> = self
            .units
            .values()
            .filter(|u| u.faction == faction && u.kind == UnitKind::Leviathan)
            .map(|u| ModulatorSource {
                position: u.position,
                radius: Q32_32::from_milli(unit_spec(UnitKind::Leviathan).modulation_aura_radius_milli),
                modulators: received_at(u.position, &beacons),
            })
            .collect();
        beacons.extend(relays);
        beacons
    }

    /// Coupling Epoch：感知 → 调质 → 脑推理
    pub(super) fn step_brains(&mut self) {
        let contacts = self.contacts();
        let mut sources: BTreeMap<FactionId, Vec<ModulatorSource>> = BTreeMap::new();
        let ids: Vec<EntitySerial> = self.units.keys().copied().collect();
        for id in ids {
            let u = &self.units[&id];
            let faction = u.faction;
            let srcs = sources
                .entry(faction)
                .or_insert_with(|| self.modulator_sources(faction))
                .clone();
            let m = received_at(u.position, &srcs);
            let x = self.sense(id, &contacts);
            let plasticity = self
                .faction(faction)
                .is_some_and(|f| f.techs.contains(TechId::HebbianPlasticity));
            let u = self.units.get_mut(&id).expect("存在");
            u.modulators = m;
            u.actions = u.brain.step_epoch(&x, m.0, plasticity);
        }
    }

    /// 组装一个单位的 16 维感知
    fn sense(&self, id: EntitySerial, contacts: &[Contact]) -> [i16; GENOME_INPUTS] {
        let u = &self.units[&id];
        let spec = unit_spec(u.kind);
        let (tx, ty) = u.tile();
        let mut x = [0i16; GENOME_INPUTS];
        let gain = match (u.kind, self.faction(u.faction)) {
            (UnitKind::Forager | UnitKind::Striker, Some(f)) => f.modifiers().chemotaxis_gain_permille,
            _ => 1000,
        };
        let grad = |field: FieldId| {
            let (gx, gy) = self.fields.gradient(field, tx, ty);
            let s = |g: i32| ((g as i64 * 8 * gain / 1000).clamp(-256, 256)) as i16;
            (s(gx), s(gy))
        };
        let level = |field: FieldId, full: i32| {
            (self.fields.get(field, tx as i64, ty as i64) as i32 * 256 / full).clamp(-256, 256) as i16
        };
        (x[sensor::NUTRIENT_GRAD_X], x[sensor::NUTRIENT_GRAD_Y]) = grad(FieldId::Nutrient);
        x[sensor::NUTRIENT] = level(FieldId::Nutrient, 25_600);
        x[sensor::HAZARD] = level(FieldId::Hazard, 25_600);
        (x[sensor::SIGNAL_A_GRAD_X], x[sensor::SIGNAL_A_GRAD_Y]) = grad(FieldId::SignalA);
        x[sensor::SIGNAL_B] = level(FieldId::SignalB, 12_800);
        x[sensor::ENERGY] = level(FieldId::Energy, 25_600);
        let (enemy, ally) = self.nearest(id, u.faction, u.position, spec.vision_tiles, contacts);
        let unit_dir = |p: Vec2| {
            let d = (p - u.position).normalized();
            (
                (d.x.0 >> 24).clamp(-256, 256) as i16,
                (d.y.0 >> 24).clamp(-256, 256) as i16,
            )
        };
        if let Some(e) = enemy {
            (x[sensor::ENEMY_DIR_X], x[sensor::ENEMY_DIR_Y]) = unit_dir(e.position);
            let dist = u.position.distance_sq(e.position).sqrt_deterministic();
            let range = Q32_32::from_milli(spec.weapon.map(|w| w.range_milli).unwrap_or(1_000));
            x[sensor::ENEMY_IN_RANGE] = if dist <= range {
                256
            } else {
                // 射程外按 range / dist 衰减
                (range.div_safe(dist, Q32_32::ZERO).0 >> 24).clamp(0, 255) as i16
            };
        }
        if let Some(a) = ally {
            (x[sensor::ALLY_DIR_X], x[sensor::ALLY_DIR_Y]) = unit_dir(a.position);
        }
        x[sensor::HP_FRACTION] = (u.defense.hp_milli * 256 / u.max_hp_milli.max(1)) as i16;
        if mirrored_frame(u.faction) {
            for ch in VECTOR_CHANNELS {
                x[ch] = -x[ch];
            }
        }
        x
    }

    /// 指令方向（单位向量，零 = 无方向）并处理到达状态转移
    fn order_direction(&mut self, id: EntitySerial, enemy: Option<Vec2>) -> Vec2 {
        let u = &self.units[&id];
        let order = u.order;
        let p = u.position;
        let to_target = order.target - p;
        let dist = to_target.length();
        let spec = unit_spec(u.kind);
        match order.kind {
            OrderKind::HoldPosition => return Vec2::ZERO,
            OrderKind::AttackMove => {
                if let (Some(e), Some(w)) = (enemy, spec.weapon) {
                    let d = (e - p).length();
                    let engage = Q32_32::from_milli(w.range_milli * 9 / 10);
                    return if d > engage { (e - p).normalized() } else { Vec2::ZERO };
                }
            }
            _ => {}
        }
        let arrive = Q32_32::from_milli(if order.kind == OrderKind::Harvest {
            HARVEST_ZONE_MILLI
        } else {
            ARRIVE_MILLI
        });
        if dist <= arrive {
            let u = self.units.get_mut(&id).expect("存在");
            match order.kind {
                OrderKind::Move => {
                    u.order.kind = OrderKind::HoldPosition;
                }
                OrderKind::Patrol => {
                    u.order = Order {
                        kind: OrderKind::Patrol,
                        target: order.origin,
                        origin: order.target,
                    };
                }
                _ => {}
            }
            return Vec2::ZERO;
        }
        let Some(goal) = order.target.tile() else {
            return Vec2::ZERO;
        };
        let here = self.units[&id].tile();
        let nav_dir = self.flow_direction(goal, here);
        if nav_dir == Vec2::ZERO {
            to_target.normalized()
        } else {
            nav_dir
        }
    }

    pub(super) fn step_movement(&mut self) {
        let contacts = self.contacts();
        let ids: Vec<EntitySerial> = self.units.keys().copied().collect();
        // 1) 期望速度
        let mut preferred: BTreeMap<EntitySerial, (Vec2, Q32_32)> = BTreeMap::new();
        for id in &ids {
            let u = &self.units[id];
            let spec = unit_spec(u.kind);
            let enemy = if u.order.kind == OrderKind::AttackMove {
                self.nearest(*id, u.faction, u.position, spec.vision_tiles, &contacts)
                    .0
                    .map(|c| c.position)
            } else {
                None
            };
            let sign = if mirrored_frame(u.faction) { -1 } else { 1 };
            let brain_move = Vec2::new(
                Q32_32((sign * u.actions[action::MOVE_X] as i64) << 24),
                Q32_32((sign * u.actions[action::MOVE_Y] as i64) << 24),
            )
            .clamp_length(Q32_32::ONE);
            let (tx, ty) = u.tile();
            let factor =
                speed_factor_milli(self.map.tile(tx.min(self.map.width - 1), ty.min(self.map.height - 1))).max(1);
            let vmax = Q32_32(speed_per_tick(spec.speed_milli_per_s).0 * factor / 1000);
            let dir = self.order_direction(*id, enemy);
            let pref = if self.units[id].order.kind == OrderKind::HoldPosition {
                Vec2::ZERO
            } else if dir == Vec2::ZERO {
                brain_move.scale(vmax)
            } else {
                (dir.scale(Q32_32::from_int(2)) + brain_move).normalized().scale(vmax)
            };
            preferred.insert(*id, (pref, vmax));
        }
        // 2) ORCA（基于上一 Tick 速度快照）
        let mut buckets: BTreeMap<(i64, i64), Vec<OrcaAgent>> = BTreeMap::new();
        for u in self.units.values() {
            let a = OrcaAgent {
                id: u.id.0,
                position: u.position,
                velocity: u.velocity,
                radius: u.radius(),
            };
            buckets
                .entry(((u.position.x.0 >> 32) / BUCKET, (u.position.y.0 >> 32) / BUCKET))
                .or_default()
                .push(a);
        }
        let reach = Q32_32::from_int(NEIGHBOR_RADIUS).mul(Q32_32::from_int(NEIGHBOR_RADIUS));
        let mut new_vel: Vec<(EntitySerial, Vec2)> = Vec::with_capacity(ids.len());
        for id in &ids {
            let u = &self.units[id];
            let me = OrcaAgent {
                id: u.id.0,
                position: u.position,
                velocity: u.velocity,
                radius: u.radius(),
            };
            let (bx, by) = ((u.position.x.0 >> 32) / BUCKET, (u.position.y.0 >> 32) / BUCKET);
            let mut neigh = Vec::new();
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(list) = buckets.get(&(bx + dx, by + dy)) {
                        neigh.extend(
                            list.iter()
                                .filter(|o| o.id != me.id && o.position.distance_sq(me.position) <= reach)
                                .copied(),
                        );
                    }
                }
            }
            let (pref, vmax) = preferred[id];
            let v = if neigh.is_empty() {
                pref
            } else {
                compute_velocity(&me, &neigh, pref, vmax)
            };
            new_vel.push((*id, v));
        }
        // 3) 积分 + 墙体滑移
        for (id, v) in new_vel {
            let u = &self.units[&id];
            let p = u.position;
            let try_pos = |q: Vec2| -> bool {
                let cur_ok = p.tile().is_some_and(|t| self.nav.passable(t.0 as i64, t.1 as i64));
                q.tile().is_some_and(|t| {
                    self.nav.passable(t.0 as i64, t.1 as i64) || !cur_ok && self.nav.in_bounds(t.0 as i64, t.1 as i64)
                })
            };
            let (pos, vel) = if try_pos(p + v) {
                (p + v, v)
            } else if try_pos(Vec2::new(p.x.saturating_add(v.x), p.y)) {
                (Vec2::new(p.x.saturating_add(v.x), p.y), Vec2::new(v.x, Q32_32::ZERO))
            } else if try_pos(Vec2::new(p.x, p.y.saturating_add(v.y))) {
                (Vec2::new(p.x, p.y.saturating_add(v.y)), Vec2::new(Q32_32::ZERO, v.y))
            } else {
                (p, Vec2::ZERO)
            };
            let u = self.units.get_mut(&id).expect("存在");
            u.position = pos;
            u.velocity = vel;
        }
    }

    /// Forager 采集：采集指令或脑采集读出为正时，从所在格抽取营养直接入账
    pub(super) fn step_harvest(&mut self) {
        let ids: Vec<EntitySerial> = self.units.keys().copied().collect();
        for id in ids {
            let u = &self.units[&id];
            let spec = unit_spec(u.kind);
            if spec.harvest_milli_per_s == 0 || u.faction.is_wild() {
                continue;
            }
            let wants = u.order.kind == OrderKind::Harvest || u.actions[action::HARVEST] > 64;
            if !wants || u.actions[action::HARVEST] < 0 {
                continue;
            }
            let rate = self
                .faction(u.faction)
                .map(|f| f.modifiers().forager_harvest_milli_per_s)
                .unwrap_or(spec.harvest_milli_per_s);
            // milli/s → Q8.8/Tick：rate × 256 / 1000 / 30
            let budget = fractional_share(rate * 256, 30_000, self.tick + id.0 as u64) as i32;
            let (tx, ty) = u.tile();
            let avail = self.fields.get(FieldId::Nutrient, tx as i64, ty as i64) as i32;
            let take = avail.min(budget);
            if take <= 0 {
                continue;
            }
            self.fields.add(FieldId::Nutrient, tx, ty, -take);
            let faction = u.faction;
            let u = self.units.get_mut(&id).expect("存在");
            let total = take as i64 * crate::fields::Q8_TO_MILLI_NUMER + u.harvest_remainder;
            u.harvest_remainder = total % crate::fields::Q8_TO_MILLI_DENOM;
            let milli = total / crate::fields::Q8_TO_MILLI_DENOM;
            if let Some(f) = self.faction_mut(faction) {
                let accepted = f.ledger.deposit(ResourceKind::Biomass, milli);
                f.ledger.biomass_harvested_total += accepted;
            }
        }
    }

    /// 某阵营收到的调质（观测用）
    pub fn modulators_at(&self, faction: FactionId, p: Vec2) -> Modulators {
        received_at(p, &self.modulator_sources(faction))
    }
}
