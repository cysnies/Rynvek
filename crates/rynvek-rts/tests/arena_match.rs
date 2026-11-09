// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `ArenaWorld` 端到端断言：Starter Kit、经济速率、建筑落点错误码、生产 / 人口、科技、繁育、
//! L3 信标调质、战斗、迷雾幽灵、胜负裁决、PvE 虫潮与整局确定性。

use rynvek_core::Q32_32;
use rynvek_rts::commands::{CommandAck, CrucibleAction, RtsCommand, RtsError, UnitSelector};
use rynvek_rts::evolution::FitnessRecord;
use rynvek_rts::ids::{BuildingKind, EntitySerial, FactionId, FieldId, OrderKind, TechId, UnitKind};
use rynvek_rts::math::Vec2;
use rynvek_rts::modulators::Modulators;
use rynvek_rts::pve::wave_budget;
use rynvek_rts::resources::ResourceAmounts;
use rynvek_rts::units::unit_spec;
use rynvek_rts::world::{ArenaWorld, MatchConfig, MatchOutcome, RtsEvent, VictoryReason};

const P0: FactionId = FactionId(0);
const P1: FactionId = FactionId(1);

fn rich(w: &mut ArenaWorld, f: FactionId) {
    let fac = &mut w.factions[f.0 as usize];
    fac.ledger.capacity = ResourceAmounts::whole(100_000, 100_000, 100_000);
    fac.ledger.balance = ResourceAmounts::whole(50_000, 50_000, 50_000);
}

/// 在主脑附近找一个合法落点（按 (y, x) 升序）
fn find_spot(w: &ArenaWorld, f: FactionId, kind: BuildingKind) -> (u32, u32) {
    let nexus = &w.buildings[&w.factions[f.0 as usize].nexus.unwrap()];
    let (cx, cy) = nexus.center_tile();
    for r in 3..20i64 {
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx as i64 + dx, cy as i64 + dy);
                if x < 0 || y < 0 {
                    continue;
                }
                if w.validate_placement(f, kind, (x as u32, y as u32)).is_ok() {
                    return (x as u32, y as u32);
                }
            }
        }
    }
    panic!("找不到 {kind} 落点");
}

fn place(w: &mut ArenaWorld, f: FactionId, kind: BuildingKind) -> EntitySerial {
    let origin = find_spot(w, f, kind);
    match w.apply_command(f, &RtsCommand::PlaceBuilding { kind, origin }).unwrap() {
        CommandAck::BuildingPlaced { id } => id,
        other => panic!("{other:?}"),
    }
}

fn finish_construction(w: &mut ArenaWorld, id: EntitySerial) {
    while !w.buildings[&id].complete {
        w.step();
    }
}

#[test]
fn starter_kit_matches_design_bible() {
    let w = ArenaWorld::new(MatchConfig::duel(42));
    let anchors = w.map.spawn_anchors();
    for (i, f) in w.factions.iter().enumerate() {
        assert_eq!(f.ledger.balance, ResourceAmounts::new(500_000, 300_000, 100_000));
        let nexus = &w.buildings[&f.nexus.unwrap()];
        assert_eq!(nexus.kind, BuildingKind::NexusCore);
        assert!(nexus.complete);
        assert_eq!(nexus.center(), anchors[i], "主脑中心即出生锚点");
        let units: Vec<_> = w.units.values().filter(|u| u.faction == f.id).collect();
        assert_eq!(units.iter().filter(|u| u.kind == UnitKind::Forager).count(), 3);
        assert_eq!(units.iter().filter(|u| u.kind == UnitKind::Striker).count(), 1);
        for u in units {
            let d = u.position.distance_sq(anchors[i]).sqrt_deterministic();
            let expect = if u.kind == UnitKind::Forager { 3_500 } else { 4_500 };
            assert!(
                (d.0 - Q32_32::from_milli(expect).0).abs() < 1 << 12,
                "{:?} 距离",
                u.kind
            );
        }
    }
    // 双方开局资产中心对称
    let p0: Vec<_> = w.units.values().filter(|u| u.faction == P0).collect();
    let p1: Vec<_> = w.units.values().filter(|u| u.faction == P1).collect();
    for (a, b) in p0.iter().zip(&p1) {
        let m = w.map.mirror_point(a.position);
        assert!((m.x.0 - b.position.x.0).abs() < 1 << 12 && (m.y.0 - b.position.y.0).abs() < 1 << 12);
    }
    assert_eq!(w.supply(P0), (3 + 2, 0));
}

