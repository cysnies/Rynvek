// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_core::RynvekErrorCode;
use rynvek_rts::world::ecology::ForagerEcologyContract;
use rynvek_rts::{ArenaMap, ArenaRuntime, ArenaWorld, FactionId, MatchConfig, TerrainTileKind, UnitKind};

fn world() -> ArenaWorld {
    let mut config = MatchConfig::pve(57, 1);
    config.ecology = Some(ForagerEcologyContract {
        basal_cost_micro_per_tick: 1000,
        reserve_ticks: 4,
        birth_energy_milli: 1000,
    });
    let mut w = ArenaWorld::with_map(
        config,
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
    );
    let origin = (0..96)
        .flat_map(|y| (0..96).map(move |x| (x, y)))
        .find(|&p| {
            w.validate_placement(FactionId(0), rynvek_rts::BuildingKind::SpawningVat, p)
                .is_ok()
        })
        .unwrap();
    w.apply_command(
        FactionId(0),
        &rynvek_rts::commands::RtsCommand::PlaceBuilding {
            kind: rynvek_rts::BuildingKind::SpawningVat,
            origin,
        },
    )
    .unwrap();
    w
}

// 使用真实环境摄食积累；不向身体账户注入能量。只暂时封住邻格，以控制出生边界。
fn ready_parent_world() -> ArenaWorld {
    let mut w = world();
    let nav = w.nav.clone();
    for y in 0..96 {
        for x in 0..96 {
            w.nav.set_occupied(x, y, true);
        }
    }
    let serial = w.next_serial();
    for _ in 0..rynvek_rts::buildings::building_spec(rynvek_rts::BuildingKind::SpawningVat)
        .build_ticks
        .max(100)
    {
        w.step();
    }
    assert_eq!(w.next_serial(), serial, "无落点不能耗用序号或远程出生");
    assert!(w.ecology.as_ref().unwrap().births.is_empty());
    assert!(
        w.ecology
            .as_ref()
            .unwrap()
            .bodies
            .values()
            .all(|b| b.account.transferred_q == 0)
    );
    assert!(
        w.ecology
            .as_ref()
            .unwrap()
            .bodies
            .values()
            .any(|b| b.account.stored_q >= 8_000_000)
    );
    w.nav = nav;
    w
}

#[test]
fn field_funded_natural_birth_inherits_mutates_and_splits_actual_accounts_and_health() {
    let mut w = ready_parent_world();
    let parents = w.units.clone();
    let birth_tick = w.tick;
    let ledger = w.faction(FactionId(0)).unwrap().ledger.balance;
    w.step();
    let eco = w.ecology.as_ref().unwrap();
    assert!(
        !eco.births.is_empty(),
        "无需生产/定向繁育/Harvest命令，真实进食应能自然出生"
    );
    let mut spots = std::collections::BTreeSet::new();
    for (child, birth) in &eco.births {
        let parent = &parents[&birth.parent];
        let u = &w.units[child];
        assert!(spots.insert(u.tile()), "同时出生不能争用同一邻格");
        assert_eq!(u.kind, UnitKind::Forager);
        assert_eq!(birth.born_tick, birth_tick);
        assert_eq!(birth.parent_genome, parent.genome.digest());
        let mut expected = parent.genome.clone();
        expected.generation += 1;
        expected.slug = format!("mitosis.{}", child.0);
        let mut rng =
            rynvek_rts::evolution::EvolutionRng::for_mitosis(w.config.world_seed, birth_tick, u64::from(child.0));
        rynvek_rts::evolution::mutate(
            &mut expected,
            rynvek_rts::evolution::DEFAULT_MUTATION_RATE_PERMILLE,
            &mut rng,
        );
        assert_eq!(u.genome, expected);
        assert_eq!(birth.child_genome, expected.digest());
        assert_eq!(birth.generation, parent.genome.generation + 1);
        assert_eq!(
            u.brain.state_root(),
            rynvek_rts::brain::UnitBrain::from_genome(&expected)
                .unwrap()
                .state_root()
        );
        assert_eq!(eco.bodies[child].account.received_q, 4_000_000);
        assert_eq!(eco.bodies[&birth.parent].account.transferred_q, 4_000_000);
        assert!(eco.bodies[&birth.parent].account.stored_q >= 4_000_000);
        assert_eq!(
            u.defense.hp_milli + w.units[&birth.parent].defense.hp_milli,
            parent.defense.hp_milli,
            "不能复制满血战力"
        );
        assert_eq!(
            u.defense.shield_milli + w.units[&birth.parent].defense.shield_milli,
            parent.defense.shield_milli
        );
    }
    assert!(
        w.faction(FactionId(0)).unwrap().ledger.balance.biomass >= ledger.biomass,
        "自然出生不得再扣阵营生产池"
    );
    let transferred: u64 = eco.bodies.values().map(|b| b.account.transferred_q).sum();
    assert_eq!(transferred, eco.births.values().map(|b| b.allocated_q).sum::<u64>());
    assert!(eco.bodies.values().all(|b| b.account.is_conserved()));
    let public = serde_json::to_string(&w.observe(FactionId(0)).unwrap()).unwrap();
    for private in [
        "transferred_q",
        "parent_genome",
        "child_genome",
        "allocated_q",
        "births",
    ] {
        assert!(!public.contains(private));
    }
}

