// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 原生生成并复核浏览器能力边界夹具，不向浏览器提供可执行原生沙箱替身。
use rynvek_rules::components::tier1::Tier1Draft;
use rynvek_world::general::GeneralWorldConfig;
#[test]
fn native_tier1_remains_paid_and_browser_fixture_is_reproducible() {
    let mut config: GeneralWorldConfig =
        serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap();
    let tier0 = config.build_package("world.abi.tier0", "1.0.0").unwrap();
    let rules = config.components.as_mut().unwrap();
    rules.rule_graphs.clear();
    rules.bindings.clear();
    rules.tier1_rules.insert("steer".into(), Tier1Draft {
        package_id: "rule.abi.tier1".into(), version: "1.0.0".into(), template_id: "model.steer".into(),
        reads: vec!["param.thrust".into()], writes: vec!["state.accel_x".into()], source_ts: None,
        source_js: "function tick(input) { return {writes: input.entities.map(e => ({id:e.id,values:{\"state.accel_x\":e.values[\"param.thrust\"]}}))}; }".into(),
    }.build().unwrap());
    let tier1 = config.build_package("world.abi.tier1", "1.0.0").unwrap();
    let mut world = config.instantiate([1; 16]).unwrap();
    world.tick_with_commands(&[]).unwrap();
    assert!(world.component_rules().unwrap().tier1_quota > 0);
    assert!(world.component_rules().unwrap().paid_q > 0);
    assert_eq!(
        world.physics().body(1).unwrap().acceleration_q32[0],
        rynvek_simulation::physics::ONE
    );
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let bodies: Vec<_> = world
        .physics()
        .iter()
        .map(|(serial, b)| serde_json::json!({"serial":serial,"positionQ32":b.position_q32,"radiusQ32":b.radius_q32}))
        .collect();
    let frame = serde_json::json!({"format":"rynvek-general-frame/1","worldId":hex(&world.world_id),"tick":world.clock.tick,"revision":world.clock.world_revision,"stateRoot":hex(&world.parent_commit_hash),"bodies":bodies});
    let fixture = serde_json::json!({"format":"rynvek-browser-world-abi/1", "tier0Hex":hex(&tier0),"tier1Hex":hex(&tier1),"tier1Frame":frame});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/engine/general_world_packages.json");
    if std::env::var_os("RYNVEK_UPDATE_BROWSER_FIXTURE").is_some() {
        std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap())).unwrap();
    }
    let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(actual, fixture, "原生唯一写入器与浏览器夹具必须完全一致");
}
