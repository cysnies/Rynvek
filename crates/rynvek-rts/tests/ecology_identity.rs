// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::world::ecology::{EcologyContract, RoleEnergyContract};
use rynvek_rts::{ArenaRuntime, ArenaWorld, MatchConfig};

fn config() -> MatchConfig {
    let mut c = MatchConfig::pve(57, 1);
    c.ecology = Some(EcologyContract::single(
        rynvek_rts::UnitKind::Forager,
        RoleEnergyContract {
            basal_cost_micro_per_tick: 1000,
            reserve_ticks: 4,
            birth_energy_milli: 1000,
            movement_cost_micro_per_tile: 0,
            synapse_maintenance_micro_per_tick: 0,
            natural_reproduction: true,
            attack_cost_micro_per_shot: 0,
            repair_cost_micro_per_milli: 0,
            rule_cost_micro_per_gas: 0,
            material_capacity_milli: 0,
            material_growth_milli_per_tick: 0,
            material_synthesis_micro_per_milli: 0,
            continuous_cost_micro_per_neuron_step: 0,
            secretion_cost_micro_per_field_raw: 0,
            birth_synthesis_milli: 0,
            activity_reserve_milli: 0,
            replication_mutation_rate_permille: 50,
        },
    ));
    c
}

#[test]
fn whole_current_contract_is_bound_and_transaction_cannot_reprice_it() {
    let w = ArenaWorld::new(config());
    let root = w.canonical_state().config_root;
    for variant in 0..4 {
        let mut changed = w.clone();
        match variant {
            0 => changed.config.ecology = None,
            1 => {
                changed
                    .config
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .get_mut(&rynvek_rts::UnitKind::Forager)
                    .unwrap()
                    .basal_cost_micro_per_tick += 1
            }
            2 => {
                changed
                    .config
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .get_mut(&rynvek_rts::UnitKind::Forager)
                    .unwrap()
                    .reserve_ticks += 1
            }
            _ => {
                changed
                    .config
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .get_mut(&rynvek_rts::UnitKind::Forager)
                    .unwrap()
                    .birth_energy_milli += 1
            }
        }
        assert_ne!(changed.canonical_state().config_root, root);
        let mut runtime = ArenaRuntime::new([91; 16], w.clone());
        let before = runtime.checkpoint().unwrap();
        assert!(
            runtime
                .transact_at(0, |world| {
                    world.config = changed.config.clone();
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(runtime.checkpoint().unwrap().bytes, before.bytes);
    }
    for field in ["ecology_identity", "forager_survival", "forager_uptake"] {
        let mut v = serde_json::to_value(config()).unwrap();
        v[field] = serde_json::json!(null);
        assert!(
            serde_json::from_value::<MatchConfig>(v).is_err(),
            "退役字段不得静默忽略"
        );
    }
}

#[test]
fn resealed_snapshot_rejects_cross_contract_and_unsupported_arena_format() {
    use rynvek_snapshot::{CONF_TAG, SnapshotReader, SnapshotWriter};
    let w = ArenaWorld::new(config());
    let (bytes, _) = w.encode_snapshot([92; 16], 0, [0; 32]).unwrap();
    let read = SnapshotReader::read_bytes(&bytes).unwrap();
    for variant in 0..6 {
        let mut chunks = read.custom_chunks.clone();
        let conf = chunks.iter_mut().find(|c| c.tag == CONF_TAG).unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&conf.payload).unwrap();
        match variant {
            0 => {
                v["config"].as_object_mut().unwrap().remove("ecology");
            }
            1 => v["config"]["ecology"]["roles"]["unit.forager"]["reserve_ticks"] = serde_json::json!(5),
            2 => v["config"]["ecology"]["roles"]["unit.forager"]["basal_cost_micro_per_tick"] = serde_json::json!(1001),
            3 => {
                v.as_object_mut().unwrap().remove("arena_format");
            }
            4 => v["arena_format"] = serde_json::json!("rynvek-arena/3"),
            _ => v["config"]["ecology"]["roles"]["unit.forager"]["birth_energy_milli"] = serde_json::json!(1001),
        }
        conf.payload = serde_json::to_vec(&v).unwrap();
        let (forged, _) = SnapshotWriter::write_bytes_with_chunks(
            [92; 16],
            w.tick,
            0,
            [0; 32],
            &read.registry,
            read.neur.as_ref(),
            &chunks,
        )
        .unwrap();
        SnapshotReader::read_bytes(&forged).expect("容器合法，领域仍必须拒绝");
        assert!(ArenaWorld::decode_snapshot(&forged).is_err());
    }
}

#[test]
fn current_fork_replays_without_migration_or_second_identity_source() {
    let mut a = ArenaRuntime::new([93; 16], ArenaWorld::new(config()))
        .fork([94; 16])
        .unwrap();
    let cp = a.checkpoint().unwrap();
    let mut b = ArenaRuntime::restore([94; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    for _ in 0..12 {
        a.step().unwrap();
        b.step().unwrap();
        assert_eq!(a.state_root(), b.state_root());
    }
    assert!(a.candidate_tick().unwrap().fork([95; 16]).is_err());
}
