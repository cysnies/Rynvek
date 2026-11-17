// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::genome::action;
use rynvek_rts::world::ecology::ForagerEcologyContract;
use rynvek_rts::{ArenaMap, ArenaRuntime, ArenaWorld, MatchConfig, TerrainTileKind, UnitKind};

fn budget_world() -> (ArenaWorld, rynvek_rts::EntitySerial) {
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
    let id = *w.units.iter().find(|(_, u)| u.kind == UnitKind::Forager).unwrap().0;
    // 合法的可配置LIF基因，真实Arena脑内核；不向个体池注资。
    let u = w.units.get_mut(&id).unwrap();
    let mut g = u.genome.clone();
    g.model = rynvek_neural::NeuronModelKind::LIF;
    g.w_in.fill(256);
    let n = g.n_hid as usize;
    g.w_out[action::HARVEST * n..(action::HARVEST + 1) * n].fill(512);
    u.brain = rynvek_rts::brain::UnitBrain::from_genome(&g).unwrap();
    u.genome = g;
    (w, id)
}

#[test]
fn unfunded_brain_cannot_commit_then_real_uptake_funds_resumption() {
    let (mut w, id) = budget_world();
    for y in 0..96 {
        for x in 0..96 {
            w.fields.set(rynvek_rts::FieldId::Nutrient, x, y, 0);
        }
    }
    let before = w.units[&id].brain.state_root();
    w.units.get_mut(&id).unwrap().actions = [200; 6];
    assert_eq!(w.ecology.as_ref().unwrap().bodies[&id].account.received_q, 0);
    w.step();
    assert_eq!(
        w.units[&id].brain.state_root(),
        before,
        "预算不足必须撤销整个候选脑Epoch，不能免费提交神经状态"
    );
    assert_eq!(w.units[&id].actions, [0; 6], "不能沿用未付费或旧动作读出");
    let a = &w.ecology.as_ref().unwrap().bodies[&id].account;
    assert_eq!(a.dissipated_q, 0);
    assert_eq!(a.unfunded_q, 0);
    assert_eq!(a.received_q, 0, "无局部营养不能凭空供能");
    assert_eq!(a.stored_q, 0);
    let (x, y) = w.units[&id].tile();
    w.fields.set(rynvek_rts::FieldId::Nutrient, x, y, 5120);
    // 当前规则先积累亲本/子代出生预算，不能沿用旧20Tick即可转出全部盈余的假设。
    w.run(40);
    assert_ne!(w.units[&id].brain.state_root(), before);
    let a = &w.ecology.as_ref().unwrap().bodies[&id].account;
    assert!(a.dissipated_q > 0);
    assert_eq!(a.unfunded_q, 0);
    assert!(a.is_conserved());
    assert!(a.exported_q > 0, "预算不是吞掉原有RTS资源转移");
}

#[test]
fn budgeted_energy_recovers_and_replays_with_private_roots() {
    let (mut w, id) = budget_world();
    w.run(9);
    let mut a = ArenaRuntime::new([73; 16], w);
    let cp = a.checkpoint().unwrap();
    let mut b = ArenaRuntime::restore([73; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    for _ in 0..21 {
        a.step().unwrap();
        b.step().unwrap();
        assert_eq!(a.state_root(), b.state_root());
    }
    assert_eq!(a.ecology, b.ecology);
    assert_eq!(a.ecology.as_ref().unwrap().bodies[&id].account.unfunded_q, 0);
    assert!(
        a.ecology
            .as_ref()
            .unwrap()
            .bodies
            .values()
            .all(|b| b.account.is_conserved())
    );
}

#[test]
fn roles_without_an_individual_feed_contract_are_not_starved_or_repriced() {
    let (mut budget, _) = budget_world();
    let mut exchange = budget.clone();
    exchange.config.ecology = None;
    exchange.ecology = None;
    let striker = *budget
        .units
        .iter()
        .find(|(_, u)| u.kind == UnitKind::Striker)
        .unwrap()
        .0;
    budget.step();
    exchange.step();
    assert_eq!(budget.units[&striker].brain, exchange.units[&striker].brain);
    assert!(budget.units.values().all(|u| u.defense.hp_milli > 0));
}

#[test]
fn exact_epoch_price_is_atomic_including_plasticity_and_both_tech_costs() {
    let (w, id) = budget_world();
    let brain = w.units[&id].brain.clone();
    let mut expected = brain.clone();
    let (actions, spikes) = expected.step_epoch_metered(&[256; 16], [0; 4], true);
    assert!(spikes > 0);
    for cost in [800, 480] {
        let price = u64::from(spikes) * u64::from(cost) * 4;
        for available in [0, price - 1, price] {
            let mut a = rynvek_neural::metabolism::EnergyAccount {
                received_q: available,
                stored_q: available,
                ..Default::default()
            };
            let old = a.clone();
            let mut actual = brain.clone();
            let out = actual.step_epoch_budgeted(&[256; 16], [0; 4], true, &mut a, cost);
            if available < price {
                assert!(out.is_none());
                assert_eq!(actual, brain);
                assert_eq!(a, old);
            } else {
                assert_eq!(out, Some((actions, spikes)));
                assert_eq!(actual, expected);
                assert_eq!(a.dissipated_q, price);
                assert_eq!(a.stored_q, 0);
                let spent = a.clone();
                let committed = actual.clone();
                let mut probe = actual.clone();
                assert!(probe.step_epoch_metered(&[256; 16], [0; 4], true).1 > 0);
                assert!(
                    actual
                        .step_epoch_budgeted(&[256; 16], [0; 4], true, &mut a, cost)
                        .is_none(),
                    "真实支付耗尽后不能继续提交下一次发放"
                );
                assert_eq!(actual, committed);
                assert_eq!(a, spent);
            }
            assert!(a.is_conserved());
            assert_eq!(a.unfunded_q, 0);
        }
    }
}

#[test]
fn budgeted_snapshots_reject_unpaid_or_double_charged_staged_spikes() {
    let (w, id) = budget_world();
    for debt in [false, true] {
        let mut bad = w.clone();
        let b = bad.ecology.as_mut().unwrap().bodies.get_mut(&id).unwrap();
        if debt {
            b.account.unfunded_q = 1;
        } else {
            b.pending_spikes = 1;
        }
        assert!(
            bad.encode_snapshot([75; 16], 1, [0; 32]).is_err(),
            "合法账户恒等式不能掩盖新规则不允许的计算欠费/重复结算"
        );
    }
}

#[test]
fn already_dead_forager_does_not_pay_for_another_candidate_epoch() {
    let (mut w, id) = budget_world();
    w.run(rynvek_core::constants::COUPLING_EPOCH_TICKS as u64);
    let before = w.ecology.as_ref().unwrap().bodies[&id].account.clone();
    assert!(before.stored_q > 0);
    w.units.get_mut(&id).unwrap().defense.hp_milli = 0;
    w.step();
    let remains = &w.ecology.as_ref().unwrap().remains[&id].account;
    assert_eq!(
        remains.dissipated_q,
        before.dissipated_q + before.stored_q - (u128::from(before.stored_q) * 75 / 100) as u64
    );
    assert_eq!(remains.unfunded_q, 0);
    assert!(remains.is_conserved());
}

#[test]
fn retired_budget_v2_snapshot_is_explicitly_unsupported() {
    let f: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/arena/forager-budget-v2.json")).unwrap();
    let bytes: Vec<u8> = f["snapshot_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    assert!(
        ArenaWorld::decode_snapshot(&bytes).is_err(),
        "退役V2数据必须明确拒绝而非隐式迁移"
    );
}
