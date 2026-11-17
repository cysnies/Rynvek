// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::commands::{CommandAck, RtsCommand};
use rynvek_rts::world::ecology::{EcologyContract, RoleEnergyContract};
use rynvek_rts::{
    ArenaMap, ArenaRuntime, ArenaWorld, BuildingKind, EntitySerial, FactionId, MatchConfig, TerrainTileKind, UnitKind,
};
const BIRTH_MILLI: i64 = 1000;
const PLAYER: FactionId = FactionId(0);

fn ready_world(vats: usize) -> (ArenaWorld, Vec<EntitySerial>) {
    let mut config = MatchConfig::pve(57, 1);
    config.ecology = Some(EcologyContract::single(
        rynvek_rts::UnitKind::Forager,
        RoleEnergyContract {
            basal_cost_micro_per_tick: 1000,
            reserve_ticks: 1000,
            birth_energy_milli: 1000,
            movement_cost_micro_per_tile: 0,
            synapse_maintenance_micro_per_tick: 0,
            natural_reproduction: true,
            replication_mutation_rate_permille: 50,
        },
    ));
    // 无营养地形，不依靠Move、Harvest或测试注入身体能量掩盖出生空池。
    let mut w = ArenaWorld::with_map(
        config,
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::ConductiveBasalt; 96 * 96]),
    );
    let g = w
        .faction_mut(PLAYER)
        .unwrap()
        .genomes
        .get_mut("seed.forager.v1")
        .unwrap();
    g.model = rynvek_neural::NeuronModelKind::LIF;
    g.w_in.fill(256);
    let mut ids = Vec::new();
    for _ in 0..vats {
        let origin = (0..96)
            .flat_map(|y| (0..96).map(move |x| (x, y)))
            .find(|&p| w.validate_placement(PLAYER, BuildingKind::SpawningVat, p).is_ok())
            .unwrap();
        let CommandAck::BuildingPlaced { id } = w
            .apply_command(
                PLAYER,
                &RtsCommand::PlaceBuilding {
                    kind: BuildingKind::SpawningVat,
                    origin,
                },
            )
            .unwrap()
        else {
            panic!("真实建设")
        };
        ids.push(id);
    }
    for _ in 0..1000 {
        if ids.iter().all(|id| w.buildings[id].complete) {
            break;
        }
        w.step();
    }
    assert!(ids.iter().all(|id| w.buildings[id].complete));
    for &vat in &ids {
        w.apply_command(
            PLAYER,
            &RtsCommand::QueueUnit {
                vat,
                unit: UnitKind::Forager,
                genome_slug: "seed.forager.v1".into(),
                count: 1,
            },
        )
        .unwrap();
    }
    for _ in 0..300 {
        if ids
            .iter()
            .all(|id| w.buildings[id].queue.front().unwrap().remaining_ticks == 1)
        {
            break;
        }
        w.step();
    }
    assert!(
        ids.iter()
            .all(|id| w.buildings[id].queue.front().unwrap().remaining_ticks == 1)
    );
    (w, ids)
}

#[test]
fn unfed_vat_birth_is_paid_from_actual_biomass_then_brain_and_basal_are_real() {
    let (mut w, _) = ready_world(1);
    w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = BIRTH_MILLI;
    let old: Vec<_> = w.units.keys().copied().collect();
    w.step();
    let born = *w.units.keys().find(|id| !old.contains(id)).unwrap();
    let a = &w.ecology.as_ref().unwrap().bodies[&born].account;
    assert_eq!(
        a.received_q,
        BIRTH_MILLI as u64 * 4000,
        "出生不能仍为空池或凭空注资，必须获得已扣生产池能量"
    );
    assert_eq!(
        w.faction(PLAYER).unwrap().ledger.balance.biomass * 4000 - a.exported_q as i64,
        0,
        "真实母池付款减去同源转回，不能既保留余额又免费发出生能量"
    );
    let root = w.units[&born].brain.state_root();
    for _ in 0..6 {
        w.step();
    }
    assert_ne!(
        w.units[&born].brain.state_root(),
        root,
        "贫营养出生也应能用已付预算提交真实LIF，而非靠免费移动取巧"
    );
    let a = &w.ecology.as_ref().unwrap().bodies[&born].account;
    assert!(a.dissipated_q > 7 * 4000);
    assert_eq!(a.unfunded_q, 0);
    assert!(a.is_conserved());
}

#[test]
fn insufficient_birth_supply_neither_spawns_nor_debits_and_retry_is_exact() {
    let (mut w, ids) = ready_world(1);
    w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = BIRTH_MILLI - 1;
    let count = w.units.len();
    w.run(3);
    assert_eq!(w.units.len(), count, "造价已付不等于出生能量可免费生成");
    assert_eq!(w.faction(PLAYER).unwrap().ledger.balance.biomass, BIRTH_MILLI - 1);
    assert_eq!(w.buildings[&ids[0]].queue.front().unwrap().remaining_ticks, 1);
    w.faction_mut(PLAYER).unwrap().ledger.balance.biomass += 1;
    w.step();
    assert_eq!(w.units.len(), count + 1);
    assert!(w.buildings[&ids[0]].queue.is_empty());
}

