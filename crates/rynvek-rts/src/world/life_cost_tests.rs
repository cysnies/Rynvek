// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use super::ecology::{EcologyContract, RoleEnergyContract};
use super::*;
use crate::{ArenaRuntime, TerrainTileKind};
use rynvek_journal::JournalWriter;

const OWNER: FactionId = FactionId(0);
fn fixture(kind: UnitKind, remaining_q: u64) -> (ArenaWorld, EntitySerial, EntitySerial) {
    let mut config = MatchConfig::pve(57, 1);
    let mut r = RoleEnergyContract {
        basal_cost_micro_per_tick: 10,
        reserve_ticks: 4,
        birth_energy_milli: 1000,
        movement_cost_micro_per_tile: 0,
        synapse_maintenance_micro_per_tick: 0,
        natural_reproduction: false,
        replication_mutation_rate_permille: 0,
        attack_cost_micro_per_shot: 0,
        repair_cost_micro_per_milli: 0,
        continuous_cost_micro_per_neuron_step: 0,
        secretion_cost_micro_per_field_raw: 0,
        birth_synthesis_milli: 0,
        activity_reserve_milli: 10,
    };
    r.attack_cost_micro_per_shot = 3;
    r.repair_cost_micro_per_milli = 2;
    r.secretion_cost_micro_per_field_raw = 2;
    config.ecology = Some(EcologyContract {
        roles: [
            UnitKind::Striker,
            UnitKind::Spitter,
            UnitKind::Weaver,
            UnitKind::Carapace,
        ]
        .into_iter()
        .map(|k| (k, r))
        .collect(),
    });
    let mut w = ArenaWorld::with_map(
        config,
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
    );
    for u in w.units.values_mut() {
        u.actions[crate::genome::action::ATTACK] = -256;
    }
    let source = w.spawn_unit(OWNER, kind, Genome::seed(kind), Vec2::tile_center(44, 44));
    let target = w.spawn_unit(
        FactionId::WILD,
        UnitKind::Striker,
        Genome::seed(UnitKind::Striker),
        Vec2::tile_center(46, 44),
    );
    w.units.get_mut(&target).unwrap().actions[crate::genome::action::ATTACK] = -256;
    w.step_role_supply();
    // 定向边界夹具：先真实实扣阵营供给，再将已有能量耗散到指定余款，不凭空注资。
    let a = &mut w.ecology.as_mut().unwrap().bodies.get_mut(&source).unwrap().account;
    a.dissipate_q(a.stored_q - remaining_q);
    w.tick = 1;
    (w, source, target)
}
#[test]
fn life_cost_shot_rejects_atomically_and_idle_dead_sources_and_targets_do_not_charge() {
    let (mut w, s, t) = fixture(UnitKind::Striker, 11); // one shot costs 12q
    let before = w.ecology.as_ref().unwrap().bodies[&s].clone();
    let shield = w.units[&t].defense.shield_milli;
    w.step_combat();
    assert_eq!(w.ecology.as_ref().unwrap().bodies[&s], before);
    assert_eq!(w.units[&t].defense.shield_milli, shield);
    assert_eq!(w.units[&s].weapon_cooldown, 0);
    // 无目标、尸体都不应消费，即便将发射价显式设为0也不允许死后开火。
    w.config
        .ecology
        .as_mut()
        .unwrap()
        .roles
        .get_mut(&UnitKind::Striker)
        .unwrap()
        .attack_cost_micro_per_shot = 0;
    w.units.get_mut(&t).unwrap().position = Vec2::tile_center(80, 80);
    w.step_combat();
    assert_eq!(w.units[&s].weapon_cooldown, 0);
    w.units.get_mut(&t).unwrap().position = Vec2::tile_center(46, 44);
    w.units.get_mut(&s).unwrap().defense.hp_milli = 0;
    w.step_combat();
    assert_eq!(w.units[&s].weapon_cooldown, 0);
    w.units.get_mut(&s).unwrap().defense.hp_milli = 1;
    w.units.get_mut(&t).unwrap().defense.hp_milli = 0;
    w.step_combat();
    assert_eq!(w.units[&s].weapon_cooldown, 0);
    assert_eq!(w.ecology.as_ref().unwrap().bodies[&s], before);
}
#[test]
fn life_cost_secretion_bills_real_clamped_output_preserves_residue_and_wal_recovers_heat() {
    let (mut w, s, _) = fixture(UnitKind::Spitter, 25);
    w.fields.set(FieldId::Hazard, 46, 44, 25_598);
    w.step_combat();
    let b = &w.ecology.as_ref().unwrap().bodies[&s];
    assert_eq!(
        (b.activity.attack_q, b.activity.secretion_q, b.account.stored_q),
        (12, 8, 5)
    );
    assert!(b.account.is_conserved());
    assert_eq!(w.fields.get(FieldId::Hazard, 46, 44), 25_599);
    w.faction_mut(OWNER).unwrap().ledger.balance.biomass = 0;
    let dir = tempfile::TempDir::new().unwrap();
    let snap = dir.path().join("life.rsnap");
    let log = dir.path().join("life.rjournal");
    let id = [39; 16];
    let out = w.save_snapshot(&snap, id, 1, [0; 32]).unwrap();
    let mut parent = out.state_root;
    let mut writer = JournalWriter::create(&log, id, w.tick, 1, w.tick).unwrap();
    w.step_with_journal(&mut writer, id, 2, &mut parent).unwrap();
    writer.append_torn_bytes(&[0xff, 0xee]).unwrap();
    let (recovered, result) = ArenaWorld::recover_from_snapshot_and_journal(&snap, &log, id).unwrap();
    assert!(result.torn_write_detected);
    assert_eq!(recovered.canonical_state(), w.canonical_state());
    assert!(!w.units.contains_key(&s));
    let heat = &recovered.ecology.as_ref().unwrap().remains[&s].activity;
    assert_eq!((heat.attack_q, heat.secretion_q), (12, 8));
}
#[test]
fn life_cost_saturated_secretion_and_exact_exhaustion_do_not_create_ghost_bodies() {
    let (mut w, s, _) = fixture(UnitKind::Spitter, 12);
    w.fields.set(FieldId::Hazard, 46, 44, 25_600);
    w.step_combat();
    let b = &w.ecology.as_ref().unwrap().bodies[&s];
    assert_eq!(
        (b.account.stored_q, b.activity.attack_q, b.activity.secretion_q),
        (0, 12, 0)
    );
    w.step_environment(); // exhausted source cannot repair itself
    w.retire_activity_exhausted();
    assert!(!w.units.contains_key(&s));
    assert_eq!(w.ecology.as_ref().unwrap().remains[&s].activity.attack_q, 12);
    ArenaRuntime::new([39; 16], w).checkpoint().unwrap();
}
#[test]
fn life_cost_shield_and_aura_pay_actual_repair_and_never_resurrect_dead_donors() {
    let (mut w, s, t) = fixture(UnitKind::Weaver, 21); // 2 repair quanta, 5q residue
    w.units.get_mut(&t).unwrap().faction = OWNER;
    w.units.get_mut(&t).unwrap().defense.hp_milli -= 10;
    let hp = w.units[&t].defense.hp_milli;
    w.step_environment();
    assert_eq!(w.units[&t].defense.hp_milli, hp + 2);
    let b = &w.ecology.as_ref().unwrap().bodies[&s];
    assert_eq!((b.activity.repair_q, b.account.stored_q), (16, 5));
    w.step_environment();
    assert_eq!(w.units[&t].defense.hp_milli, hp + 2);
    // 满血无费用；死源即使有足量真实余额也不允许恢复他人。
    let (mut full, donor, target) = fixture(UnitKind::Weaver, 1000);
    full.units.get_mut(&target).unwrap().faction = OWNER;
    full.step_environment();
    assert_eq!(full.ecology.as_ref().unwrap().bodies[&donor].activity.repair_q, 0);
    full.units.get_mut(&target).unwrap().defense.hp_milli -= 10;
    full.units.get_mut(&donor).unwrap().defense.hp_milli = 0;
    let hp = full.units[&target].defense.hp_milli;
    full.step_environment();
    assert_eq!(full.units[&target].defense.hp_milli, hp);
    let (mut shield, id, _) = fixture(UnitKind::Carapace, 21);
    shield.units.get_mut(&id).unwrap().defense.shield_milli -= 1;
    shield.step_environment();
    assert_eq!(
        shield.units[&id].defense.shield_milli,
        shield.units[&id].max_shield_milli
    );
    assert_eq!(shield.ecology.as_ref().unwrap().bodies[&id].activity.repair_q, 8);
    shield.step_environment();
    assert_eq!(shield.ecology.as_ref().unwrap().bodies[&id].activity.repair_q, 8);
}
#[test]
fn life_cost_natural_synthesis_is_atomic_and_preserves_parent_child_body_and_energy() {
    let (mut w, s, _) = fixture(UnitKind::Spitter, 100);
    // 真实自然出生要求已有完工孵化设施的supply；这里仅构造该前置设施夹具。
    w.insert_building(OWNER, BuildingKind::SpawningVat, (70, 70), true);
    let r = w
        .config
        .ecology
        .as_mut()
        .unwrap()
        .roles
        .get_mut(&UnitKind::Spitter)
        .unwrap();
    r.natural_reproduction = true;
    r.birth_synthesis_milli = 10;
    w.step_role_supply();
    let retain = 4000;
    let allocation = 4_000_000;
    let fee = 40_000;
    let a = &mut w.ecology.as_mut().unwrap().bodies.get_mut(&s).unwrap().account;
    a.dissipate_q(a.stored_q - (retain + allocation + fee - 1));
    let before = w.ecology.as_ref().unwrap().clone();
    let mut occupied = w.units.values().map(|u| u.tile()).collect();
    w.try_mitosis(s, retain, &mut occupied);
    assert_eq!(w.ecology.as_ref().unwrap(), &before);
    w.step_role_supply();
    let before = w.ecology.as_ref().unwrap().bodies[&s].account.clone();
    let (hp, shield) = (w.units[&s].defense.hp_milli, w.units[&s].defense.shield_milli);
    w.try_mitosis(s, retain, &mut occupied);
    let e = w.ecology.as_ref().unwrap();
    let child = *e.births.keys().next().expect("真实分裂");
    assert_eq!(before.stored_q - e.bodies[&s].account.stored_q, allocation + fee);
    assert_eq!(e.bodies[&s].activity.synthesis_q, fee);
    assert_eq!(e.bodies[&child].account.stored_q, allocation);
    assert_eq!(hp, w.units[&s].defense.hp_milli + w.units[&child].defense.hp_milli);
    assert_eq!(
        shield,
        w.units[&s].defense.shield_milli + w.units[&child].defense.shield_milli
    );
    let a = ArenaRuntime::new([39; 16], w);
    let cp = a.checkpoint().unwrap();
    let b = ArenaRuntime::restore([39; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    assert_eq!(a.state_root(), b.state_root());
}

#[test]
fn life_cost_emit_signed_saturation_budget_and_dead_sources() {
    use crate::genome::action::EMIT;
    for (action, start, end) in [(i16::MAX, 12_798, 12_800), (i16::MIN, -12_798, -12_800)] {
        let (mut w, s, _) = fixture(UnitKind::Striker, 21);
        w.units.get_mut(&s).unwrap().actions[EMIT] = action;
        w.fields.set(FieldId::SignalA, 44, 44, start);
        w.step_signal_emission();
        assert_eq!(w.fields.get(FieldId::SignalA, 44, 44), end);
        let b = &w.ecology.as_ref().unwrap().bodies[&s];
        assert_eq!((b.account.stored_q, b.activity.secretion_q), (5, 16));
        assert!(b.account.is_conserved());
        let before = b.clone();
        w.step_signal_emission(); // 满场不收请求量费用
        assert_eq!(w.ecology.as_ref().unwrap().bodies[&s], before);
        w.fields.set(FieldId::SignalA, 44, 44, 0);
        w.step_signal_emission(); // 余款不足一个量子，保留原池
        assert_eq!(w.fields.get(FieldId::SignalA, 44, 44), 0);
        assert_eq!(w.ecology.as_ref().unwrap().bodies[&s], before);
    }
    let (mut w, s, wild) = fixture(UnitKind::Striker, 2048);
    w.units.get_mut(&wild).unwrap().actions[EMIT] = 256;
    w.units.get_mut(&s).unwrap().actions[EMIT] = 0;
    w.step_signal_emission();
    assert_eq!(w.fields.get(FieldId::SignalA, 46, 44), 0, "野生角色没有显式供能契约");
    assert_eq!(w.ecology.as_ref().unwrap().bodies[&s].activity.secretion_q, 0);
    w.units.get_mut(&s).unwrap().actions[EMIT] = 256;
    w.units.get_mut(&s).unwrap().defense.hp_milli = 0;
    w.step_signal_emission();
    assert_eq!(w.fields.get(FieldId::SignalA, 44, 44), 0);
}

#[test]
fn life_cost_emit_real_tick_checkpoint_wal_and_exhaustion() {
    use crate::genome::action::EMIT;
    let (mut w, s, _) = fixture(UnitKind::Striker, 10_000);
    w.units.get_mut(&s).unwrap().actions = [0, 0, -256, 0, 0, 256];
    w.faction_mut(OWNER).unwrap().ledger.balance.biomass = 0;
    w.fields.set(FieldId::SignalA, 44, 44, 0);
    let dir = tempfile::TempDir::new().unwrap();
    let snap = dir.path().join("emit.rsnap");
    let log = dir.path().join("emit.rjournal");
    let id = [42; 16];
    let out = w.save_snapshot(&snap, id, 1, [0; 32]).unwrap();
    let mut parent = out.state_root;
    let mut writer = JournalWriter::create(&log, id, w.tick, 1, w.tick).unwrap();
    w.step_with_journal(&mut writer, id, 2, &mut parent).unwrap();
    assert_eq!(
        w.fields.get(FieldId::SignalA, 44, 44),
        256,
        "真实非脑更新Tick消费原动作"
    );
    assert_eq!(w.ecology.as_ref().unwrap().bodies[&s].activity.secretion_q, 2048);
    writer.append_torn_bytes(&[0xfe]).unwrap();
    let (recovered, result) = ArenaWorld::recover_from_snapshot_and_journal(&snap, &log, id).unwrap();
    assert!(result.torn_write_detected);
    assert_eq!(recovered.canonical_state(), w.canonical_state());
    let (mut w, s, _) = fixture(UnitKind::Striker, 8);
    w.units.get_mut(&s).unwrap().actions[EMIT] = 256;
    w.step_signal_emission();
    assert_eq!(w.fields.get(FieldId::SignalA, 44, 44), 1);
    w.retire_activity_exhausted();
    assert!(!w.units.contains_key(&s));
    assert_eq!(w.ecology.as_ref().unwrap().remains[&s].activity.secretion_q, 8);
    ArenaRuntime::new(id, w).checkpoint().unwrap();
}

#[test]
fn life_cost_continuous_brain_pays_real_updates_and_rejects_atomically() {
    for (remaining, succeeds) in [(1023, false), (1024, true), (10_000, true)] {
        let (mut w, s, _) = fixture(UnitKind::Carapace, remaining);
        w.config
            .ecology
            .as_mut()
            .unwrap()
            .roles
            .get_mut(&UnitKind::Carapace)
            .unwrap()
            .continuous_cost_micro_per_neuron_step = 2;
        let expected = 32 * 4 * 2 * 4; // 32隐藏神经元 × 4真实子步 × 2micro × 4q
        assert_eq!(w.continuous_neural_q(s), expected);
        let brain = w.units[&s].brain.state_root();
        let account = w.ecology.as_ref().unwrap().bodies[&s].account.clone();
        w.step_brains();
        let body = &w.ecology.as_ref().unwrap().bodies[&s];
        assert!(body.account.is_conserved());
        if succeeds {
            assert_eq!(body.activity.continuous_q, expected);
            assert_eq!(body.account.stored_q, account.stored_q - expected);
            assert_eq!(body.account.dissipated_q, account.dissipated_q + expected);
            assert_ne!(w.units[&s].brain.state_root(), brain);
        } else {
            assert_eq!(body.account, account);
            assert_eq!(body.activity.continuous_q, 0);
            assert_eq!(w.units[&s].brain.state_root(), brain);
            assert_eq!(w.units[&s].actions, [0; 6]);
        }
        w.retire_activity_exhausted();
        if remaining == 1024 {
            assert_eq!(w.ecology.as_ref().unwrap().remains[&s].activity.continuous_q, expected);
        }
        let runtime = ArenaRuntime::new([42; 16], w);
        let cp = runtime.checkpoint().unwrap();
        let restored = ArenaRuntime::restore([42; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
        assert_eq!(runtime.world().canonical_state(), restored.world().canonical_state());
    }
}

#[test]
fn life_cost_emit_consumes_real_brain_readout_and_world_field() {
    use crate::genome::{action::EMIT, sensor};
    let (mut w, s, _) = fixture(UnitKind::Weaver, 1_000_000);
    // 定向基因夹具：原CTRNN的敌方在射程感知→首隐藏神经元→原第六动作。
    let u = w.units.get_mut(&s).unwrap();
    u.genome.w_in.fill(0);
    u.genome.w_rec.fill(0);
    u.genome.w_out.fill(0);
    u.genome.w_in[sensor::ENEMY_IN_RANGE] = 512;
    u.genome.w_out[EMIT * u.genome.n_hid as usize] = 512;
    u.brain = crate::brain::UnitBrain::from_genome(&u.genome).unwrap();
    w.step_brains();
    let emitted = w.units[&s].actions[EMIT];
    assert!(emitted > 0);
    w.step_signal_emission();
    assert_eq!(w.fields.get(FieldId::SignalA, 44, 44), emitted);
    assert_eq!(
        w.ecology.as_ref().unwrap().bodies[&s].activity.secretion_q,
        emitted as u64 * 8
    );
}