#[test]
fn nexus_and_reactor_rates_are_exact() {
    let mut w = ArenaWorld::new(MatchConfig::duel(42));
    w.run(300);
    assert_eq!(w.factions[0].ledger.balance.flux, 600_000, "主脑 +30 Flux/s × 10 s");
    let reactor = place(&mut w, P0, BuildingKind::FluxReactor);
    assert_eq!(w.factions[0].ledger.balance.biomass, 300_000);
    assert_eq!(w.factions[0].ledger.balance.neurogel, 80_000);
    finish_construction(&mut w, reactor);
    assert_eq!(w.factions[0].ledger.capacity.flux, 1_500_000, "反应堆 +500 电容");
    // 对齐到秒边界后量一整秒
    while w.tick % 30 != 0 {
        w.step();
    }
    let before = w.factions[0].ledger.balance;
    w.run(30);
    let after = w.factions[0].ledger.balance;
    let bonus = if w.buildings[&reactor].on_fissure {
        90_000
    } else {
        60_000
    };
    assert_eq!(after.flux - before.flux, 30_000 + bonus);
    assert_eq!(after.biomass - before.biomass, -1_500);
}

#[test]
fn placement_errors_use_spec_codes() {
    let mut w = ArenaWorld::new(MatchConfig::duel(7));
    let code = |r: Result<CommandAck, RtsError>| r.unwrap_err().code().code();
    let nexus_origin = w.buildings[&w.factions[0].nexus.unwrap()].origin;
    let cmd = |kind, origin| RtsCommand::PlaceBuilding { kind, origin };
    assert_eq!(
        code(w.apply_command(P0, &cmd(BuildingKind::PulseTurret, (255, 255)))),
        0x8006,
        "越界"
    );
    assert_eq!(
        code(w.apply_command(P0, &cmd(BuildingKind::PulseTurret, nexus_origin))),
        0x8006,
        "重叠"
    );
    assert_eq!(
        code(w.apply_command(P0, &cmd(BuildingKind::PulseTurret, (128, 128)))),
        0x8006,
        "不可见"
    );
    assert_eq!(
        code(w.apply_command(P0, &cmd(BuildingKind::NexusCore, (40, 40)))),
        0x8001,
        "主脑不可建造"
    );
    w.factions[0].ledger.balance = ResourceAmounts::ZERO;
    let spot = find_spot(&w, P0, BuildingKind::PulseTurret);
    assert_eq!(
        code(w.apply_command(P0, &cmd(BuildingKind::PulseTurret, spot))),
        0x8005,
        "资源不足"
    );
    assert_eq!(w.factions[0].ledger.balance, ResourceAmounts::ZERO, "失败不扣费");
    // 对方实体
    let enemy_nexus = w.factions[1].nexus.unwrap();
    assert_eq!(
        code(w.apply_command(P0, &RtsCommand::Demolish { building: enemy_nexus })),
        0x8001
    );
    assert_eq!(
        code(w.apply_command(
            P0,
            &RtsCommand::Demolish {
                building: EntitySerial(9999)
            }
        )),
        0x2002
    );
}

#[test]
fn production_respects_supply_queue_and_hatch_time() {
    let mut w = ArenaWorld::new(MatchConfig::duel(3));
    rich(&mut w, P0);
    let vat = place(&mut w, P0, BuildingKind::SpawningVat);
    let q = |count| RtsCommand::QueueUnit {
        vat,
        unit: UnitKind::Striker,
        genome_slug: "seed.striker.v1".into(),
        count,
    };
    assert_eq!(w.apply_command(P0, &q(1)).unwrap_err().code().code(), 0x8001, "未完工");
    finish_construction(&mut w, vat);
    assert_eq!(w.supply(P0), (5, 24));
    assert_eq!(
        w.apply_command(P0, &q(5)).unwrap(),
        CommandAck::Queued { vat, count: 5 }
    );
    assert_eq!(
        w.apply_command(P0, &q(1)).unwrap_err().code().code(),
        0x8004,
        "队列上限 5"
    );
    assert_eq!(w.supply(P0), (15, 24));
    let locked = RtsCommand::QueueUnit {
        vat,
        unit: UnitKind::Carapace,
        genome_slug: "seed.carapace.v1".into(),
        count: 1,
    };
    w.apply_command(P0, &RtsCommand::CancelQueue { vat }).unwrap();
    assert_eq!(
        w.apply_command(P0, &locked).unwrap_err(),
        RtsError::TechLocked {
            unit: UnitKind::Carapace
        }
    );
    w.drain_events();
    let hatch = unit_spec(UnitKind::Striker).hatch_ticks as u64;
    w.run(hatch);
    let spawned = w
        .drain_events()
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                RtsEvent::UnitSpawned {
                    kind: UnitKind::Striker,
                    ..
                }
            )
        })
        .count();
    assert_eq!(spawned, 1, "Striker 孵化 120 Ticks");
    assert_eq!(
        w.units
            .values()
            .filter(|u| u.faction == P0 && u.kind == UnitKind::Striker)
            .count(),
        2
    );
}

