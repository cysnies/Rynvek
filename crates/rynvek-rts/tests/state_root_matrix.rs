// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规范化状态根单字段扰动全覆盖测试矩阵（审计报告 §3.4）。
//!
//! 对 `ArenaWorld` 的 14 类历史哈希盲区逐一构造最小单字段扰动，验证：
//! 1. 对应的分层子根（`neural_root` / `entity_root` / `economy_root` / `evolution_root` /
//!    `visibility_root` / `scheduled_event_root`）必然发生变化；
//! 2. 顶层 `world.state_hash()` 必然发生变化，杜绝静默状态分叉（Silent Desync）。

use rynvek_core::Q8_8;
use rynvek_neural::PopulationBrainSlot;
use rynvek_rts::brain::UnitBrain;
use rynvek_rts::evolution::FitnessRecord;
use rynvek_rts::fog::BuildingGhost;
use rynvek_rts::genome::Genome;
use rynvek_rts::ids::{BuildingKind, EntitySerial, FactionId, FieldId, OrderKind, TechId, UnitKind};
use rynvek_rts::math::Vec2;
use rynvek_rts::modulators::Modulators;
use rynvek_rts::resources::ResourceAmounts;
use rynvek_rts::world::{ArenaWorld, BreedJob, MatchConfig, ProductionJob};

/// 构造一个包含四种脑模型单位与多类建筑的标准对局基准世界
fn baseline_world() -> ArenaWorld {
    let mut w = ArenaWorld::new(MatchConfig::pve(42, 2));
    // 补充包含 CTRNN（Carapace / Weaver）与 Izhikevich（Leviathan）的单位
    for (idx, kind) in [UnitKind::Carapace, UnitKind::Weaver, UnitKind::Leviathan]
        .into_iter()
        .enumerate()
    {
        let id = EntitySerial(100 + idx as u32);
        let spec = rynvek_rts::units::unit_spec(kind);
        let genome = Genome::seed(kind);
        let brain = UnitBrain::from_genome(&genome).expect("种子基因组可表达");
        let pos = Vec2::from_int(40 + idx as i32 * 2, 40);
        w.units.insert(
            id,
            rynvek_rts::world::Unit {
                id,
                faction: FactionId(0),
                kind,
                genome,
                brain,
                position: pos,
                velocity: Vec2::ZERO,
                defense: rynvek_rts::combat::Defense {
                    hp_milli: spec.hp_milli,
                    shield_milli: spec.shield_milli,
                    armor: spec.armor,
                },
                max_hp_milli: spec.hp_milli,
                max_shield_milli: spec.shield_milli,
                weapon_cooldown: 0,
                order: rynvek_rts::world::Order {
                    kind: OrderKind::HoldPosition,
                    target: pos,
                    origin: pos,
                },
                born_tick: 0,
                fitness: FitnessRecord::default(),
                modulators: Modulators::ZERO,
                actions: [0; 6],
                harvest_remainder: 0,
            },
        );
    }
    w.set_next_serial(200);
    w
}

