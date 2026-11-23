// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use rynvek_rules::components::tier1::Tier1Draft;
use rynvek_simulation::{
    engine::SimulationEngine,
    life::{LifeProfile, NutrientDeposit, universe_energy_q},
    physics::{Body, ONE, PhysicsCommand as C},
    rules::GeneralRulesConfig,
};
use serde_json::{Value, json};
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap()
}
fn config(v: &Value) -> GeneralRulesConfig {
    serde_json::from_value(v["components"].clone()).unwrap()
}
fn world(v: &Value) -> SimulationEngine {
    let bodies: Vec<Body> = serde_json::from_value(v["bodies"].clone()).unwrap();
    let mut w = SimulationEngine::with_bodies([48; 16], &bodies).unwrap();
    if !v["life"].is_null() {
        let profile: LifeProfile = serde_json::from_value(v["life"]["profile"].clone()).unwrap();
        let deposits: Vec<NutrientDeposit> = serde_json::from_value(v["life"]["nutrient_deposits"].clone()).unwrap();
        w = w.with_sourced_life(profile, &deposits).unwrap();
    }
    w.with_component_rules(config(v)).unwrap()
}
fn tier1(v: &mut Value, js: &str, writes: &[&str]) {
    let draft: Tier1Draft = serde_json::from_value(json!({"package_id":"local.steer","version":"0.1.0","template_id":"model.steer","reads":["state.accel_x","param.thrust"],"writes":writes,"source_js":js,"source_ts":null})).unwrap();
    v["components"]["tier1_rules"]["steer.script"] = serde_json::to_value(draft.build().unwrap()).unwrap();
}
fn closed(w: &SimulationEngine) {
    let l = w.life().unwrap();
    assert_eq!(universe_energy_q(l, &w.field).unwrap(), l.genesis_energy_q());
}
#[test]
fn tier0_actual_components_body_motion_and_exact_sourced_charge() {
    let mut w = world(&fixture());
    let p = w.physics().body(1).unwrap().position_q32;
    w.tick().unwrap();
    let r = w.component_rules().unwrap();
    assert_eq!(r.components.entities["steer.one"].values["state.accel_x"], 256);
    assert!(r.tier0_gas > 0);
    assert_eq!(r.tier1_quota, 0);
    assert_eq!(r.paid_q, r.tier0_gas * 4);
    let a = w.life().unwrap().account(w.physics(), 1).unwrap();
    assert_eq!(a.dissipated_q, r.paid_q);
    assert_eq!(a.received_q, a.stored_q + a.dissipated_q);
    assert_eq!(w.physics().body(1).unwrap().acceleration_q32, [ONE, 0]);
    assert!(w.physics().body(1).unwrap().position_q32[0] > p[0]);
    closed(&w);
}
#[test]
fn cpu_only_rules_are_not_biological_energy_and_inert_defaults_do_not_thrust_life() {
    let mut v = fixture();
    v["life"] = Value::Null;
    v["components"]["rule_cost_micro_per_gas"] = Value::Null;
    let mut w = world(&v);
    w.tick().unwrap();
    assert!(w.life().is_none());
    assert_eq!(w.component_rules().unwrap().paid_q, 0);
    let mut v = fixture();
    v["components"]["bindings"] = json!({});
    v["components"]["templates"]["model.steer"]["components"]["state.accel_x"]["default"] = json!(256);
    let mut w = world(&v);
    w.tick().unwrap();
    assert_eq!(w.component_rules().unwrap().tier0_gas, 0);
    assert_eq!(w.physics().body(1).unwrap().acceleration_q32, [0, 0]);
    closed(&w);
}
#[test]
fn unpaid_invalid_output_and_budget_exhaustion_rollback_entire_original_tick() {
    for kind in 0..3 {
        let mut v = fixture();
        if kind == 0 {
            v["life"]["nutrient_deposits"] = json!([]);
        }
        if kind == 1 {
            v["components"]["instances"]["steer.one"]["parameters"]["param.thrust"] = json!(33 * 256);
        }
        if kind == 2 {
            v["components"]["gas_budget"] = json!(1);
        }
        let mut w = world(&v);
        let before = w.encode_snapshot().unwrap();
        assert!(w.tick().is_err(), "{kind}");
        assert_eq!(w.encode_snapshot().unwrap(), before);
    }
}
#[test]
fn tier0_tier1_share_old_snapshot_and_quota_is_priced_not_instruction_gas() {
    let mut v = fixture();
    tier1(
        &mut v,
        "function tick(i){return {writes:i.entities.map(e=>({id:e.id,values:{'state.accel_y':e.values['state.accel_x']}}))};}",
        &["state.accel_y"],
    );
    let mut w = world(&v);
    w.tick().unwrap();
    let r = w.component_rules().unwrap();
    assert_eq!(r.components.entities["steer.one"].values["state.accel_y"], 0);
    assert!(r.tier1_quota >= 10000);
    assert_eq!(r.paid_q, (r.tier0_gas + r.tier1_quota) * 4);
    closed(&w);
    w.tick().unwrap();
    assert_eq!(
        w.component_rules().unwrap().components.entities["steer.one"].values["state.accel_y"],
        256
    );
    closed(&w);
}
#[test]
fn tier1_coarse_quota_split_is_exact_over_sorted_instances() {
    let mut v = fixture();
    v["components"]["bindings"] = json!({});
    v["components"]["instances"]["steer.two"] = v["components"]["instances"]["steer.one"].clone();
    tier1(
        &mut v,
        "function tick(i){return {writes:i.entities.map(e=>({id:e.id,values:{'state.accel_x':0}}))};}",
        &["state.accel_x"],
    );
    let mut w = world(&v);
    w.tick().unwrap();
    let r = w.component_rules().unwrap();
    assert_eq!(r.paid_q, (r.tier1_quota / 2 + r.tier1_quota % 2) * 4);
    closed(&w);
}
#[test]
fn sandbox_fault_after_tier0_never_publishes_partial_state_or_ingest() {
    for js in [
        "function tick(i){while(true){}}",
        "function tick(i){return {writes:[{id:'forged',values:{'state.accel_y':0}}]};}",
    ] {
        let mut v = fixture();
        tier1(&mut v, js, &["state.accel_y"]);
        let mut w = world(&v);
        let before = w.encode_snapshot().unwrap();
        assert!(w.tick().is_err());
        assert_eq!(w.encode_snapshot().unwrap(), before);
    }
}
#[test]
fn permissions_conflicts_unknown_writes_and_competing_neural_are_rejected() {
    let mut v = fixture();
    tier1(&mut v, "function tick(i){return {};}", &["state.accel_x"]);
    assert!(config(&v).compile().is_err());
    let mut v = fixture();
    let s = v["components"]["rule_graphs"]["steer"]
        .as_str()
        .unwrap()
        .replace("state.accel_x", "energy.stored");
    v["components"]["rule_graphs"]["steer"] = json!(s);
    assert!(config(&v).compile().is_err());
    let mut v = fixture();
    v["life"]["profile"]["neural"] = json!({"hidden":1,"weights_base_q8":vec![0;rynvek_neural::LIF_INPUTS+6]});
    let profile: LifeProfile = serde_json::from_value(v["life"]["profile"].clone()).unwrap();
    let bodies: Vec<Body> = serde_json::from_value(v["bodies"].clone()).unwrap();
    let neural = SimulationEngine::with_bodies([48; 16], &bodies)
        .unwrap()
        .with_sourced_life(profile, &[])
        .unwrap();
    assert!(neural.with_component_rules(config(&v)).is_err());
    let v = fixture();
    let bodies: Vec<Body> = serde_json::from_value(v["bodies"].clone()).unwrap();
    let w = SimulationEngine::with_bodies([48; 16], &bodies).unwrap();
    // 有价生命规则不能先作为无生命CPU控制器安装，也不能用反安装顺序绕过。
    assert!(w.clone().with_component_rules(config(&v)).is_err());
    let mut cpu = config(&v);
    cpu.rule_cost_micro_per_gas = None;
    let w = w.with_component_rules(cpu).unwrap();
    let p: LifeProfile = serde_json::from_value(v["life"]["profile"].clone()).unwrap();
    assert!(w.with_sourced_life(p, &[]).is_err());
}
#[test]
fn retired_serial_never_steers_new_dense_slot_or_creates_energy() {
    let mut w = world(&fixture());
    w.tick().unwrap();
    let b = w.physics().body(1).unwrap().clone();
    w.tick_with_commands(&[
        C::Despawn { serial: 1 },
        C::Spawn {
            body: Body {
                acceleration_q32: [0, 0],
                velocity_q32: [0, 0],
                ..b
            },
        },
    ])
    .unwrap();
    assert!(w.physics().body(1).is_none());
    assert_eq!(w.physics().body(2).unwrap().acceleration_q32, [0, 0]);
    assert_eq!(w.component_rules().unwrap().paid_q, 0);
    closed(&w);
}
#[test]
fn ruls_snapshot_wal_and_future_roots_use_original_recovery() {
    use rynvek_journal::JournalWriter;
    let dir = tempfile::tempdir().unwrap();
    let snap = dir.path().join("s");
    let wal = dir.path().join("w");
    let mut w = world(&fixture());
    std::fs::write(&snap, w.encode_snapshot().unwrap()).unwrap();
    let mut writer = JournalWriter::create_new(&wal, w.world_id, 0, 0, 0).unwrap();
    for _ in 0..3 {
        w.tick_recorded(&[], &mut writer).unwrap();
    }
    drop(writer);
    let (mut r, out) = SimulationEngine::recover(&snap, &wal).unwrap();
    assert_eq!(out.replayed_commits.len(), 3);
    assert_eq!(w.encode_snapshot().unwrap(), r.encode_snapshot().unwrap());
    for _ in 0..3 {
        w.tick().unwrap();
        r.tick().unwrap();
        assert_eq!(w.parent_commit_hash, r.parent_commit_hash);
        closed(&r);
    }
}
#[test]
fn rewrapped_ruls_tampering_cannot_hide_behind_valid_snapshot_envelope() {
    use rynvek_snapshot::{CMIT_TAG, SnapshotReader, SnapshotWriter};
    let mut w = world(&fixture());
    w.tick().unwrap();
    let bytes = w.encode_snapshot().unwrap();
    for field in ["config", "components", "paid_q"] {
        let mut read = SnapshotReader::read_bytes(&bytes).unwrap();
        let commit = rynvek_journal::CommitManifest::decode(read.find_chunk(CMIT_TAG).unwrap()).unwrap();
        let c = read.custom_chunks.iter_mut().find(|c| c.tag == *b"RULS").unwrap();
        let mut v: Value = serde_json::from_slice(&c.payload).unwrap();
        match field {
            "config" => v["config"]["world_seed"] = json!(99),
            "components" => v["components"]["entities"]["steer.one"]["values"]["state.accel_y"] = json!(1),
            _ => v["paid_q"] = json!(0),
        }
        c.payload = serde_json::to_vec(&v).unwrap();
        let forged = SnapshotWriter::write_bytes_with_chunks(
            w.world_id,
            1,
            1,
            commit.parent_commit_hash,
            &read.registry,
            None,
            &read.custom_chunks,
        )
        .unwrap()
        .0;
        assert!(SimulationEngine::decode_snapshot(&forged).is_err(), "{field}");
    }
}