#[test]
fn tech_tree_prerequisites_and_effects() {
    let mut w = ArenaWorld::new(MatchConfig::duel(11));
    rich(&mut w, P0);
    let refinery = place(&mut w, P0, BuildingKind::SynapseRefinery);
    finish_construction(&mut w, refinery);
    let research = |tech| RtsCommand::ResearchTech { site: refinery, tech };
    assert_eq!(
        w.apply_command(P0, &research(TechId::ChemotaxisAmp)).unwrap_err(),
        RtsError::PrerequisitesUnmet {
            tech: TechId::ChemotaxisAmp
        }
    );
    assert_eq!(
        w.apply_command(P0, &research(TechId::MulticellularChitin))
            .unwrap_err()
            .code()
            .code(),
        0x2002,
        "T2 只能在坩埚研发"
    );
    let vat = place(&mut w, P0, BuildingKind::SpawningVat);
    finish_construction(&mut w, vat);
    let before = w.factions[0].ledger.balance;
    w.apply_command(P0, &research(TechId::ChemotaxisAmp)).unwrap();
    assert_eq!(
        before.minus(w.factions[0].ledger.balance),
        ResourceAmounts::whole(150, 100, 30)
    );
    assert!(matches!(
        w.apply_command(P0, &research(TechId::FluxCapacitance)),
        Err(RtsError::SiteBusy { .. })
    ));
    w.run(450);
    assert!(w.factions[0].techs.contains(TechId::ChemotaxisAmp));
    assert_eq!(w.factions[0].modifiers().forager_harvest_milli_per_s, 5_200);
    assert_eq!(
        w.apply_command(P0, &research(TechId::ChemotaxisAmp))
            .unwrap_err()
            .code()
            .code(),
        0x8001
    );
    // 坩埚：T2 几丁质 → 护甲 +2、解锁 Carapace
    let crucible = place(&mut w, P0, BuildingKind::MutationCrucible);
    finish_construction(&mut w, crucible);
    let armor_before = w.units.values().find(|u| u.faction == P0).unwrap().defense.armor;
    w.apply_command(
        P0,
        &RtsCommand::Crucible {
            crucible,
            action: CrucibleAction::ResearchTech(TechId::MulticellularChitin),
        },
    )
    .unwrap();
    w.run(750);
    assert!(w.factions[0].techs.unit_unlocked(UnitKind::Carapace));
    assert_eq!(
        w.units.values().find(|u| u.faction == P0).unwrap().defense.armor,
        armor_before + 2
    );
}

#[test]
fn crucible_breeds_elite_genome_usable_by_vats() {
    let mut w = ArenaWorld::new(MatchConfig::duel(5));
    rich(&mut w, P0);
    let crucible = place(&mut w, P0, BuildingKind::MutationCrucible);
    finish_construction(&mut w, crucible);
    let breed = |rate| RtsCommand::Crucible {
        crucible,
        action: CrucibleAction::BreedEliteGenome {
            unit: UnitKind::Forager,
            mutation_rate_permille: rate,
        },
    };
    assert_eq!(
        w.apply_command(P0, &breed(400)).unwrap_err().code().code(),
        0x5001,
        "突变率越界"
    );
    // 人为抬高一个 Forager 的适应度，使其成为精英
    let elite = w
        .units
        .values()
        .find(|u| u.faction == P0 && u.kind == UnitKind::Forager)
        .unwrap()
        .id;
    w.units.get_mut(&elite).unwrap().fitness = FitnessRecord {
        damage_dealt_milli: 1_000_000,
        survival_ticks: 0,
    };
    let before = w.factions[0].ledger.balance;
    w.apply_command(P0, &breed(80)).unwrap();
    assert_eq!(
        before.minus(w.factions[0].ledger.balance),
        ResourceAmounts::whole(0, 50, 50)
    );
    w.run(600);
    let bred = w.factions[0].genomes.get("bred.f0.1").expect("繁育产物入库");
    assert_eq!(bred.unit, UnitKind::Forager);
    assert_eq!(bred.generation, 1);
    assert!(
        w.drain_events()
            .iter()
            .any(|e| matches!(e, RtsEvent::GenomeBred { .. }))
    );
    let vat = place(&mut w, P0, BuildingKind::SpawningVat);
    finish_construction(&mut w, vat);
    w.apply_command(
        P0,
        &RtsCommand::QueueUnit {
            vat,
            unit: UnitKind::Forager,
            genome_slug: "bred.f0.1".into(),
            count: 1,
        },
    )
    .unwrap();
    w.run(90);
    assert!(w.units.values().any(|u| u.genome.slug == "bred.f0.1"));
}

