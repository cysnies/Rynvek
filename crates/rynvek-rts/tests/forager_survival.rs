// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::commands::{RtsCommand, UnitSelector};
use rynvek_rts::{ArenaMap, ArenaRuntime, ArenaWorld, FactionId, FieldId, MatchConfig, TerrainTileKind, UnitKind};

fn contract(cost: u32, reserve: u32) -> serde_json::Value {
    serde_json::json!({"activation":"after_first_uptake","basal_cost_micro_per_tick":cost,"reserve_ticks":reserve})
}
fn world(cost: u32, reserve: u32) -> (ArenaWorld, rynvek_rts::EntitySerial) {
    let mut cfg = serde_json::to_value(MatchConfig::pve(57, 1)).unwrap();
    cfg["ecology"] = serde_json::json!("forager_budget_v2");
    cfg["forager_survival"] = contract(cost, reserve);
    let mut w = ArenaWorld::with_map(
        serde_json::from_value(cfg).unwrap(),
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
    );
    let id = *w.units.iter().find(|(_, u)| u.kind == UnitKind::Forager).unwrap().0;
    let target = w.units[&id].position;
    w.apply_command(
        FactionId(0),
        &RtsCommand::IssueOrder {
            units: UnitSelector::Archetype(Some(UnitKind::Forager)),
            order: rynvek_rts::OrderKind::Harvest,
            target,
        },
    )
    .unwrap();
    // 受控闭合交换步：真实场只有一个量子，无初始个体注资；避开独立扩散/再生阶段。
    for y in 0..96 {
        for x in 0..96 {
            w.fields.set(FieldId::Nutrient, x, y, 0);
        }
    }
    let tile = w.units[&id].tile();
    w.fields.set(FieldId::Nutrient, tile.0, tile.1, 1);
    w.tick = 1;
    (w, id)
}

#[test]
fn real_uptake_then_basal_exhaustion_causes_terminal_conserved_death() {
    let (mut w, id) = world(1000, 0);
    let tile = w.units[&id].tile();
    w.step();
    assert!(w.units.contains_key(&id));
    let a = &w.ecology.as_ref().unwrap().bodies[&id].account;
    assert_eq!(a.received_q, 15625);
    assert_eq!(a.dissipated_q, 4000, "基础费必须真实支付而非只加参数");
    assert_eq!(a.exported_q, 8000);
    assert_eq!(a.stored_q, 3625);
    assert!(a.is_conserved());
    assert_eq!(w.fields.get(FieldId::Nutrient, tile.0.into(), tile.1.into()), 0);
    w.step();
    assert!(
        !w.units.contains_key(&id),
        "已激活且耗尽者必须走真实死亡，不是HP不变/计算暂停"
    );
    let dead = w.ecology.as_ref().unwrap().remains[&id].account.clone();
    assert_eq!(dead.stored_q, 0);
    assert_eq!(dead.dissipated_q, 7625);
    assert_eq!(dead.exported_q, 8000);
    assert_eq!(dead.returned_q, 0);
    assert!(dead.is_conserved());
    // 场减少的一个量子严格等于资源转出+热耗；不能从躯体购买价制造遗骸营养。
    assert_eq!(15625, dead.exported_q + dead.dissipated_q);
    w.fields.set(FieldId::Nutrient, tile.0, tile.1, 1000);
    w.run(8);
    assert!(!w.units.contains_key(&id), "死亡后重新有食物不得复活旧实体");
    assert_eq!(w.ecology.as_ref().unwrap().remains[&id].account, dead);
}

#[test]
fn failed_transaction_rolls_back_basal_charge_and_death_before_retry() {
    let (mut w, id) = world(1000, 0);
    w.step();
    let mut a = ArenaRuntime::new([82; 16], w);
    let mut control = a.clone();
    let before = a.state_root();
    let result = a.transact_at(a.tick + 2, |_| {
        Err::<(), _>(rynvek_core::RynvekErrorCode::SimulationInvariantViolated)
    });
    assert!(result.is_err());
    assert_eq!(a.state_root(), before);
    assert!(a.units.contains_key(&id));
    a.step().unwrap();
    control.step().unwrap();
    assert_eq!(a.state_root(), control.state_root());
    assert_eq!(a.ecology, control.ecology);
    assert!(!a.units.contains_key(&id));
}

