// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 经济、施工、生产、研发、繁育与环境场推进。
//!
//! 设计圣经只给出速率（每秒），实现取舍（ERRATA E-031）：
//! - 每秒速率用累计取整差分 [`per_tick_share`] 拆到 Tick，任意连续 30 Tick 精确等于标称值；
//! - **供电**：先结算产出（主脑 `+30/s`、反应堆），再按实体序号依次为耗电建筑扣除本 Tick 能耗，
//!   余额不足的建筑本 Tick 掉电停工（Brownout）；施工中的建筑不耗电也不产出；
//! - **反应堆**须先扣到本 Tick 的 `1.5 Biomass/s` 份额才发电；
//! - **萃取器**每 Tick 至多从 2×2 覆盖格抽取 `0.4` 营养（按格序号从高到低取），`1:1` 精确换算为生物质；
//! - **精炼厂**每 60 Tick 一个周期，周期末若余额覆盖 `10 Biomass + 50 Flux` 才合成 `5 Neurogel`；
//! - **孵化槽**造价在入队时扣除，队首任务在供电时倒计时，完成后在占地外圈最近可通行格孵化；
//! - **施工**无需工蜂，按 `build_ticks` 自动推进。

use crate::buildings::{
    HARVESTER_MAX_DRAW_MILLI_PER_TICK, REACTOR_BIOMASS_UPKEEP_MILLI_PER_S, REACTOR_FISSURE_FLUX_PER_S_MILLI,
    REFINERY_CYCLE_INPUT, REFINERY_CYCLE_OUTPUT_NEUROGEL_MILLI, REFINERY_CYCLE_TICKS, building_spec,
};
use crate::commands::RtsError;
use crate::evolution::{EvolutionRng, baldwin_assimilate, breed};
use crate::fields::{Q8_TO_MILLI_DENOM, Q8_TO_MILLI_NUMER};
use crate::genome::Genome;
use crate::ids::{BuildingKind, EntitySerial, FactionId, FieldId, ResourceKind, UnitKind};
use crate::math::{Vec2, mul_div, per_tick_share};
use crate::modulators::inject_beacon;

use super::{ArenaWorld, RtsEvent};

/// 萃取器每 Tick 抽取上限（Q8.8）：`0.4 × 256 = 102.4` → 累计取整 `512 / 5`
const HARVESTER_DRAW_Q8_NUMER: i64 = 512;
const HARVESTER_DRAW_Q8_DENOM: i64 = 5;
const _: () =
    assert!(HARVESTER_MAX_DRAW_MILLI_PER_TICK * 256 == HARVESTER_DRAW_Q8_NUMER * 1000 / HARVESTER_DRAW_Q8_DENOM);

impl ArenaWorld {
    pub(super) fn step_economy(&mut self) {
        let tick = self.tick;
        let ids: Vec<EntitySerial> = self.buildings.keys().copied().collect();
        // 1) 产出
        for id in &ids {
            let b = &self.buildings[id];
            if !b.complete || b.faction.is_wild() {
                continue;
            }
            let (faction, kind, on_fissure) = (b.faction, b.kind, b.on_fissure);
            let Some(f) = self.faction_mut(faction) else { continue };
            match kind {
                BuildingKind::NexusCore => {
                    let share = per_tick_share(building_spec(BuildingKind::NexusCore).flux_per_s_milli, tick);
                    f.ledger.deposit(ResourceKind::Flux, share);
                    self.buildings.get_mut(id).expect("存在").powered = true;
                }
                BuildingKind::FluxReactor => {
                    let upkeep = per_tick_share(REACTOR_BIOMASS_UPKEEP_MILLI_PER_S, tick);
                    let ok = f.ledger.try_consume(ResourceKind::Biomass, upkeep);
                    if ok {
                        let base = if on_fissure {
                            REACTOR_FISSURE_FLUX_PER_S_MILLI
                        } else {
                            building_spec(BuildingKind::FluxReactor).flux_per_s_milli
                        };
                        let rate = mul_div(base, f.modifiers().reactor_output_permille, 1000);
                        f.ledger.deposit(ResourceKind::Flux, per_tick_share(rate, tick));
                    }
                    self.buildings.get_mut(id).expect("存在").powered = ok;
                }
                _ => {}
            }
        }
        // 2) 耗电（按序号）
        for id in &ids {
            let b = &self.buildings[id];
            let consumption = -building_spec(b.kind).flux_per_s_milli;
            if consumption <= 0 {
                continue;
            }
            let faction = b.faction;
            let complete = b.complete;
            let powered = complete
                && self.faction_mut(faction).is_some_and(|f| {
                    f.ledger
                        .try_consume(ResourceKind::Flux, per_tick_share(consumption, tick))
                });
            self.buildings.get_mut(id).expect("存在").powered = powered;
        }
        // 3) 运转
        for id in ids {
            let b = &self.buildings[&id];
            if !b.powered {
                continue;
            }
            match b.kind {
                BuildingKind::BiomassHarvester => self.run_harvester(id),
                BuildingKind::SynapseRefinery => self.run_refinery(id),
                BuildingKind::SpawningVat => self.run_vat(id),
                BuildingKind::MutationCrucible => self.run_crucible(id),
                _ => {}
            }
            if matches!(
                self.buildings.get(&id).map(|b| b.kind),
                Some(BuildingKind::SynapseRefinery | BuildingKind::MutationCrucible)
            ) {
                self.run_research(id);
            }
        }
    }

