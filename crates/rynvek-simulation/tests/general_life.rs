// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use rynvek_field::FieldChannelId;
use rynvek_neural::LIF_INPUTS;
use rynvek_neural::metabolism::ENERGY_Q_PER_FIELD_RAW;
use rynvek_simulation::engine::SimulationEngine;
use rynvek_simulation::life::{
    GeneralLifGenome, LIFE_PROFILE, LifeProfile, NutrientDeposit, nutrient_energy_q, universe_energy_q,
};
use rynvek_simulation::physics::{Body, ONE, PhysicsCommand as C};

fn body_at(x: i64, y: i64) -> Body {
    Body {
        position_q32: [x * ONE, y * ONE],
        velocity_q32: [0, 0],
        acceleration_q32: [0, 0],
        radius_q32: ONE / 2,
        mass_q32: ONE,
        fixed: false,
    }
}

fn profile(maintenance: u64, ingest: u32, recycle: u32, neural: Option<GeneralLifGenome>) -> LifeProfile {
    LifeProfile {
        profile: LIFE_PROFILE.into(),
        max_ingest_raw_per_tick: ingest,
        maintenance_q_per_tick: maintenance,
        recycle_percent: recycle,
        neural,
    }
}

fn life_world(bodies: &[Body], deposits: &[NutrientDeposit], profile: LifeProfile) -> SimulationEngine {
    SimulationEngine::with_bodies([9; 16], bodies)
        .unwrap()
        .with_sourced_life(profile, deposits)
        .unwrap()
}

fn assert_closed(world: &SimulationEngine) {
    let life = world.life().expect("生命世界");
    assert_eq!(universe_energy_q(life, &world.field).unwrap(), life.genesis_energy_q());
    assert_eq!(world.biomass, 0);
    assert_eq!(world.flux, 0);
    assert_eq!(world.neurogel, 0);
}

#[test]
fn sourced_ingest_is_exact_on_ticks_without_field_solve() {
    let deposits = [NutrientDeposit {
        x: 10,
        y: 64,
        value: 40,
    }];
    let mut world = life_world(&[body_at(10, 64)], &deposits, profile(0, 16, 100, None));
    assert_eq!(
        nutrient_energy_q(&world.field).unwrap(),
        40 * u128::from(ENERGY_Q_PER_FIELD_RAW)
    );
    assert_closed(&world);
    // Tick 1 会先做反应扩散，格值不再等于创世沉淀；Tick 2 无场步，摄取必须等额扣场。
    world.tick().unwrap();
    assert_closed(&world);
    assert_eq!(world.biomass, 0);
    let cell = world.field.interior(FieldChannelId::Nutrient, 10, 64).unwrap().max(0) as u32;
    let stored_before = world.life().unwrap().account(world.physics(), 1).unwrap().stored_q;
    world.tick().unwrap();
    let taken = cell.min(16);
    let account = world.life().unwrap().account(world.physics(), 1).unwrap();
    assert_eq!(
        account.stored_q,
        stored_before + u64::from(taken) * ENERGY_Q_PER_FIELD_RAW
    );
    assert_eq!(
        world.field.interior(FieldChannelId::Nutrient, 10, 64).unwrap(),
        (cell - taken) as i16
    );
    assert_closed(&world);
}

#[test]
fn fixed_body_does_not_ingest_and_world_stays_closed_after_field_decay() {
    let mut fixed = body_at(10, 64);
    fixed.fixed = true;
    let mut world = life_world(
        &[fixed],
        &[NutrientDeposit { x: 10, y: 64, value: 8 }],
        profile(1, 16, 100, None),
    );
    let genesis = nutrient_energy_q(&world.field).unwrap();
    world.tick().unwrap();
    assert_eq!(world.physics().iter().count(), 1);
    assert_eq!(world.life().unwrap().account(world.physics(), 1).unwrap().stored_q, 0);
    assert_eq!(world.life().unwrap().account(world.physics(), 1).unwrap().received_q, 0);
    assert!(universe_energy_q(world.life().unwrap(), &world.field).unwrap() <= genesis);
    assert_closed(&world);
}

#[test]
fn starvation_despawns_empty_accounts_without_recording_carcass_harvest() {
    let mut world = life_world(
        &[body_at(10, 64)],
        &[NutrientDeposit { x: 10, y: 64, value: 2 }],
        profile(256 * ENERGY_Q_PER_FIELD_RAW, 2, 50, None),
    );
    world.tick().unwrap();
    // 场步后即使还能摄取，超高维护也会在同 Tick 耗尽并死亡；回场按已空库存为 0，不记 carcass。
    assert_eq!(world.physics().iter().count(), 0);
    assert_closed(&world);
}

#[test]
fn command_despawn_returns_half_stored_raw_to_the_current_cell() {
    let mut world = life_world(
        &[body_at(10, 64)],
        &[NutrientDeposit {
            x: 10,
            y: 64,
            value: 40,
        }],
        profile(0, 8, 50, None),
    );
    world.tick().unwrap();
    world.tick().unwrap();
    let serial = world.physics().iter().next().unwrap().0;
    let stored = world.life().unwrap().account(world.physics(), serial).unwrap().stored_q;
    assert!(stored >= 2 * ENERGY_Q_PER_FIELD_RAW);
    let (x, y) = {
        let body = world.physics().body(serial).unwrap();
        (
            (body.position_q32[0] / ONE) as usize,
            (body.position_q32[1] / ONE) as usize,
        )
    };
    let before = world.field.interior(FieldChannelId::Nutrient, x, y).unwrap();
    let expect_raw = (stored / ENERGY_Q_PER_FIELD_RAW) / 2;
    world
        .tick_with_commands(&[C::Despawn { serial }])
        .expect("命令回收应返还残骸");
    assert_eq!(world.physics().iter().count(), 0);
    assert_eq!(
        world.field.interior(FieldChannelId::Nutrient, x, y).unwrap(),
        before + expect_raw as i16
    );
    assert_closed(&world);
}