#[test]
fn simultaneous_vats_cannot_overdraw_one_real_birth_budget() {
    let (mut w, ids) = ready_world(2);
    w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = BIRTH_MILLI;
    let old: Vec<_> = w.units.keys().copied().collect();
    w.step();
    let children: Vec<_> = w.units.keys().filter(|id| !old.contains(id)).copied().collect();
    assert_eq!(children.len(), 1, "真实同Tick两个生产者只能消费一份能量预算");
    assert!(w.buildings[&ids[0]].queue.is_empty());
    assert_eq!(w.buildings[&ids[1]].queue.front().unwrap().remaining_ticks, 1);
    let a = &w.ecology.as_ref().unwrap().bodies[&children[0]].account;
    assert_eq!(a.received_q, BIRTH_MILLI as u64 * 4000);
    assert_eq!(
        w.faction(PLAYER).unwrap().ledger.balance.biomass * 4000,
        a.exported_q as i64
    );
    assert!(a.is_conserved());
}

#[test]
fn birth_payment_failed_transaction_and_checkpoint_replay_are_atomic() {
    let (mut w, _) = ready_world(1);
    w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = BIRTH_MILLI;
    let old = w.units.len();
    let mut a = ArenaRuntime::new([101; 16], w);
    let cp = a.checkpoint().unwrap();
    assert!(
        a.transact_at(a.tick + 2, |_| Err::<(), _>(
            rynvek_core::RynvekErrorCode::SimulationInvariantViolated
        ))
        .is_err()
    );
    assert_eq!(
        a.checkpoint().unwrap().bytes,
        cp.bytes,
        "已经候选出生和扣款也必须整体撤销"
    );
    let mut b = ArenaRuntime::restore([101; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    a.step().unwrap();
    b.step().unwrap();
    assert_eq!(a.units.len(), old + 1);
    for _ in 0..18 {
        assert_eq!(a.state_root(), b.state_root());
        a.step().unwrap();
        b.step().unwrap();
    }
    assert_eq!(a.state_root(), b.state_root());
}

#[test]
fn birth_to_natural_death_and_combat_return_close_the_source_ledger() {
    for combat in [false, true] {
        let (mut w, _) = ready_world(1);
        w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = BIRTH_MILLI;
        let old: Vec<_> = w.units.keys().copied().collect();
        w.step();
        let child = *w.units.keys().find(|id| !old.contains(id)).unwrap();
        if combat {
            w.units.get_mut(&child).unwrap().defense.hp_milli = 0;
        }
        for _ in 0..512 {
            if !w.units.contains_key(&child) {
                break;
            }
            w.step();
        }
        assert!(
            !w.units.contains_key(&child),
            "有限真实出生能量不能让贫营养个体免费永生"
        );
        let a = &w.ecology.as_ref().unwrap().remains[&child].account;
        assert!(a.is_conserved());
        assert_eq!(a.received_q, BIRTH_MILLI as u64 * 4000);
        assert_eq!(a.unfunded_q, 0);
        let mut field_q = 0u64;
        for y in 0..96 {
            for x in 0..96 {
                field_q += w.fields.get(rynvek_rts::FieldId::Nutrient, x, y) as u64 * 15_625;
            }
        }
        assert_eq!(
            field_q, a.returned_q,
            "回收只来自实际未耗尽出生余额，不从原单位购买价造营养"
        );
        assert_eq!(
            BIRTH_MILLI as u64 * 4000,
            w.faction(PLAYER).unwrap().ledger.balance.biomass as u64 * 4000 + a.dissipated_q + a.stored_q + field_q
        );
        if combat {
            assert!(a.returned_q > 0);
        } else {
            assert_eq!(a.stored_q, 0);
            assert_eq!(a.returned_q, 0);
        }
    }
}

#[test]
fn unfunded_ready_job_cancels_without_charging_or_refunding_unpaid_birth_energy() {
    let (mut w, ids) = ready_world(1);
    w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = 0;
    w.step();
    let before = w.units.len();
    w.apply_command(PLAYER, &RtsCommand::CancelQueue { vat: ids[0] })
        .unwrap();
    assert!(w.buildings[&ids[0]].queue.is_empty());
    assert_eq!(w.units.len(), before);
    assert_eq!(
        w.faction(PLAYER).unwrap().ledger.balance.biomass,
        rynvek_rts::units::unit_spec(UnitKind::Forager).cost.biomass
    );
}
