// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::{ArenaMap, ArenaWorld, MatchConfig, TerrainTileKind, UnitKind};
fn world() -> ArenaWorld {
    let mut c = serde_json::to_value(MatchConfig::pve(57, 1)).unwrap();
    c["ecology"] = serde_json::json!({"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"replication_mutation_rate_permille":50}}});
    ArenaWorld::with_map(
        serde_json::from_value(c).unwrap(),
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
    )
}
#[test]
fn real_local_nutrient_boots_unfunded_lif_without_manual_harvest_or_birth_grant() {
    let mut w = world();
    let id = *w.units.iter().find(|(_, u)| u.kind == UnitKind::Forager).unwrap().0;
    let u = w.units.get_mut(&id).unwrap();
    let mut g = u.genome.clone();
    g.model = rynvek_neural::NeuronModelKind::LIF;
    g.w_in.fill(256);
    u.brain = rynvek_rts::brain::UnitBrain::from_genome(&g).unwrap();
    u.genome = g;
    let before = u.brain.state_root();
    assert_eq!(w.ecology.as_ref().unwrap().bodies[&id].account.received_q, 0);
    assert_eq!(w.units[&id].order.kind, rynvek_rts::OrderKind::HoldPosition);
    w.step();
    let a = &w.ecology.as_ref().unwrap().bodies[&id].account;
    assert!(
        a.received_q > 0,
        "无Harvest命令/免费出生池也必须先从真实局部场获得来源能量"
    );
    assert_ne!(
        w.units[&id].brain.state_root(),
        before,
        "先取能才能支付并提交首次真实LIF脑Epoch"
    );
    assert!(a.dissipated_q > 4000, "除基础代谢外还须实际支付神经发放");
    assert_eq!(a.unfunded_q, 0);
    assert!(a.is_conserved());
}

#[test]
fn real_vat_birth_pays_production_once_then_feeds_from_local_field_and_restores() {
    use rynvek_rts::commands::{CommandAck, RtsCommand};
    use rynvek_rts::{ArenaRuntime, BuildingKind, FactionId, FieldId};
    let mut w = world();
    let faction = FactionId(0);
    let kind = BuildingKind::SpawningVat;
    let origin = (0..96)
        .flat_map(|y| (0..96).map(move |x| (x, y)))
        .find(|&p| w.validate_placement(faction, kind, p).is_ok())
        .unwrap();
    let CommandAck::BuildingPlaced { id: vat } = w
        .apply_command(faction, &RtsCommand::PlaceBuilding { kind, origin })
        .unwrap()
    else {
        panic!("真实放置应返回孵化槽")
    };
    for _ in 0..1000 {
        if w.buildings[&vat].complete {
            break;
        }
        w.step();
    }
    assert!(w.buildings[&vat].complete, "使用真实建设时钟，不直接改完工状态");
    // 只提供一份既有冻结购买成本，两个排队请求不得透支；购买不等于给躯体注资。
    let cost = rynvek_rts::units::unit_spec(UnitKind::Forager).cost;
    w.faction_mut(faction).unwrap().ledger.balance = cost;
    let command = RtsCommand::QueueUnit {
        vat,
        unit: UnitKind::Forager,
        genome_slug: "seed.forager.v1".into(),
        count: 1,
    };
    let mut runtime = ArenaRuntime::new([97; 16], w);
    runtime.command(faction, &command).unwrap();
    let paid = runtime.checkpoint().unwrap();
    assert_eq!(runtime.faction(faction).unwrap().ledger.balance.biomass, 0);
    assert!(runtime.command(faction, &command).is_err());
    assert_eq!(
        runtime.checkpoint().unwrap().bytes,
        paid.bytes,
        "资源失败不能多出生、多扣款或改变真实场"
    );
    for _ in 0..300 {
        if runtime.buildings[&vat].queue.front().unwrap().remaining_ticks == 1 {
            break;
        }
        runtime.step().unwrap();
    }
    assert_eq!(
        runtime.buildings[&vat].queue.front().unwrap().remaining_ticks,
        1,
        "真实供电/孵化必须在有界Tick内推进"
    );
    let existing: Vec<_> = runtime.units.keys().copied().collect();
    let cp = runtime.checkpoint().unwrap();
    let mut restored = ArenaRuntime::restore([97; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    runtime.step().unwrap();
    restored.step().unwrap();
    assert_eq!(runtime.state_root(), restored.state_root());
    let born = *runtime
        .units
        .keys()
        .find(|id| !existing.contains(id))
        .expect("真实孵化队列必须出生");
    let body = &runtime.ecology.as_ref().unwrap().bodies[&born].account;
    assert!(body.received_q > 0, "孵化当Tick无需Harvest即可从已有局部营养首摄食");
    assert_eq!(
        (body.received_q - 1000 * 4000) % 15_625,
        0,
        "扣除真实生产池支付的出生预算后，额外收入只能是环境场量子"
    );
    assert_eq!(body.unfunded_q, 0);
    assert!(body.is_conserved());
    assert_eq!(runtime.units[&born].order.kind, rynvek_rts::OrderKind::HoldPosition);
    assert!(
        runtime.fields.get(
            FieldId::Nutrient,
            runtime.units[&born].tile().0.into(),
            runtime.units[&born].tile().1.into()
        ) >= 0
    );
    for _ in 0..12 {
        runtime.step().unwrap();
        restored.step().unwrap();
        assert_eq!(runtime.state_root(), restored.state_root());
    }
}

#[test]
fn early_uptake_is_not_repeated_by_later_harvest_and_failed_tick_is_atomic() {
    use rynvek_rts::commands::{RtsCommand, UnitSelector};
    use rynvek_rts::{ArenaRuntime, FactionId, FieldId};
    let mut w = world();
    w.tick = 1; // 单独核对既有34/35量子采集上限，不混入独立场步。
    let id = *w.units.iter().find(|(_, u)| u.kind == UnitKind::Forager).unwrap().0;
    let target = w.units[&id].position;
    w.apply_command(
        FactionId(0),
        &RtsCommand::IssueOrder {
            units: UnitSelector::Ids(vec![id]),
            order: rynvek_rts::OrderKind::Harvest,
            target,
        },
    )
    .unwrap();
    let tile = w.units[&id].tile();
    let before = w.fields.get(FieldId::Nutrient, tile.0.into(), tile.1.into());
    let mut runtime = ArenaRuntime::new([98; 16], w);
    let cp = runtime.checkpoint().unwrap();
    assert!(
        runtime
            .transact_at(runtime.tick + 2, |_| Err::<(), _>(
                rynvek_core::RynvekErrorCode::SimulationInvariantViolated
            ))
            .is_err()
    );
    assert_eq!(runtime.checkpoint().unwrap().bytes, cp.bytes);
    runtime.step().unwrap();
    let received = runtime.ecology.as_ref().unwrap().bodies[&id].account.received_q;
    assert!(
        (34 * 15_625..=35 * 15_625).contains(&received),
        "早摄食加Harvest不能双领速率"
    );
    assert_eq!(
        i64::from(before - runtime.fields.get(FieldId::Nutrient, tile.0.into(), tile.1.into())) * 15_625,
        received as i64
    );
}