#[test]
fn failed_tick_does_not_keep_ingest_or_advance_clock() {
    let mut world = life_world(
        &[body_at(10, 64)],
        &[NutrientDeposit {
            x: 10,
            y: 64,
            value: 20,
        }],
        profile(0, 8, 100, None),
    );
    let field = world.field.authority_bytes();
    let root = world.parent_commit_hash;
    let stored = world.life().unwrap().account(world.physics(), 1).unwrap().stored_q;
    world.diagnostics_nonzero_count = 1;
    assert!(world.tick().is_err());
    assert_eq!(world.clock.tick, 0);
    assert_eq!(world.parent_commit_hash, root);
    assert_eq!(world.field.authority_bytes(), field);
    assert_eq!(
        world.life().unwrap().account(world.physics(), 1).unwrap().stored_q,
        stored
    );
}

#[test]
fn snapshot_and_wal_restore_life_accounts_and_future_roots() {
    let mut world = life_world(
        &[body_at(10, 64)],
        &[NutrientDeposit {
            x: 10,
            y: 64,
            value: 32,
        }],
        profile(ENERGY_Q_PER_FIELD_RAW, 4, 80, None),
    );
    world.tick().unwrap();
    world.tick().unwrap();
    let bytes = world.encode_snapshot().unwrap();
    let mut restored = SimulationEngine::decode_snapshot(&bytes).unwrap();
    assert_eq!(restored.encode_snapshot().unwrap(), bytes);
    assert_closed(&restored);
    for _ in 0..6 {
        assert_eq!(
            world.tick().unwrap().state_root_hash,
            restored.tick().unwrap().state_root_hash
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let snap = directory.path().join("life.rsnap");
    let wal = directory.path().join("life.rjournal");
    let mut recorded = life_world(
        &[body_at(10, 64)],
        &[NutrientDeposit {
            x: 10,
            y: 64,
            value: 32,
        }],
        profile(ENERGY_Q_PER_FIELD_RAW, 4, 80, None),
    );
    std::fs::write(&snap, recorded.encode_snapshot().unwrap()).unwrap();
    let mut writer = rynvek_journal::JournalWriter::create_new(&wal, recorded.world_id, 0, 0, 0).unwrap();
    recorded.tick_recorded(&[], &mut writer).unwrap();
    recorded
        .tick_recorded(&[C::Despawn { serial: 1 }], &mut writer)
        .unwrap();
    drop(writer);
    let (replayed, out) = SimulationEngine::recover(&snap, &wal).unwrap();
    assert_eq!(out.replayed_commits.len(), 2);
    assert_eq!(replayed.parent_commit_hash, recorded.parent_commit_hash);
    assert_eq!(replayed.encode_snapshot().unwrap(), recorded.encode_snapshot().unwrap());
    assert_eq!(replayed.physics().iter().count(), 0);
    assert_closed(&replayed);
}

#[test]
fn unpaid_neural_does_not_keep_acceleration_and_funded_spikes_can_steer() {
    let n = 1usize;
    let mut weights = vec![0i16; n * (LIF_INPUTS + 6)];
    weights[0] = 12 * 256;
    weights[4] = 12 * 256;
    weights[LIF_INPUTS] = 12 * 256;
    let genome = GeneralLifGenome {
        hidden: 1,
        weights_base_q8: weights,
    };
    let mut unpaid = life_world(&[body_at(10, 64)], &[], profile(0, 0, 100, Some(genome.clone())));
    unpaid.tick().unwrap();
    let body = unpaid.physics().body(1).unwrap();
    assert_eq!(body.acceleration_q32, [0, 0]);
    assert_eq!(body.velocity_q32, [0, 0]);
    assert_eq!(
        unpaid
            .life()
            .unwrap()
            .account(unpaid.physics(), 1)
            .unwrap()
            .dissipated_q,
        0
    );

    let mut paid = life_world(
        &[body_at(10, 64)],
        &[NutrientDeposit {
            x: 10,
            y: 64,
            value: 64,
        }],
        profile(0, 64, 100, Some(genome)),
    );
    paid.tick().unwrap();
    paid.tick().unwrap();
    let body = paid.physics().body(1).unwrap();
    let account = paid.life().unwrap().account(paid.physics(), 1).unwrap();
    if body.acceleration_q32 != [0, 0] || body.velocity_q32 != [0, 0] {
        assert!(account.dissipated_q > 0);
    }
    assert_closed(&paid);
    assert_closed(&unpaid);
}

#[test]
fn physics_only_world_does_not_grow_a_life_chunk() {
    let world = SimulationEngine::with_bodies([2; 16], &[body_at(10, 64)]).unwrap();
    let bytes = world.encode_snapshot().unwrap();
    let read = rynvek_snapshot::SnapshotReader::read_bytes(&bytes).unwrap();
    assert_eq!(read.custom_chunks.len(), 4);
    assert!(read.find_chunk(*b"LIFE").is_none());
    assert!(read.neur.is_none());
}
