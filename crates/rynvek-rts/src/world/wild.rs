// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 自然野生profile：种子只有基因与原RTS初始HP，没有免费内能或结构材料。
//! 局部营养是唯一进项；不运行波次预算、精英适应度筛选或玩家阵营补给。
use super::{
    ArenaWorld,
    ecology::{ActivityKind, RoleEnergyContract},
};
use crate::{ArenaMap, EntitySerial, FactionId, UnitKind, Vec2, genome::Genome, nav::NavGrid};
use rynvek_neural::metabolism::ENERGY_Q_PER_MILLI;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 有界计算/规模安全限制，不冒充原规范冻结的生态平衡参数。
pub const MAX_WILD_SEEDS: usize = 16;
pub const MAX_WILD_POPULATION: u32 = 128;
pub const MAX_WILD_UPTAKE_RAW: u32 = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WildRoleContract {
    pub energy: RoleEnergyContract,
    pub uptake_raw_per_tick: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WildSeed {
    pub kind: UnitKind,
    pub x: u32,
    pub y: u32,
}
/// 空表/0上限/空种子明确保留原RTS波次profile；字段仍须完整给出，不读旧格式。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WildEcologyContract {
    pub roles: BTreeMap<UnitKind, WildRoleContract>,
    pub population_limit: u32,
    pub seeds: Vec<WildSeed>,
}
impl WildEcologyContract {
    pub fn enabled(&self) -> bool {
        !self.roles.is_empty()
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.enabled() {
            return if self.population_limit == 0 && self.seeds.is_empty() {
                Ok(())
            } else {
                Err("关闭野生自然profile时须为空种子与0上限")
            };
        }
        if self.seeds.is_empty()
            || self.seeds.len() > MAX_WILD_SEEDS
            || !(1..=MAX_WILD_POPULATION).contains(&self.population_limit)
            || self.seeds.len() > self.population_limit as usize
        {
            return Err("野生种子须1..16且不超过1..128的种群上限");
        }
        for r in self.roles.values() {
            r.energy.validate()?;
            if r.uptake_raw_per_tick > MAX_WILD_UPTAKE_RAW {
                return Err("野生局部摄食须在0..256 raw/Tick");
            }
            if r.energy.material_capacity_milli < 2 || r.energy.material_growth_milli_per_tick == 0 {
                return Err("自然野生身体须显式启用至少2 milli结构容量和正增长速率");
            }
        }
        let mut cells = BTreeSet::new();
        for s in &self.seeds {
            if !self.roles.contains_key(&s.kind) || !cells.insert((s.x, s.y)) {
                return Err("野生种子须有完整角色契约且不能重叠");
            }
        }
        Ok(())
    }
    pub(super) fn validate_map(&self, map: &ArenaMap, players: u8) -> Result<(), &'static str> {
        let nav = NavGrid::from_terrain(map.width, map.height, &map.tiles);
        let anchors = map.spawn_tiles();
        for s in &self.seeds {
            if !nav.passable(i64::from(s.x), i64::from(s.y))
                || anchors.iter().take(usize::from(players)).any(|&(x, y)| {
                    s.x >= x.saturating_sub(2) && s.x < x + 2 && s.y >= y.saturating_sub(2) && s.y < y + 2
                })
            {
                return Err("野生种子须在可通行地图范围内且避开初始主脑占格");
            }
        }
        Ok(())
    }
}
impl ArenaWorld {
    pub(super) fn seed_wild_ecology(&mut self) {
        let Some(c) = self.config.ecology.as_ref().filter(|c| c.wild.enabled()) else {
            return;
        };
        c.wild
            .validate_map(&self.map, self.config.players)
            .expect("初始野生种子位置须通过实际地图验证");
        let seeds = c.wild.seeds.clone();
        for (i, seed) in seeds.into_iter().enumerate() {
            let mut genome = Genome::seed(seed.kind);
            genome.slug = format!("wild.seed.{i}");
            self.spawn_unit(FactionId::WILD, seed.kind, genome, Vec2::tile_center(seed.x, seed.y));
        }
    }
    pub(super) fn validate_wild_profile(&self) -> bool {
        let Some(c) = self.config.ecology.as_ref().filter(|c| c.wild.enabled()) else {
            return true;
        };
        c.wild.validate_map(&self.map, self.config.players).is_ok()
            && self.pve.is_none()
            && self.pve_records.is_empty()
            && self.units.values().filter(|u| u.faction.is_wild()).count() <= c.wild.population_limit as usize
            && self
                .units
                .values()
                .filter(|u| u.faction.is_wild())
                .all(|u| c.wild.roles.contains_key(&u.kind))
    }
    /// 营养已在原摄食步骤扣场，只从身体原池转出化学预算。1 milli预算=256u；
    /// 合成热耗另付，不重复领营养，不将实验等价物解释为真实质能互换或元素化学。
    pub(super) fn grow_wild_material(&mut self, id: EntitySerial, r: RoleEnergyContract) {
        let keep = self
            .maintenance_q(id, r)
            .saturating_mul(u64::from(r.reserve_ticks).max(1))
            .saturating_add(self.neural_reserve_q(id))
            .saturating_add(r.activity_reserve_q());
        let heat = u64::from(r.material_synthesis_micro_per_milli) * 4;
        let b = &self.ecology.as_ref().expect("显式契约").bodies[&id];
        let cap = u64::from(r.material_capacity_milli) * 256;
        let accepted = u64::from(r.material_growth_milli_per_tick)
            .min(cap.saturating_sub(b.material.stored_u) / 256)
            .min((u64::MAX - b.material.received_u) / 256)
            .min((u64::MAX - b.material.converted_u) / 256)
            .min(b.account.stored_q.saturating_sub(keep) / (ENERGY_Q_PER_MILLI + heat));
        if accepted == 0 {
            return;
        }
        assert!(self.charge_activity(id, ActivityKind::Synthesis, accepted * heat));
        let b = self
            .ecology
            .as_mut()
            .expect("显式契约")
            .bodies
            .get_mut(&id)
            .expect("活体");
        assert_eq!(b.account.export_milli_retaining(accepted, keep), accepted);
        b.material.received_u += accepted * 256;
        b.material.converted_u += accepted * 256;
        b.material.stored_u += accepted * 256;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ecology::EcologyContract;
    use crate::{ArenaRuntime, FieldId, MatchConfig, TerrainTileKind};
    fn fixture(kind: UnitKind, limit: u32) -> (ArenaWorld, EntitySerial) {
        let r = RoleEnergyContract {
            basal_cost_micro_per_tick: 10,
            reserve_ticks: 1,
            birth_energy_milli: 10,
            movement_cost_micro_per_tile: 1,
            synapse_maintenance_micro_per_tick: 0,
            attack_cost_micro_per_shot: 1,
            repair_cost_micro_per_milli: 1,
            secretion_cost_micro_per_field_raw: 1,
            continuous_cost_micro_per_neuron_step: 1,
            rule_cost_micro_per_gas: 1,
            material_capacity_milli: 8,
            material_growth_milli_per_tick: 4,
            material_synthesis_micro_per_milli: 1,
            birth_synthesis_milli: 1,
            activity_reserve_milli: 1,
            natural_reproduction: true,
            replication_mutation_rate_permille: 50,
        };
        let generated = ArenaMap::generate(crate::ArenaMapSize::Duel1v1, 57);
        let (x, y) = (40..70)
            .flat_map(|y| (40..70).map(move |x| (x, y)))
            .find(|&(x, y)| generated.tile(x, y) == TerrainTileKind::OpenSubstrate)
            .unwrap();
        let mut config = MatchConfig::pve(57, 1);
        config.ecology = Some(EcologyContract {
            roles: BTreeMap::new(),
            wild: WildEcologyContract {
                roles: BTreeMap::from([(
                    kind,
                    WildRoleContract {
                        energy: r,
                        uptake_raw_per_tick: 128,
                    },
                )]),
                population_limit: limit,
                seeds: vec![WildSeed { kind, x, y }],
            },
        });
        let w = ArenaWorld::with_map(
            config,
            ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
        );
        let id = *w.units.iter().find(|(_, u)| u.faction.is_wild()).unwrap().0;
        (w, id)
    }
    #[test]
    fn wild_sourced_intake_material_mitosis_cap_death_and_no_double_return() {
        // 非Forager也必须走同一有来源摄食链，不能偷偷取玩家阵营资金。
        let (mut w, id) = fixture(UnitKind::Striker, 2);
        let initial_balance = w.factions[0].ledger.balance;
        let (x, y) = w.units[&id].tile();
        assert_eq!(w.ecology.as_ref().unwrap().bodies[&id].account.received_q, 0);
        assert_eq!(w.unit_radius_milli(id), 0);
        let source = w.fields.get(FieldId::Nutrient, x.into(), y.into());
        for _ in 0..2 {
            w.step_ecology_uptake();
            w.step_role_supply();
            w.step_material_growth();
        }
        let b = &w.ecology.as_ref().unwrap().bodies[&id];
        assert_eq!(w.fields.get(FieldId::Nutrient, x.into(), y.into()), source - 256);
        assert_eq!(b.account.received_q, 256 * 15_625);
        assert_eq!(
            (b.material.external_u, b.material.converted_u, b.material.stored_u),
            (0, 2048, 2048)
        );
        assert_eq!(b.account.exported_q, 8 * 4000);
        assert_eq!(b.activity.synthesis_q, 8 * 4);
        assert_eq!(w.factions[0].ledger.balance, initial_balance);
        assert_eq!(
            w.unit_radius_milli(id),
            crate::units::unit_spec(UnitKind::Striker).radius_milli
        );
        w.units.get_mut(&id).unwrap().actions[crate::genome::action::EMIT] = 256;
        w.step_signal_emission();
        assert_eq!(w.ecology.as_ref().unwrap().bodies[&id].activity.secretion_q, 1024);
        w.units.get_mut(&id).unwrap().actions[crate::genome::action::MOVE_X] = 256;
        let pos = w.units[&id].position;
        w.step_movement();
        assert_ne!(w.units[&id].position, pos, "原Hold默认不能封死野生自主移动");
        assert!(w.ecology.as_ref().unwrap().bodies[&id].movement_remainder > 0);
        w.tick = 1;
        let mut occupied = w.units.values().map(|u| u.tile()).collect();
        w.try_mitosis(id, 40_000, &mut occupied);
        let e = w.ecology.as_ref().unwrap();
        let child = *e.births.keys().next().expect("无需玩家建筑的真实野生复制");
        assert_eq!(e.births[&child].allocated_material_u, 1024);
        assert_eq!(w.units[&child].genome.generation, 1);
        assert!(w.validate_ecology());
        // 补足来源材料后仍受显式存活上限限制，失败不扣费/不耗序号。
        w.step_material_growth();
        let before = w.ecology.as_ref().unwrap().clone();
        let next = w.next_serial();
        w.try_mitosis(id, 40_000, &mut occupied);
        assert_eq!(w.ecology.as_ref().unwrap(), &before);
        assert_eq!(w.next_serial(), next);
        for dead in [id, child] {
            w.retire_dead_unit(dead);
        }
        assert!(w.pve.is_none() && w.pve_records.is_empty());
        w.step_ecology_returns();
        w.step_material_returns();
        assert!(w.validate_ecology());
        let e = w.ecology.as_ref().unwrap();
        assert!(
            e.remains
                .values()
                .all(|r| r.material.stored_u < 1000 && r.account.stored_q < 15_625)
        );
        assert!(
            e.remains
                .values()
                .all(|r| r.material.is_conserved() && r.account.is_conserved())
        );
        let fields = w.fields.export_q8();
        w.step_ecology_returns();
        w.step_material_returns();
        assert_eq!(fields, w.fields.export_q8());
    }
    #[test]
    fn wild_finite_local_source_exhausts_and_empty_seed_dies_without_free_corpse() {
        let (mut w, id) = fixture(UnitKind::Forager, 2);
        let (x, y) = w.units[&id].tile();
        let raw = w.fields.get(FieldId::Nutrient, x.into(), y.into());
        for _ in 0..50 {
            w.step_ecology_uptake();
        }
        assert_eq!(w.fields.get(FieldId::Nutrient, x.into(), y.into()), 0);
        assert_eq!(
            w.ecology.as_ref().unwrap().bodies[&id].account.received_q,
            u64::from(raw as u16) * 15_625
        );
        let b = w.ecology.as_ref().unwrap().bodies[&id].clone();
        w.step_ecology_uptake();
        assert_eq!(w.ecology.as_ref().unwrap().bodies[&id], b);
        let (mut empty, id) = fixture(UnitKind::Forager, 2);
        empty.fields = crate::fields::FieldGrid::new(96, 96);
        empty.step();
        assert!(!empty.units.contains_key(&id));
        let r = &empty.ecology.as_ref().unwrap().remains[&id];
        assert_eq!((r.account.received_q, r.material.received_u), (0, 0));
        assert!(empty.fields.export_q8().iter().all(|v| *v == 0));
        assert!(empty.pve_records.is_empty());
        assert!(empty.validate_ecology());
    }
    #[test]
    fn wild_real_tick_checkpoint_wal_replay_and_director_isolation() {
        use rynvek_journal::JournalWriter;
        let (mut w, id) = fixture(UnitKind::Forager, 3);
        let initial = ArenaRuntime::new([44; 16], w.clone()).checkpoint().unwrap();
        ArenaRuntime::restore([44; 16], initial.revision, initial.state_root, &initial.bytes).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let snap = dir.path().join("wild.rsnap");
        let log = dir.path().join("wild.rjournal");
        let out = w.save_snapshot(&snap, [44; 16], 1, [0; 32]).unwrap();
        let mut parent = out.state_root;
        let mut writer = JournalWriter::create(&log, [44; 16], w.tick, 1, w.tick).unwrap();
        for revision in 2..=13 {
            w.step_with_journal(&mut writer, [44; 16], revision, &mut parent)
                .unwrap();
        }
        drop(writer);
        assert!(w.ecology.as_ref().unwrap().bodies[&id].material.received_u > 0);
        assert!(!w.ecology.as_ref().unwrap().births.is_empty());
        assert!(w.ecology.as_ref().unwrap().bodies[&id].activity.continuous_q > 0);
        let (replay, _) = ArenaWorld::recover_from_snapshot_and_journal(&snap, &log, [44; 16]).unwrap();
        assert_eq!(w.canonical_state().state_root(), replay.canonical_state().state_root());
        w.tick = 1800;
        w.events.clear();
        w.step_pve();
        assert!(
            !w.events
                .iter()
                .any(|e| matches!(e, crate::world::RtsEvent::WaveSpawned { .. }))
        );
        assert!(w.pve.is_none() && w.pve_records.is_empty());
    }
    #[test]
    fn wild_rules_pay_actual_gas_and_reject_all_writes_without_funds() {
        use crate::world::authoring::{AuthoredRule, compile_action_rule};
        let (mut w, id) = fixture(UnitKind::Striker, 2);
        w.step_ecology_uptake();
        w.step_material_growth();
        let source=serde_json::json!({"format":"rynvek.rule-graph.v1","name":"野生实付动作","writes":[{"component":"unit.actions.move_x","ty":"Q8_8"}],"nodes":[{"op":"ConstQ8_8","imm":"128"},{"op":"EmitComponentWrite","component":"unit.actions.move_x","args":[0]}]}).to_string();
        let module = compile_action_rule(&source).unwrap();
        let rir = rynvek_rules::encode_rir(&module);
        w.authoring.rules.insert(
            "wild.paid".into(),
            AuthoredRule {
                rule_graph_json: source,
                compiled_rir_blake3: *blake3::hash(&rir).as_bytes(),
                rir,
            },
        );
        let before = w.ecology.as_ref().unwrap().bodies[&id].account.stored_q;
        w.step_authored_rules();
        let b = &w.ecology.as_ref().unwrap().bodies[&id];
        let fee = b.activity.rule_q;
        assert!(fee > 0);
        assert_eq!(before - b.account.stored_q, fee);
        assert_eq!(w.units[&id].actions[0], 128);
        let a = &mut w.ecology.as_mut().unwrap().bodies.get_mut(&id).unwrap().account;
        a.dissipate_q(a.stored_q - (fee - 1));
        w.units.get_mut(&id).unwrap().actions[0] = -17;
        let before = w.ecology.as_ref().unwrap().clone();
        let actions: Vec<_> = w.units.values().map(|u| u.actions).collect();
        w.step_authored_rules();
        assert_eq!(w.ecology.as_ref().unwrap(), &before);
        assert_eq!(w.units.values().map(|u| u.actions).collect::<Vec<_>>(), actions);
        assert_eq!(w.authoring.rule_fault.as_ref().unwrap().0, "wild.paid");
    }
    #[test]
    fn wild_contract_rejects_unfunded_or_unbounded_profiles_and_old_shape() {
        let (w, _) = fixture(UnitKind::Forager, 2);
        let original = w.config.ecology.as_ref().unwrap();
        let mut old = serde_json::to_value(original).unwrap();
        old.as_object_mut().unwrap().remove("wild");
        assert!(serde_json::from_value::<EcologyContract>(old).is_err());
        for case in 0..6 {
            let mut c = original.clone();
            let role = c.wild.roles.get_mut(&UnitKind::Forager).unwrap();
            match case {
                0 => c.wild.population_limit = 129,
                1 => role.uptake_raw_per_tick = 257,
                2 => role.energy.material_capacity_milli = 0,
                3 => c.wild.seeds.push(c.wild.seeds[0]),
                4 => c.wild.seeds.clear(),
                _ => c.wild.roles.clear(),
            }
            assert!(c.validate().is_err());
        }
        let mut config = w.config.clone();
        config.ecology.as_mut().unwrap().wild.seeds[0].x = u32::MAX;
        assert!(config.validate_energy_contract().is_err());
    }
}