#[test]
fn mutation_01_lif_membrane_potential_changes_neural_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    // 找到 Striker（LIF 脑）并扰动第 0 个隐藏神经元膜电位 +1
    let striker = mutated
        .units
        .values_mut()
        .find(|u| matches!(u.brain.0, PopulationBrainSlot::Lif(_)))
        .expect("存在 LIF 单位");
    if let PopulationBrainSlot::Lif(b) = &mut striker.brain.0 {
        b.membrane_v_q8[0] = Q8_8(b.membrane_v_q8[0].0.wrapping_add(1));
    }
    assert_ne!(
        mutated.canonical_state().neural_root,
        base.canonical_state().neural_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_02_lif_plasticity_delta_w_changes_neural_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let striker = mutated
        .units
        .values_mut()
        .find(|u| matches!(u.brain.0, PopulationBrainSlot::Lif(_)))
        .expect("存在 LIF 单位");
    if let PopulationBrainSlot::Lif(b) = &mut striker.brain.0 {
        b.weights_delta_q8[0] = b.weights_delta_q8[0].wrapping_add(1);
    }
    assert_ne!(
        mutated.canonical_state().neural_root,
        base.canonical_state().neural_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_03_ctrnn_continuous_state_changes_neural_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let carapace = mutated
        .units
        .values_mut()
        .find(|u| matches!(u.brain.0, PopulationBrainSlot::Ctrnn(_)))
        .expect("存在 CTRNN 单位");
    if let PopulationBrainSlot::Ctrnn(b) = &mut carapace.brain.0 {
        b.y_q8[0] = b.y_q8[0].wrapping_add(1);
    }
    assert_ne!(
        mutated.canonical_state().neural_root,
        base.canonical_state().neural_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_04_izhikevich_recovery_u_changes_neural_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let leviathan = mutated
        .units
        .values_mut()
        .find(|u| matches!(u.brain.0, PopulationBrainSlot::Izhikevich(_)))
        .expect("存在 Izhikevich 单位");
    if let PopulationBrainSlot::Izhikevich(b) = &mut leviathan.brain.0 {
        b.u_q8[0] = b.u_q8[0].wrapping_add(1);
    }
    assert_ne!(
        mutated.canonical_state().neural_root,
        base.canonical_state().neural_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_05_unit_order_target_changes_entity_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let u = mutated.units.values_mut().next().expect("存在单位");
    u.order.target.x.0 += 1;
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_06_unit_fitness_damage_changes_entity_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let u = mutated.units.values_mut().next().expect("存在单位");
    u.fitness.damage_dealt_milli += 100;
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_07_unit_harvest_remainder_changes_entity_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let u = mutated.units.values_mut().next().expect("存在单位");
    u.harvest_remainder += 1;
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_08_building_production_queue_remaining_ticks_changes_entity_and_state_root() {
    let mut base = baseline_world();
    let b_id = *base.buildings.keys().next().expect("存在建筑");
    base.buildings.get_mut(&b_id).unwrap().queue.push_back(ProductionJob {
        unit: UnitKind::Forager,
        genome: Genome::seed(UnitKind::Forager),
        remaining_ticks: 45,
        cost: ResourceAmounts::whole(40, 0, 20),
    });
    let mut mutated = base.clone();
    mutated.buildings.get_mut(&b_id).unwrap().queue[0].remaining_ticks -= 1;
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_09_building_research_progress_changes_entity_and_state_root() {
    let mut base = baseline_world();
    let b_id = *base.buildings.keys().next().expect("存在建筑");
    base.buildings.get_mut(&b_id).unwrap().research = Some((TechId::ChemotaxisAmp, 120));
    let mut mutated = base.clone();
    mutated.buildings.get_mut(&b_id).unwrap().research = Some((TechId::ChemotaxisAmp, 119));
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_10_building_breeding_progress_changes_entity_and_state_root() {
    let mut base = baseline_world();
    let b_id = *base.buildings.keys().next().expect("存在建筑");
    base.buildings.get_mut(&b_id).unwrap().breeding = Some(BreedJob {
        unit: UnitKind::Striker,
        child: Genome::seed(UnitKind::Striker),
        remaining_ticks: 300,
    });
    let mut mutated = base.clone();
    mutated
        .buildings
        .get_mut(&b_id)
        .unwrap()
        .breeding
        .as_mut()
        .unwrap()
        .remaining_ticks -= 1;
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_11_building_beacon_strength_changes_entity_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    let b = mutated.buildings.values_mut().next().expect("存在建筑");
    b.beacon.channel = FieldId::SignalB;
    b.beacon.strength_q8 += 256;
    assert_ne!(
        mutated.canonical_state().entity_root,
        base.canonical_state().entity_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}

#[test]
fn mutation_12_faction_fog_and_ghosts_change_visibility_and_state_root() {
    let base = baseline_world();
    let mut mutated_fog = base.clone();
    mutated_fog.factions[0].fog.set_explored(120, 120, true);
    assert_ne!(
        mutated_fog.canonical_state().visibility_root,
        base.canonical_state().visibility_root
    );
    assert_ne!(mutated_fog.state_hash(), base.state_hash());

    let mut mutated_ghost = base.clone();
    mutated_ghost.factions[0].ghosts.insert(
        EntitySerial(777),
        BuildingGhost {
            entity_id: EntitySerial(777),
            faction: FactionId(1),
            kind: BuildingKind::SpawningVat,
            origin: (180, 180),
            last_seen_tick: 12,
        },
    );
    assert_ne!(
        mutated_ghost.canonical_state().visibility_root,
        base.canonical_state().visibility_root
    );
    assert_ne!(mutated_ghost.state_hash(), base.state_hash());
}

#[test]
fn mutation_13_pve_director_wave_and_records_change_evolution_and_state_root() {
    let base = baseline_world();
    let mut mutated_wave = base.clone();
    mutated_wave.pve.as_mut().expect("PvE 模式").wave += 1;
    assert_ne!(
        mutated_wave.canonical_state().evolution_root,
        base.canonical_state().evolution_root
    );
    assert_ne!(mutated_wave.state_hash(), base.state_hash());

    let mut mutated_rec = base.clone();
    mutated_rec.pve_records.push((
        301,
        Genome::seed(UnitKind::Striker),
        FitnessRecord {
            damage_dealt_milli: 5_000,
            survival_ticks: 90,
        },
    ));
    assert_ne!(
        mutated_rec.canonical_state().evolution_root,
        base.canonical_state().evolution_root
    );
    assert_ne!(mutated_rec.state_hash(), base.state_hash());
}

#[test]
fn mutation_14_sudden_death_until_changes_scheduled_event_and_state_root() {
    let base = baseline_world();
    let mut mutated = base.clone();
    mutated.sudden_death_until = Some(39_600);
    assert_ne!(
        mutated.canonical_state().scheduled_event_root,
        base.canonical_state().scheduled_event_root
    );
    assert_ne!(mutated.state_hash(), base.state_hash());
}