#[test]
fn insufficient_energy_and_exhausted_generation_or_serial_never_create_offspring() {
    let mut empty = world();
    let serial = empty.next_serial();
    empty.step();
    assert!(empty.ecology.as_ref().unwrap().births.is_empty());
    assert_eq!(empty.next_serial(), serial);
    let mut full = ready_parent_world();
    let faction = FactionId(0);
    for _ in 0..500 {
        let (used, cap) = full.supply(faction);
        if used == cap {
            break;
        }
        full.step();
    }
    let (used, cap) = full.supply(faction);
    assert_eq!(used, cap, "真实自然出生应能抵达既有RTS人口容量");
    let serial = full.next_serial();
    let births = full.ecology.as_ref().unwrap().births.len();
    full.step();
    assert_eq!(full.next_serial(), serial, "自然出生不能超出现有人口容量");
    assert_eq!(full.ecology.as_ref().unwrap().births.len(), births);
    for serial_exhausted in [false, true] {
        let mut w = ready_parent_world();
        if serial_exhausted {
            w.set_next_serial(u32::MAX);
        } else {
            for u in w.units.values_mut() {
                u.genome.generation = u32::MAX;
            }
        }
        let serial = w.next_serial();
        w.step();
        assert_eq!(w.next_serial(), serial);
        assert!(w.ecology.as_ref().unwrap().births.is_empty());
        assert!(
            w.ecology
                .as_ref()
                .unwrap()
                .bodies
                .values()
                .all(|b| b.account.transferred_q == 0)
        );
    }
}

#[test]
fn candidate_birth_rollback_checkpoint_replay_and_ancestry_after_death_are_real() {
    let mut original = ArenaRuntime::new([105; 16], ready_parent_world());
    let cp = original.checkpoint().unwrap();
    assert!(
        original
            .transact_at(original.tick + 2, |_| Err::<(), _>(
                RynvekErrorCode::SimulationInvariantViolated
            ))
            .is_err()
    );
    assert_eq!(
        original.checkpoint().unwrap().bytes,
        cp.bytes,
        "候选Tick已出生也必须连同序号/脑/能量/谱系整体撤销"
    );
    let mut replay = ArenaRuntime::restore([105; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    for _ in 0..8 {
        original.step().unwrap();
        replay.step().unwrap();
        assert_eq!(original.state_root(), replay.state_root());
    }
    let mut tampered = original.world().clone();
    let root = tampered.canonical_state().entity_root;
    tampered.ecology.as_mut().unwrap().births.clear();
    assert_ne!(
        tampered.canonical_state().entity_root,
        root,
        "私有谱系事实必须进入规范根"
    );
    assert!(
        tampered.encode_snapshot([105; 16], 1, [0; 32]).is_err(),
        "不能保留亲本转出却丢弃出生收款谱系"
    );
    let eco = original.ecology.as_ref().unwrap();
    assert!(!eco.births.is_empty());
    let (&child, birth) = eco.births.iter().next().unwrap();
    let parent = birth.parent;
    let saved = birth.clone();
    original
        .transact_at(original.tick, |w| {
            w.units.get_mut(&parent).unwrap().defense.hp_milli = 0;
            Ok(())
        })
        .unwrap();
    original.step().unwrap();
    assert!(!original.units.contains_key(&parent));
    assert_eq!(original.ecology.as_ref().unwrap().births[&child], saved);
    assert!(
        original.ecology.as_ref().unwrap().remains[&parent]
            .account
            .is_conserved()
    );
    let cp = original.checkpoint().unwrap();
    let restored = ArenaRuntime::restore([105; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    assert_eq!(restored.ecology, original.ecology);
    assert_eq!(restored.state_root(), original.state_root());
}