#[test]
fn beacon_injects_field_and_broadcasts_modulators() {
    let mut w = ArenaWorld::new(MatchConfig::duel(9));
    let beacon = place(&mut w, P0, BuildingKind::SignalBeacon);
    finish_construction(&mut w, beacon);
    w.apply_command(
        P0,
        &RtsCommand::ConfigureBeacon {
            beacon,
            channel: FieldId::SignalB,
            strength_milli: 40_000,
            modulators_milli: [500, -250, 1000, 0],
        },
    )
    .unwrap();
    let bad = RtsCommand::ConfigureBeacon {
        beacon,
        channel: FieldId::Hazard,
        strength_milli: 0,
        modulators_milli: [0; 4],
    };
    assert!(w.apply_command(P0, &bad).is_err());
    w.run(12);
    let c = w.buildings[&beacon].center_tile();
    assert!(w.fields.get(FieldId::SignalB, c.0 as i64, c.1 as i64) > 0);
    let center = w.buildings[&beacon].center();
    assert_eq!(w.modulators_at(P0, center), Modulators([128, -64, 256, 0]));
    assert_eq!(w.modulators_at(P1, center), Modulators::ZERO, "调质只广播给本阵营");
    // 信标半径内的己方单位在下一 Epoch 收到调质
    let near = w
        .units
        .values()
        .filter(|u| u.faction == P0 && u.position.distance_sq(center) <= Q32_32::from_int(256))
        .count();
    assert!(near > 0);
    assert!(
        w.units
            .values()
            .filter(|u| u.faction == P0 && u.position.distance_sq(center) <= Q32_32::from_int(256))
            .all(|u| u.modulators.0[2] == 256)
    );
}

#[test]
fn surrender_and_elimination_end_the_match() {
    let mut w = ArenaWorld::new(MatchConfig::duel(1));
    w.apply_command(P1, &RtsCommand::Surrender).unwrap();
    w.step();
    assert!(matches!(
        w.outcome,
        Some(MatchOutcome::Victory {
            winner: P0,
            reason: VictoryReason::Surrender,
            ..
        })
    ));
    assert_eq!(
        w.apply_command(P0, &RtsCommand::Surrender).unwrap_err().code().code(),
        0x8001
    );

    let mut w = ArenaWorld::new(MatchConfig::duel(1));
    let nexus = w.factions[0].nexus.unwrap();
    w.buildings.get_mut(&nexus).unwrap().defense.hp_milli = 0;
    w.step();
    assert!(matches!(
        w.outcome,
        Some(MatchOutcome::Victory {
            winner: P1,
            reason: VictoryReason::Elimination,
            ..
        })
    ));
}

#[test]
fn timeout_tie_breaker_triggers_sudden_death_on_symmetric_match() {
    let mut w = ArenaWorld::new(MatchConfig::duel(2));
    // 快进到时限前（对称局面下双方积分相等 → 突然死亡）
    w.tick = 35_990;
    w.run(20);
    assert!(w.sudden_death_until.is_some());
    assert!(w.outcome.is_none());
    let hp0 = w.buildings[&w.factions[0].nexus.unwrap()].defense.hp_milli;
    w.run(30);
    let hp1 = w.buildings[&w.factions[0].nexus.unwrap()].defense.hp_milli;
    assert_eq!(hp0 - hp1, 20_000, "突然死亡主脑每秒流失 20 HP");
    w.run(3_600);
    assert!(matches!(
        w.outcome,
        Some(MatchOutcome::Draw { .. })
            | Some(MatchOutcome::Victory {
                reason: VictoryReason::SuddenDeath,
                ..
            })
    ));

    // 不对称局面：P0 多一座完工建筑 → 直接按积分获胜
    let mut w = ArenaWorld::new(MatchConfig::duel(2));
    rich(&mut w, P0);
    let t = place(&mut w, P0, BuildingKind::PulseTurret);
    finish_construction(&mut w, t);
    assert!(w.score_milli(P0) > w.score_milli(P1));
    w.tick = 35_999;
    w.step();
    assert!(matches!(
        w.outcome,
        Some(MatchOutcome::Victory {
            winner: P0,
            reason: VictoryReason::TieBreakerScore,
            ..
        })
    ));
}

