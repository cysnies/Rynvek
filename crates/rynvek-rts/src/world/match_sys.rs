// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 迷雾刷新、PvE 虫潮调度与胜负裁决（项目文档 §5.5.2 / §5.5.3）。
//!
//! **超时积分**（`Tick == 36,000`）：
//! `S = 1000 · HP/HP_max + 2.0 · ΣCost(b) + 1.5 · ΣCost(u) + 0.25 · Biomass_harvested_total`，
//! 其中 `Cost` 取设计圣经造价三资源之和（只计已完工建筑与存活单位，主脑造价为 0），全部以 milli 定点计算。
//! 分差 `< 1%`（相对较高分）时进入 `3,600 Ticks` 突然死亡，期间主脑每秒流失 `20 HP`；
//! 加赛中主脑被毁按歼灭判负，加赛结束再按积分裁决，仍相等则平局（ERRATA E-033）。

use std::collections::BTreeSet;

use crate::buildings::{BEACON_DETECTION_RADIUS_TILES, building_spec};
use crate::fog::BuildingGhost;
use crate::ids::{BuildingKind, EntitySerial, FactionId, OrderKind};
use crate::map_gen::TerrainTileKind;
use crate::math::Vec2;
use crate::pve::{PVE_MAX_WAVES, PVE_WAVE_INTERVAL_TICKS};
use crate::units::unit_spec;

use super::{
    ArenaWorld, MATCH_TIME_LIMIT_TICKS, MatchMode, MatchOutcome, Order, RtsEvent, SUDDEN_DEATH_TICKS, VictoryReason,
};

impl ArenaWorld {
    /// 重算全部玩家阵营的视野、已探索与建筑幽灵
    pub fn update_fog(&mut self) {
        let map = &self.map;
        let blocks = |x: i64, y: i64| map.tile(x as u32, y as u32) == TerrainTileKind::ImperviousWall;
        for fi in 0..self.factions.len() {
            let fid = self.factions[fi].id;
            let mut sources: Vec<((u32, u32), u32)> = Vec::new();
            for u in self.units.values().filter(|u| u.faction == fid) {
                sources.push((u.tile(), unit_spec(u.kind).vision_tiles));
            }
            for b in self.buildings.values().filter(|b| b.faction == fid) {
                let mut r = building_spec(b.kind).vision_tiles;
                if b.kind == BuildingKind::SignalBeacon && b.complete {
                    r = r.max(BEACON_DETECTION_RADIUS_TILES);
                }
                sources.push((b.center_tile(), r));
            }
            let fog = &mut self.factions[fi].fog;
            fog.begin_frame();
            for (origin, r) in sources {
                fog.reveal_from(origin, r, blocks);
            }
            // 幽灵：可见的敌方建筑刷新快照；可见区域内已消失的建筑删除幽灵
            let mut seen = BTreeSet::new();
            let mut updates = Vec::new();
            for b in self.buildings.values().filter(|b| Self::hostile(fid, b.faction)) {
                let r = b.rect();
                if fog.any_visible(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0) {
                    seen.insert(b.id);
                    updates.push(BuildingGhost {
                        entity_id: b.id,
                        faction: b.faction,
                        kind: b.kind,
                        origin: b.origin,
                        last_seen_tick: self.tick,
                    });
                }
            }
            let stale: Vec<EntitySerial> = self.factions[fi]
                .ghosts
                .values()
                .filter(|g| {
                    let f = building_spec(g.kind).footprint;
                    !self.buildings.contains_key(&g.entity_id)
                        && self.factions[fi].fog.any_visible(g.origin.0, g.origin.1, f, f)
                })
                .map(|g| g.entity_id)
                .collect();
            let ghosts = &mut self.factions[fi].ghosts;
            for id in stale {
                ghosts.remove(&id);
            }
            for g in updates {
                ghosts.insert(g.entity_id, g);
            }
        }
    }

