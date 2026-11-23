// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::world::ecology::{EcologyContract, RoleEnergyContract};
use rynvek_rts::{ArenaMap, ArenaRuntime, ArenaWorld, FactionId, FieldId, MatchConfig, TerrainTileKind, UnitKind};

fn world() -> ArenaWorld {
    let mut config = MatchConfig::pve(57, 1);
    config.ecology = Some(EcologyContract::single(
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
    ArenaWorld::with_map(
        config,
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
    )
}

#[test]
fn uptake_is_real_conserved_and_resumes_per_commit() {
    let mut world = world();
    // 阵营容量已满，实际营养不能凭空消失，必须留在个体池；不改任何采集速率。
    let faction = world.faction_mut(FactionId(0)).unwrap();
    faction.ledger.balance.biomass = faction.ledger.capacity.biomass;
    world.run(9);
    let eco = world.ecology.as_ref().unwrap();
    assert!(eco.bodies.values().any(|b| b.account.stored_q > 0));
    for b in eco.bodies.values() {
        assert!(b.account.is_conserved());
    }
    let mut a = ArenaRuntime::new([57; 16], world);
    let checkpoint = a.checkpoint().unwrap();
    let mut b = ArenaRuntime::restore([57; 16], checkpoint.revision, checkpoint.state_root, &checkpoint.bytes).unwrap();
    for _ in 0..13 {
        a.step().unwrap();
        b.step().unwrap();
        assert_eq!(a.state_root(), b.state_root());
    }
    assert_eq!(a.world().ecology, b.world().ecology);
}

#[test]
fn energy_is_private_but_binds_authoritative_entity_root() {
    let mut world = world();
    let before = world.canonical_state();
    let observation = serde_json::to_vec(&world.observe(FactionId(0)).unwrap()).unwrap();
    let body = world.ecology.as_mut().unwrap().bodies.values_mut().next().unwrap();
    body.account.receive_field_raw(1);
    assert!(body.account.is_conserved());
    assert_ne!(world.canonical_state().entity_root, before.entity_root);
    assert_eq!(
        serde_json::to_vec(&world.observe(FactionId(0)).unwrap()).unwrap(),
        observation,
        "私有个体能量不可经现有观测泄漏"
    );
    let (bytes, _) = world.encode_snapshot([1; 16], 1, [0; 32]).unwrap();
    assert_eq!(ArenaWorld::decode_snapshot(&bytes).unwrap().ecology, world.ecology);
}

#[test]
fn actual_forager_brain_spikes_are_charged_at_the_frozen_cost() {
    let mut world = world();
    let id = *world.units.iter().find(|(_, u)| u.kind == UnitKind::Forager).unwrap().0;
    let mut genome = world.units[&id].genome.clone();
    genome.model = rynvek_neural::NeuronModelKind::LIF;
    genome.w_in.fill(256);
    let n = genome.n_hid as usize;
    genome.w_out[rynvek_rts::genome::action::HARVEST * n..(rynvek_rts::genome::action::HARVEST + 1) * n].fill(512);
    let position = world.units[&id].position;
    let unit = world.units.get_mut(&id).unwrap();
    unit.brain = rynvek_rts::brain::UnitBrain::from_genome(&genome).unwrap();
    unit.genome = genome;
    world
        .apply_command(
            FactionId(0),
            &rynvek_rts::commands::RtsCommand::IssueOrder {
                units: rynvek_rts::commands::UnitSelector::Archetype(Some(UnitKind::Forager)),
                order: rynvek_rts::OrderKind::Harvest,
                target: position,
            },
        )
        .unwrap();
    let mut superconduct = world.clone();
    let faction = superconduct.faction_mut(FactionId(0)).unwrap();
    faction.techs = faction.techs.with(rynvek_rts::TechId::MetabolicSuperconduct);
    world.step();
    superconduct.step();
    let body = &world.ecology.as_ref().unwrap().bodies[&id];
    let enhanced = &superconduct.ecology.as_ref().unwrap().bodies[&id];
    assert!(
        body.account.received_q > 0,
        "供能必须来自本Tick真实场扣除，而非测试注资"
    );
    assert_eq!(body.account.unfunded_q, 0);
    assert_eq!(
        (body.account.dissipated_q - 4000) * 3,
        (enhanced.account.dissipated_q - 4000) * 5,
        "超导科技必须消费同一真实发放数的0.48 milli成本"
    );
    assert!(
        body.account.dissipated_q > 4000,
        "必须由真实发放扣费，而非动作幅值或固定常数"
    );
    assert_eq!((body.account.dissipated_q - 4000) % (800 * 4), 0);
    assert!(body.account.is_conserved());
}

#[test]
fn death_returns_only_real_remaining_energy_and_keeps_clipped_residue() {
    let mut world = world();
    world.tick = 1; // 不跨越场扩散或脑 Epoch，只检查真实死亡结算。
    let id = *world.units.keys().next().unwrap();
    let tile = world.units[&id].tile();
    world.fields.set(FieldId::Nutrient, tile.0, tile.1, 25_600);
    world
        .ecology
        .as_mut()
        .unwrap()
        .bodies
        .get_mut(&id)
        .unwrap()
        .account
        .receive_field_raw(100);
    world.units.get_mut(&id).unwrap().defense.hp_milli = 0;
    world.step();
    let eco = world.ecology.as_ref().unwrap();
    assert!(!eco.bodies.contains_key(&id));
    let corpse = &eco.remains[&id];
    assert!(corpse.account.stored_q > 0, "满场截断不可吞掉尚未归还的营养");
    assert_eq!(corpse.account.dissipated_q, 100 * 15_625 / 4);
    assert!(corpse.account.is_conserved());
    let (bytes, _) = world.encode_snapshot([3; 16], 2, [0; 32]).unwrap();
    let mut restored = ArenaWorld::decode_snapshot(&bytes).unwrap();
    world.fields.set(FieldId::Nutrient, tile.0, tile.1, 0);
    restored.fields.set(FieldId::Nutrient, tile.0, tile.1, 0);
    world.step();
    restored.step();
    assert_eq!(world.state_hash(), restored.state_hash());
    assert_eq!(world.fields.get(FieldId::Nutrient, tile.0 as i64, tile.1 as i64), 75);
}

#[test]
fn retired_arena_snapshot_is_explicitly_unsupported() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/arena/energy-legacy.json")).unwrap();
    let hex = fixture["snapshot_hex"].as_str().unwrap();
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    assert!(
        ArenaWorld::decode_snapshot(&bytes).is_err(),
        "当前Arena格式显式拒绝旧编码，不默默升级"
    );
}

#[test]
fn ranked_cannot_opt_into_an_uncertified_ecology_version() {
    let mut value = serde_json::to_value(MatchConfig::duel(42)).unwrap();
    value["ecology"] = serde_json::json!({"wild":{"roles":{},"population_limit":0,"seeds":[]},"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"rule_cost_micro_per_gas":0,"material_capacity_milli":0,"material_growth_milli_per_tick":0,"material_synthesis_micro_per_milli":0,"continuous_cost_micro_per_neuron_step":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}});
    assert!(serde_json::from_value::<MatchConfig>(value).is_err());
}

#[test]
fn competing_foragers_transfer_exactly_the_available_field_quantum() {
    let mut world = world();
    world.tick = 1;
    // 只有这一个真实场量子；其他Forager自动摄食不能混入额外测试收入。
    for y in 0..96 {
        for x in 0..96 {
            world.fields.set(FieldId::Nutrient, x, y, 0);
        }
    }
    let ids: Vec<_> = world.units.keys().copied().take(2).collect();
    for id in ids {
        let u = world.units.get_mut(&id).unwrap();
        u.position = rynvek_rts::Vec2::from_milli(48_500, 48_500);
        u.actions[rynvek_rts::genome::action::HARVEST] = 256;
    }
    let f = world.faction_mut(FactionId(0)).unwrap();
    f.ledger.balance.biomass = f.ledger.capacity.biomass;
    world.fields.set(FieldId::Nutrient, 48, 48, 1);
    world.step();
    assert_eq!(world.fields.get(FieldId::Nutrient, 48, 48), 0);
    let accounts: Vec<_> = world
        .ecology
        .as_ref()
        .unwrap()
        .bodies
        .values()
        .map(|b| &b.account)
        .collect();
    assert_eq!(accounts.iter().map(|a| a.received_q).sum::<u64>(), 15_625);
    assert_eq!(accounts.iter().map(|a| a.dissipated_q).sum::<u64>(), 4_000);
    assert_eq!(accounts.iter().map(|a| a.stored_q).sum::<u64>(), 11_625);
    assert!(accounts.iter().all(|a| a.is_conserved()));
}

#[test]
fn snapshot_cannot_drop_or_forge_private_energy_even_with_valid_container_checksum() {
    use rynvek_snapshot::{SnapshotReader, SnapshotWriter};
    let world = world();
    let (bytes, _) = world.encode_snapshot([1; 16], 1, [0; 32]).unwrap();
    let read = SnapshotReader::read_bytes(&bytes).unwrap();
    for mode in [0, 1, 2] {
        let mut chunks = read.custom_chunks.clone();
        if mode == 0 {
            chunks.retain(|c| c.tag != *b"ECOL");
        } else {
            let c = chunks.iter_mut().find(|c| c.tag == *b"ECOL").unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&c.payload).unwrap();
            if mode == 1 {
                value["bodies"] = serde_json::json!({});
            } else {
                value["bodies"].as_object_mut().unwrap().values_mut().next().unwrap()["account"]["stored_q"] =
                    serde_json::json!(1);
            }
            c.payload = serde_json::to_vec(&value).unwrap();
        }
        let (forged, _) = SnapshotWriter::write_bytes_with_chunks(
            [1; 16],
            world.tick,
            1,
            [0; 32],
            &read.registry,
            read.neur.as_ref(),
            &chunks,
        )
        .unwrap();
        SnapshotReader::read_bytes(&forged).expect("重封装的通用容器校验应有效，必须由权威领域检查拒绝");
        assert!(
            ArenaWorld::decode_snapshot(&forged).is_err(),
            "不能由缺失块或不守恒池生成新权威状态，mode={mode}"
        );
    }
}

#[test]
fn retired_exchange_v1_snapshot_is_explicitly_unsupported() {
    let f: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/arena/energy-exchange-v1.json")).unwrap();
    let bytes: Vec<u8> = f["snapshot_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    assert!(
        ArenaWorld::decode_snapshot(&bytes).is_err(),
        "不再兼容已退役的V1生态分支"
    );
}