#[test]
fn striker_assault_paths_across_map_and_engages_enemy_base() {
    let mut w = ArenaWorld::new(MatchConfig::duel(42));
    let striker = w
        .units
        .values()
        .find(|u| u.faction == P0 && u.kind == UnitKind::Striker)
        .unwrap()
        .id;
    let target = w.buildings[&w.factions[1].nexus.unwrap()].center();
    w.apply_command(
        P0,
        &RtsCommand::IssueOrder {
            units: UnitSelector::Ids(vec![striker]),
            order: OrderKind::AttackMove,
            target,
        },
    )
    .unwrap();
    let start = w.units[&striker].position;
    let mut engaged = false;
    for _ in 0..4_000 {
        w.step();
        let enemy_hurt = w
            .units
            .values()
            .any(|u| u.faction == P1 && u.defense.hp_milli < u.max_hp_milli)
            || w.buildings
                .values()
                .any(|b| b.faction == P1 && b.defense.hp_milli < b.max_hp_milli);
        if enemy_hurt || !w.units.contains_key(&striker) {
            engaged = true;
            break;
        }
    }
    assert!(engaged, "Striker 应在 4000 Ticks 内穿越地图接敌");
    if let Some(u) = w.units.get(&striker) {
        assert!(u.position.distance_sq(start) > Q32_32::from_int(100 * 100));
    }
    // 接敌后敌方阵营在迷雾中看到了我方单位或我方记录了敌方建筑幽灵
    w.update_fog();
    assert!(!w.factions[0].ghosts.is_empty() || !w.units.contains_key(&striker));
}

#[test]
fn fog_hides_enemy_base_at_start() {
    let w = ArenaWorld::new(MatchConfig::duel(42));
    let enemy = &w.buildings[&w.factions[1].nexus.unwrap()];
    let r = enemy.rect();
    assert!(!w.factions[0].fog.any_visible(r.x0, r.y0, 4, 4));
    assert!(w.factions[0].ghosts.is_empty());
    let own = &w.buildings[&w.factions[0].nexus.unwrap()];
    assert!(w.factions[0].fog.is_visible(own.origin.0, own.origin.1));
}

#[test]
fn pve_waves_spawn_on_budget_and_evolve() {
    let mut w = ArenaWorld::new(MatchConfig::pve(13, 1));
    assert_eq!(w.factions.len(), 1);
    w.run(1_800);
    let wild: Vec<_> = w.units.values().filter(|u| u.faction.is_wild()).collect();
    assert!(!wild.is_empty());
    let spent: i64 = wild.iter().map(|u| unit_spec(u.kind).cost.biomass).sum();
    assert!(spent <= wave_budget(1) as i64 * 1000);
    assert!(wild.iter().all(|u| u.order.kind == OrderKind::AttackMove));
    let rifts = w.map.edge_rift_anchors();
    for u in &wild {
        let t = u.tile();
        assert!(
            rifts.iter().any(|r| r.0.abs_diff(t.0) <= 2 && r.1.abs_diff(t.1) <= 2),
            "{t:?} 不在裂隙附近"
        );
    }
    w.run(1_800);
    let d = w.pve.as_ref().unwrap();
    assert_eq!(d.wave, 2);
    assert!(d.pool.iter().all(|g| g.generation >= 1), "第 2 波基因池来自繁育");
    assert!(
        w.drain_events()
            .iter()
            .filter(|e| matches!(e, RtsEvent::WaveSpawned { .. }))
            .count()
            == 2
    );
}

#[test]
fn whole_match_is_bitwise_deterministic() {
    let script = |w: &mut ArenaWorld| {
        let mut hashes = Vec::new();
        for step in 0..1_200u64 {
            if step == 10 {
                for f in [P0, P1] {
                    let target = Vec2::from_int(128, 128);
                    w.apply_command(
                        f,
                        &RtsCommand::IssueOrder {
                            units: UnitSelector::Archetype(None),
                            order: OrderKind::AttackMove,
                            target,
                        },
                    )
                    .unwrap();
                }
            }
            if step == 20 {
                let _ = place(w, P0, BuildingKind::BiomassHarvester);
                let _ = place(w, P1, BuildingKind::FluxReactor);
            }
            w.step();
            if step % 200 == 0 {
                hashes.push(w.state_hash());
            }
        }
        hashes
    };
    let mut a = ArenaWorld::new(MatchConfig::duel(77));
    let mut b = ArenaWorld::new(MatchConfig::duel(77));
    assert_eq!(script(&mut a), script(&mut b));
    let mut c = ArenaWorld::new(MatchConfig::duel(78));
    assert_ne!(a.state_hash(), {
        script(&mut c);
        c.state_hash()
    });
}