    /// PvE：到点刷新下一波；刷新前用上一波全部个体的适应度繁育新基因池
    pub(super) fn step_pve(&mut self) {
        let Some(director) = self.pve.as_ref() else { return };
        if self.tick % PVE_WAVE_INTERVAL_TICKS != 0 || director.wave >= PVE_MAX_WAVES {
            return;
        }
        let wave = director.wave + 1;
        if wave > 1 {
            // 存活个体也计入上一波适应度
            let alive: Vec<(u32, crate::genome::Genome, crate::evolution::FitnessRecord)> = self
                .units
                .values()
                .filter(|u| u.faction.is_wild())
                .map(|u| {
                    let mut f = u.fitness;
                    f.survival_ticks = self.tick - u.born_tick;
                    (u.id.0, u.genome.clone(), f)
                })
                .collect();
            let mut records = std::mem::take(&mut self.pve_records);
            records.extend(alive);
            records.sort_by_key(|r| r.0);
            self.pve.as_mut().expect("PvE").evolve(&records, self.tick);
        }
        let director = self.pve.as_mut().expect("PvE");
        director.wave = wave;
        let plan = director.plan_wave(wave);
        let pool = director.pool.clone();
        let rifts = self.map.edge_rift_anchors();
        let nexuses: Vec<Vec2> = self
            .factions
            .iter()
            .filter_map(|f| f.nexus)
            .filter_map(|id| self.buildings.get(&id))
            .map(|b| b.center())
            .collect();
        for (k, idx) in plan.iter().enumerate() {
            let rift = rifts[k % 4];
            // 同一裂隙的第 n 个个体按 3×3 网格错开 0.8 Tile
            let slot = (k / 4) as i64;
            let (ox, oy) = ((slot % 3 - 1) * 800, (slot / 3 % 3 - 1) * 800);
            let base = Vec2::tile_center(rift.0, rift.1);
            let mut p = base + Vec2::from_milli(ox, oy);
            if !p.tile().is_some_and(|t| self.nav.passable(t.0 as i64, t.1 as i64)) {
                p = base;
            }
            let g = pool[*idx].clone();
            let kind = g.unit;
            let id = self.spawn_unit(FactionId::WILD, kind, g, p);
            let target = nexuses
                .iter()
                .copied()
                .min_by_key(|n| (n.distance_sq(p), n.x, n.y))
                .unwrap_or(p);
            self.units.get_mut(&id).expect("刚生成").order = Order {
                kind: OrderKind::AttackMove,
                target,
                origin: p,
            };
        }
        self.events.push(RtsEvent::WaveSpawned {
            wave,
            count: plan.len() as u32,
        });
    }

    /// 阵营超时积分（milli）
    pub fn score_milli(&self, faction: FactionId) -> i64 {
        let Some(f) = self.faction(faction) else { return 0 };
        let mut s = 0i64;
        if let Some(b) = f.nexus.and_then(|id| self.buildings.get(&id)) {
            s += 1000 * 1000 * b.defense.hp_milli / b.max_hp_milli.max(1);
        }
        let bcost: i64 = self
            .buildings
            .values()
            .filter(|b| b.faction == faction && b.complete)
            .map(|b| building_spec(b.kind).cost.total())
            .sum();
        let ucost: i64 = self
            .units
            .values()
            .filter(|u| u.faction == faction)
            .map(|u| unit_spec(u.kind).cost.total())
            .sum();
        s + 2 * bcost + ucost * 3 / 2 + f.ledger.biomass_harvested_total / 4
    }

    fn finish(&mut self, outcome: MatchOutcome) {
        self.events.push(RtsEvent::MatchEnded {
            outcome: outcome.clone(),
        });
        self.outcome = Some(outcome);
    }

    pub(super) fn step_victory(&mut self) {
        let tick = self.tick;
        let alive: Vec<FactionId> = self
            .factions
            .iter()
            .filter(|f| !f.eliminated && !f.surrendered)
            .map(|f| f.id)
            .collect();
        match self.config.mode {
            MatchMode::ParameterEntities => {}
            MatchMode::PveSurvival => {
                let waves = self.pve.as_ref().map(|d| d.wave).unwrap_or(0);
                if alive.is_empty() {
                    let cleared = waves.saturating_sub(1);
                    self.finish(MatchOutcome::Defeat {
                        tick,
                        waves_cleared: cleared,
                    });
                } else if waves >= PVE_MAX_WAVES && !self.units.values().any(|u| u.faction.is_wild()) {
                    self.finish(MatchOutcome::Victory {
                        winner: alive[0],
                        reason: VictoryReason::WavesCleared,
                        tick,
                    });
                }
            }
            MatchMode::PvpRanked => {
                if alive.len() <= 1 {
                    let reason = if self.factions.iter().any(|f| f.surrendered) {
                        VictoryReason::Surrender
                    } else {
                        VictoryReason::Elimination
                    };
                    let outcome = match alive.first() {
                        Some(w) => MatchOutcome::Victory {
                            winner: *w,
                            reason,
                            tick,
                        },
                        None => MatchOutcome::Draw { tick },
                    };
                    self.finish(outcome);
                    return;
                }
                let (a, b) = (alive[0], alive[1]);
                let (sa, sb) = (self.score_milli(a), self.score_milli(b));
                match self.sudden_death_until {
                    None if tick >= MATCH_TIME_LIMIT_TICKS => {
                        let hi = sa.max(sb);
                        if (sa - sb).abs() * 100 < hi {
                            self.sudden_death_until = Some(tick + SUDDEN_DEATH_TICKS);
                            self.events.push(RtsEvent::SuddenDeathStarted { tick });
                        } else {
                            let winner = if sa > sb { a } else { b };
                            self.finish(MatchOutcome::Victory {
                                winner,
                                reason: VictoryReason::TieBreakerScore,
                                tick,
                            });
                        }
                    }
                    Some(end) if tick >= end => {
                        let outcome = if sa == sb {
                            MatchOutcome::Draw { tick }
                        } else {
                            MatchOutcome::Victory {
                                winner: if sa > sb { a } else { b },
                                reason: VictoryReason::SuddenDeath,
                                tick,
                            }
                        };
                        self.finish(outcome);
                    }
                    _ => {}
                }
            }
        }
    }
}