#[test]
fn snapshot_restores_activated_basal_contract_across_the_death_boundary() {
    let (mut w, id) = world(1000, 0);
    w.step();
    let mut a = ArenaRuntime::new([83; 16], w);
    let cp = a.checkpoint().unwrap();
    let mut b = ArenaRuntime::restore([83; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    for _ in 0..7 {
        a.step().unwrap();
        b.step().unwrap();
        assert_eq!(a.state_root(), b.state_root());
        assert_eq!(a.ecology, b.ecology);
    }
    assert!(!a.units.contains_key(&id));
}

#[test]
fn never_fed_is_explicitly_unactivated_not_given_free_birth_energy() {
    let (mut w, id) = world(1000, 4);
    let tile = w.units[&id].tile();
    w.fields.set(FieldId::Nutrient, tile.0, tile.1, 0);
    w.step();
    assert!(w.units.contains_key(&id));
    assert!(
        w.ecology
            .as_ref()
            .unwrap()
            .bodies
            .values()
            .all(|b| b.account.received_q == 0 && b.account.stored_q == 0 && b.account.dissipated_q == 0)
    );
    let cfg = serde_json::to_value(&w.config).unwrap();
    assert_eq!(cfg["forager_survival"], contract(1000, 4));
}

#[test]
fn survival_parameters_are_bound_validated_and_not_implicitly_enabled() {
    let (w, _) = world(1000, 4);
    let cfg = serde_json::to_value(&w.config).unwrap();
    let root = w.canonical_state().config_root;
    let mut changed = w.clone();
    changed
        .config
        .forager_survival
        .as_mut()
        .unwrap()
        .basal_cost_micro_per_tick += 1;
    assert_ne!(changed.canonical_state().config_root, root);
    for bad in [
        serde_json::Value::Null,
        serde_json::json!({"activation":"after_first_uptake","reserve_ticks":4}),
        contract(0, 4),
        contract(u32::MAX, u32::MAX),
    ] {
        let mut value = cfg.clone();
        value["forager_survival"] = bad.clone();
        if bad.is_null() {
            assert!(
                serde_json::from_value::<MatchConfig>(value)
                    .unwrap()
                    .forager_survival
                    .is_none()
            );
        } else {
            assert!(serde_json::from_value::<MatchConfig>(value).is_err());
        }
    }
    for version in [serde_json::Value::Null, serde_json::json!("energy_exchange_v1")] {
        let mut value = cfg.clone();
        value["ecology"] = version;
        assert!(serde_json::from_value::<MatchConfig>(value).is_err());
    }
    assert!(MatchConfig::pve(57, 1).forager_survival.is_none());
    assert!(MatchConfig::duel(57).forager_survival.is_none());
}

#[test]
fn combat_death_still_recycles_only_real_unspent_reserve_at_both_rates() {
    for superconduct in [false, true] {
        let (mut w, id) = world(1, 50_000);
        let tile = w.units[&id].tile();
        w.fields.set(FieldId::Nutrient, tile.0, tile.1, 32);
        w.step();
        let before = w.ecology.as_ref().unwrap().bodies[&id].account.clone();
        assert!(before.stored_q > 15625);
        if superconduct {
            let f = w.faction_mut(FactionId(0)).unwrap();
            f.techs = f.techs.with(rynvek_rts::TechId::MetabolicSuperconduct);
        }
        w.units.get_mut(&id).unwrap().defense.hp_milli = 0;
        w.step();
        let dead = &w.ecology.as_ref().unwrap().remains[&id].account;
        let recycled = (u128::from(before.stored_q) * if superconduct { 90 } else { 75 } / 100) as u64;
        assert_eq!(dead.dissipated_q, before.dissipated_q + before.stored_q - recycled);
        assert_eq!(dead.stored_q + dead.returned_q, recycled);
        assert!(dead.returned_q > 0);
        assert!(dead.is_conserved());
    }
}

#[test]
fn validly_resealed_snapshot_cannot_reprice_or_drop_survival_contract() {
    use rynvek_snapshot::{CONF_TAG, SnapshotReader, SnapshotWriter};
    let (w, _) = world(1000, 4);
    let (bytes, _) = w.encode_snapshot([84; 16], 1, [0; 32]).unwrap();
    let read = SnapshotReader::read_bytes(&bytes).unwrap();
    for remove in [false, true] {
        let mut chunks = read.custom_chunks.clone();
        let conf = chunks.iter_mut().find(|c| c.tag == CONF_TAG).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&conf.payload).unwrap();
        if remove {
            value["config"].as_object_mut().unwrap().remove("forager_survival");
        } else {
            value["config"]["forager_survival"]["basal_cost_micro_per_tick"] = serde_json::json!(1001);
        }
        conf.payload = serde_json::to_vec(&value).unwrap();
        let (forged, _) = SnapshotWriter::write_bytes_with_chunks(
            [84; 16],
            w.tick,
            1,
            [0; 32],
            &read.registry,
            read.neur.as_ref(),
            &chunks,
        )
        .unwrap();
        SnapshotReader::read_bytes(&forged).expect("通用容器合法，不应靠坏校验和伪造领域拒绝证据");
        assert!(
            ArenaWorld::decode_snapshot(&forged).is_err(),
            "改价/移除契约必须因规范根绑定拒绝而不是静默降级"
        );
    }
}

#[test]
fn exhausted_activated_body_cannot_be_checkpointed_as_alive() {
    let (mut w, id) = world(1000, 0);
    w.step();
    let a = &mut w.ecology.as_mut().unwrap().bodies.get_mut(&id).unwrap().account;
    a.dissipate_micro(u32::MAX);
    assert!(a.is_conserved());
    assert_eq!(a.stored_q, 0);
    assert!(
        w.encode_snapshot([85; 16], 1, [0; 32]).is_err(),
        "稳定检查点不能把已激活耗尽个体保存成活体，待后续食物复活"
    );
}