    fn run_harvester(&mut self, id: EntitySerial) {
        let b = &self.buildings[&id];
        let (faction, rect) = (b.faction, b.rect());
        let mut budget =
            crate::math::fractional_share(HARVESTER_DRAW_Q8_NUMER, HARVESTER_DRAW_Q8_DENOM, self.tick) as i32;
        let mut drawn = 0i64;
        // 从最富营养的格开始抽取（同值按 (y, x) 升序）
        let mut cells: Vec<(i16, u32, u32)> = (rect.y0..rect.y1)
            .flat_map(|y| (rect.x0..rect.x1).map(move |x| (x, y)))
            .map(|(x, y)| (self.fields.get(FieldId::Nutrient, x as i64, y as i64), y, x))
            .collect();
        cells.sort_by_key(|(v, y, x)| (std::cmp::Reverse(*v), *y, *x));
        for (v, y, x) in cells {
            if budget <= 0 {
                break;
            }
            let take = (v as i32).min(budget);
            if take > 0 {
                self.fields.add(FieldId::Nutrient, x, y, -take);
                budget -= take;
                drawn += take as i64;
            }
        }
        let b = self.buildings.get_mut(&id).expect("存在");
        let total = drawn * Q8_TO_MILLI_NUMER + b.harvest_remainder;
        let milli = total / Q8_TO_MILLI_DENOM;
        b.harvest_remainder = total % Q8_TO_MILLI_DENOM;
        if let Some(f) = self.faction_mut(faction) {
            let accepted = f.ledger.deposit(ResourceKind::Biomass, milli);
            f.ledger.biomass_harvested_total += accepted;
        }
    }

    fn run_refinery(&mut self, id: EntitySerial) {
        let b = self.buildings.get_mut(&id).expect("存在");
        b.refinery_progress += 1;
        if b.refinery_progress < REFINERY_CYCLE_TICKS {
            return;
        }
        b.refinery_progress = 0;
        let faction = b.faction;
        if let Some(f) = self.faction_mut(faction) {
            if f.ledger.spend(&REFINERY_CYCLE_INPUT).is_ok() {
                f.ledger
                    .deposit(ResourceKind::Neurogel, REFINERY_CYCLE_OUTPUT_NEUROGEL_MILLI);
            }
        }
    }

    fn run_vat(&mut self, id: EntitySerial) {
        let b = self.buildings.get_mut(&id).expect("存在");
        let Some(job) = b.queue.front_mut() else { return };
        job.remaining_ticks = job.remaining_ticks.saturating_sub(1);
        if job.remaining_ticks > 0 {
            return;
        }
        let job = b.queue.pop_front().expect("非空");
        let (faction, rect) = (b.faction, b.rect());
        let spot = self.free_tile_around(rect);
        self.spawn_unit(faction, job.unit, job.genome, Vec2::tile_center(spot.0, spot.1));
    }

    /// 占地外圈最近的可通行 Tile（按环距、再按 `(y, x)` 升序）
    pub(crate) fn free_tile_around(&self, rect: crate::nav::Rect) -> (u32, u32) {
        for r in 1..64i64 {
            let mut best: Option<(u32, u32)> = None;
            for y in rect.y0 as i64 - r..rect.y1 as i64 + r {
                for x in rect.x0 as i64 - r..rect.x1 as i64 + r {
                    let ring = x < rect.x0 as i64 - r + 1
                        || x >= rect.x1 as i64 + r - 1
                        || y < rect.y0 as i64 - r + 1
                        || y >= rect.y1 as i64 + r - 1;
                    if ring && self.nav.passable(x, y) {
                        let c = (x as u32, y as u32);
                        if best.is_none_or(|b| (c.1, c.0) < (b.1, b.0)) {
                            best = Some(c);
                        }
                    }
                }
            }
            if let Some(b) = best {
                return b;
            }
        }
        (rect.x0, rect.y0)
    }

    fn run_research(&mut self, id: EntitySerial) {
        let b = self.buildings.get_mut(&id).expect("存在");
        let Some((tech, remaining)) = b.research.as_mut() else {
            return;
        };
        *remaining = remaining.saturating_sub(1);
        if *remaining > 0 {
            return;
        }
        let tech = *tech;
        b.research = None;
        let faction = b.faction;
        if let Some(f) = self.faction_mut(faction) {
            f.techs = f.techs.with(tech);
        }
        // 即时生效的修正：护甲、电容上限
        if tech == crate::ids::TechId::MulticellularChitin {
            for u in self.units.values_mut().filter(|u| u.faction == faction) {
                u.defense.armor += 2;
            }
        }
        self.recompute_capacity(faction);
        self.events.push(RtsEvent::ResearchCompleted { faction, tech });
    }

    fn run_crucible(&mut self, id: EntitySerial) {
        let b = self.buildings.get_mut(&id).expect("存在");
        let Some(job) = b.breeding.as_mut() else { return };
        job.remaining_ticks = job.remaining_ticks.saturating_sub(1);
        if job.remaining_ticks > 0 {
            return;
        }
        let job = b.breeding.take().expect("存在");
        let faction = b.faction;
        let slug = job.child.slug.clone();
        if let Some(f) = self.faction_mut(faction) {
            f.genomes.insert(slug.clone(), job.child);
        }
        self.events.push(RtsEvent::GenomeBred {
            faction,
            slug,
            unit: job.unit,
        });
    }

    /// 选择本阵营该原型适应度最高的两个存活个体并预先生成后代（命令受理时即确定，保证回放一致）
    pub(super) fn prepare_breeding(
        &mut self,
        faction: FactionId,
        unit: UnitKind,
        rate: u32,
    ) -> Result<Genome, RtsError> {
        let mut cands: Vec<(i64, EntitySerial)> = self
            .units
            .values()
            .filter(|u| u.faction == faction && u.kind == unit)
            .map(|u| {
                let mut fit = u.fitness;
                fit.survival_ticks = self.tick - u.born_tick;
                (fit.fitness_milli(), u.id)
            })
            .collect();
        if cands.is_empty() {
            return Err(RtsError::NoBreedingCandidates { unit });
        }
        cands.sort_by_key(|(f, id)| (std::cmp::Reverse(*f), *id));
        let a = &self.units[&cands[0].1];
        let b = cands.get(1).map(|c| &self.units[&c.1]).unwrap_or(a);
        let mods = self.faction(faction).expect("存在").modifiers();
        let n = self.faction(faction).expect("存在").bred_count + 1;
        let mut rng = EvolutionRng::new(self.config.world_seed, self.tick, ((faction.0 as u64) << 32) | n as u64);
        let mut child = breed(&a.genome, &b.genome, rate, &mut rng, format!("bred.f{}.{n}", faction.0));
        // 鲍德温：把亲本 LIF 后天可塑性偏移按 α 同化
        if let Some(delta) = a.brain.plasticity_delta() {
            baldwin_assimilate(&mut child, delta, mods.baldwin_alpha_permille);
        }
        self.faction_mut(faction).expect("存在").bred_count = n;
        Ok(child)
    }

    pub(super) fn step_construction(&mut self) {
        let mut completed = Vec::new();
        for b in self.buildings.values_mut() {
            if b.complete {
                continue;
            }
            let spec = building_spec(b.kind);
            b.build_progress += 1;
            let start = b.max_hp_milli * super::CONSTRUCTION_START_HP_PERCENT / 100;
            let gain = (b.max_hp_milli - start) / spec.build_ticks as i64;
            b.defense.hp_milli = (b.defense.hp_milli + gain).min(b.max_hp_milli);
            if b.build_progress >= spec.build_ticks {
                b.complete = true;
                completed.push((b.id, b.faction, b.kind));
            }
        }
        for (id, faction, kind) in completed {
            self.recompute_capacity(faction);
            self.events.push(RtsEvent::BuildingCompleted { id, faction, kind });
        }
    }

    pub(super) fn step_fields(&mut self) {
        // 信标注入（按序号）
        let beacons: Vec<(EntitySerial, (i64, i64), u32, crate::modulators::BeaconConfig)> = self
            .buildings
            .values()
            .filter(|b| b.kind == BuildingKind::SignalBeacon && b.powered)
            .map(|b| {
                let c = b.center_tile();
                let r = self
                    .faction(b.faction)
                    .map(|f| f.modifiers().beacon_radius_tiles)
                    .unwrap_or(16);
                (b.id, (c.0 as i64, c.1 as i64), r, b.beacon)
            })
            .collect();
        for (_, c, r, cfg) in beacons {
            inject_beacon(&mut self.fields, c, r, &cfg);
        }
        // 反应堆钉住能量场：占地格 = 100.0
        let reactors: Vec<crate::nav::Rect> = self
            .buildings
            .values()
            .filter(|b| b.kind == BuildingKind::FluxReactor && b.powered)
            .map(|b| b.rect())
            .collect();
        for r in reactors {
            for y in r.y0..r.y1 {
                for x in r.x0..r.x1 {
                    self.fields.set(FieldId::Energy, x, y, 100 * 256);
                }
            }
        }
        self.fields.step(&self.map.tiles);
    }
}
