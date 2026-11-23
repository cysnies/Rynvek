// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 原包、原Tick、确定性与有界事务，不冒充一般物理世界。
use rynvek_rts::{
    lab::{ExperimentSpec, ParameterModel, run_experiment},
    world::{ArenaWorld, MatchConfig, tier1::Tier1Draft},
};
use serde_json::json;
const JS: &str = "function tick(i){return {writes:i.entities.map(e=>({id:e.id,values:{'state.total':e.values['state.total']+e.values['param.rate']}}))};}";
fn draft(js: &str) -> Tier1Draft {
    serde_json::from_value(json!({"package_id":"local.counter","version":"0.1.0","template_id":"model.counter","reads":["state.total","param.rate"],"writes":["state.total"],"source_js":js,"source_ts":null})).unwrap()
}
fn model() -> ParameterModel {
    let mut m: ParameterModel =
        serde_json::from_str(include_str!("../../../examples/lab/parameter-counter.json")).unwrap();
    m.rules.clear();
    m.bindings.clear();
    m.tier1.insert("counter.script".into(), draft(JS).build().unwrap());
    m
}
fn world(m: &ParameterModel) -> ArenaWorld {
    let mut w = ArenaWorld::new(MatchConfig::parameters(57));
    m.install(&mut w).unwrap();
    w
}
fn total(w: &ArenaWorld) -> i32 {
    w.parameter_state.entities["counter.one"].values["state.total"]
}
#[test]
fn real_package_tick_capture_snapshot_and_lab_sweep() {
    let m = model();
    let mut w = world(&m);
    w.run(2);
    assert_eq!(total(&w), 6);
    assert!((10000..=500000).contains(&w.parameter_state.tier1_gas_charged));
    assert_eq!(ParameterModel::capture(&w.authoring).unwrap().tier1, m.tier1);
    let (bytes, _) = w.encode_snapshot([45; 16], 2, [0; 32]).unwrap();
    let mut recovered = ArenaWorld::decode_snapshot(&bytes).unwrap();
    w.run(2);
    recovered.run(2);
    assert_eq!(w.state_hash(), recovered.state_hash());
    assert_eq!(total(&w), 12);
    let mut fast = m.clone();
    fast.instances
        .get_mut("counter.one")
        .unwrap()
        .parameters
        .insert("param.rate".into(), 7);
    let s:ExperimentSpec=serde_json::from_value(json!({"name":"Tier1真实扫描","seeds":[57,58],"ticks":4,"sample_every":2,"variants":[{"label":"slow","mode":"parameters","policy":"idle","model":m},{"label":"fast","mode":"parameters","policy":"idle","model":fast}]})).unwrap();
    s.validate().unwrap();
    let a = run_experiment(&s, &|_, _| {});
    assert_eq!(a, run_experiment(&s, &|_, _| {}));
    assert_eq!(
        a.metrics
            .iter()
            .filter(|r| r.parameter_component.as_deref() == Some("state.total"))
            .map(|r| r.parameter_value.unwrap())
            .collect::<Vec<_>>(),
        vec![6, 12, 6, 12, 14, 28, 14, 28]
    );
    assert_eq!(a.runs[0].final_state_root, w.state_hash().0);
    #[cfg(feature = "arrow")]
    {
        use rynvek_rts::lab::arrow_io::{LabTable, parquet_page, table_batch, to_parquet};
        for t in LabTable::ALL {
            let b = table_batch(&a, t).unwrap();
            let p = parquet_page(to_parquet(&b).unwrap(), 0, 100).unwrap();
            assert_eq!(p.total_rows, b.num_rows());
            if t == LabTable::Metrics {
                assert!(
                    p.rows
                        .iter()
                        .any(|r| r["parameter_component"] == "state.total" && r["parameter_value"] == 28)
                );
            }
        }
    }
}
#[test]
fn untrusted_top_level_loop_host_access_and_runtime_loop_fail_bounded() {
    for js in [
        "while(true){}",
        "function tick(i){while(true){}}",
        "function tick(i){Math.random();}",
        "function tick(i){return Date.now();}",
        "function tick(i){return fetch('https://invalid.test');}",
        "function tick(i){return process.env;}",
    ] {
        let mut m = model();
        m.tier1.insert("counter.script".into(), draft(js).build().unwrap());
        let mut w = world(&m);
        w.run(1);
        assert!(w.parameter_state.fault.is_some());
        assert_eq!(total(&w), 0);
        assert_eq!(w.parameter_state.tier1_gas_charged, 500000);
        let (b, _) = w.encode_snapshot([45; 16], 1, [0; 32]).unwrap();
        assert_eq!(w.state_hash(), ArenaWorld::decode_snapshot(&b).unwrap().state_hash());
    }
}
#[test]
fn no_cross_tick_hidden_state() {
    let mut m = model();
    m.tier1.insert(
        "counter.script".into(),
        draft("let n=0;function tick(i){++n;return {writes:i.entities.map(e=>({id:e.id,values:{'state.total':n}}))};}")
            .build()
            .unwrap(),
    );
    let mut w = world(&m);
    w.run(3);
    assert_eq!(total(&w), 1);
}
#[test]
fn access_container_limits_tamper_and_tier0_write_conflicts_reject() {
    let mut m = model();
    let mut d = draft(JS);
    d.reads.push("state.missing".into());
    m.tier1.insert("counter.script".into(), d.build().unwrap());
    assert!(m.compile().is_err());
    d.writes = vec!["param.rate".into()];
    assert!(d.build().is_err());
    let mut b = draft(JS).build().unwrap();
    b.package_hex.replace_range(100..102, "ff");
    assert!(b.checked().is_err());
    let mut d = draft(JS);
    d.source_js = "x".repeat(8193);
    assert!(d.build().is_err());
    let mut m: ParameterModel =
        serde_json::from_str(include_str!("../../../examples/lab/parameter-counter.json")).unwrap();
    m.tier1.insert("conflict".into(), draft(JS).build().unwrap());
    assert!(m.compile().is_err());
}
#[test]
fn strict_typed_outputs_and_all_entities_commit_or_none() {
    for v in [
        "{'state.total':2147483648}",
        "{'state.total':1.5}",
        "{'state.total':true}",
        "{'param.rate':3}",
        "{'state.total':1,'state.other':2}",
    ] {
        let mut m = model();
        m.tier1.insert(
            "counter.script".into(),
            draft(&format!(
                "function tick(i){{return {{writes:i.entities.map(e=>({{id:e.id,values:{v}}}))}};}}"
            ))
            .build()
            .unwrap(),
        );
        let mut w = world(&m);
        w.run(1);
        assert!(w.parameter_state.fault.is_some());
        assert_eq!(total(&w), 0);
    }
    let mut m = model();
    m.instances
        .insert("counter.two".into(), m.instances["counter.one"].clone());
    m.tier1.insert("counter.script".into(),draft("function tick(i){return {writes:[{id:i.entities[0].id,values:{'state.total':17}},{id:'forged',values:{'state.total':22}}]};}").build().unwrap());
    let mut w = world(&m);
    w.run(1);
    assert!(w.parameter_state.fault.is_some());
    assert!(
        w.parameter_state
            .entities
            .values()
            .all(|e| e.values["state.total"] == 0)
    );
}
#[test]
fn tier0_and_tier1_read_same_old_state_and_rollback_together() {
    let mut m: ParameterModel =
        serde_json::from_str(include_str!("../../../examples/lab/parameter-counter.json")).unwrap();
    let c = m.templates["model.counter"].components["state.total"].clone();
    m.templates
        .get_mut("model.counter")
        .unwrap()
        .components
        .insert("state.other".into(), c);
    let mut d = draft(
        "function tick(i){return {writes:i.entities.map(e=>({id:e.id,values:{'state.other':e.values['state.total']}}))};}",
    );
    d.writes = vec!["state.other".into()];
    m.tier1.insert("other".into(), d.build().unwrap());
    let mut w = world(&m);
    w.run(2);
    assert_eq!(total(&w), 6);
    assert_eq!(w.parameter_state.entities["counter.one"].values["state.other"], 3);
    d.source_js = "function tick(i){return {writes:[]};}".into();
    m.tier1.insert("other".into(), d.build().unwrap());
    let mut w = world(&m);
    w.run(1);
    assert!(w.parameter_state.fault.is_some());
    assert_eq!(total(&w), 0);
}

#[test]
fn declared_only_inputs_and_raw_bool_q8_are_enforced() {
    let mut m = model();
    let mut d = draft(
        "function tick(i){if(Object.keys(i).sort().join(',')!=='entities,tick,world_seed'||i.entities[0].values['state.flag']!==undefined)throw new Error('hidden input');return {writes:i.entities.map(e=>({id:e.id,values:{'state.total':e.values['param.rate'],'state.bias':-127,'state.flag':0}}))};}",
    );
    d.writes = vec!["state.total".into(), "state.bias".into(), "state.flag".into()];
    m.tier1.insert("counter.script".into(), d.build().unwrap());
    let mut w = world(&m);
    w.run(1);
    assert!(w.parameter_state.fault.is_none());
    assert_eq!(w.parameter_state.entities["counter.one"].values["state.bias"], -127);
    assert_eq!(w.parameter_state.entities["counter.one"].values["state.flag"], 0);
    d.source_js = d.source_js.replace("'state.bias':-127", "'state.bias':32768");
    m.tier1.insert("counter.script".into(), d.build().unwrap());
    let mut w = world(&m);
    w.run(1);
    assert!(w.parameter_state.fault.is_some());
    assert_eq!(total(&w), 0);
}

#[test]
fn valid_container_with_foreign_engine_range_cannot_execute() {
    use rynvek_core::package::{PackageInput, PackageInputEntry, PackageKind, read_package, write_package};
    let mut i = draft(JS).build().unwrap();
    let bytes = (0..i.package_hex.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&i.package_hex[n..n + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let p = read_package(&bytes, Some(PackageKind::Rpack)).unwrap();
    let bytes = write_package(&PackageInput {
        kind: PackageKind::Rpack,
        id: p.manifest.id.clone(),
        version: p.manifest.version.clone(),
        name: p.manifest.name.clone(),
        license: p.manifest.license.clone(),
        engine_compat: ">=99.0.0 <100.0.0".into(),
        entries: p
            .manifest
            .entries
            .iter()
            .map(|e| PackageInputEntry {
                path: e.path.clone(),
                kind: e.kind.clone(),
                media_type: e.media_type.clone(),
                data: p.file(&e.path).unwrap().to_vec(),
            })
            .collect(),
    })
    .unwrap();
    i.package_hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert!(i.checked().is_err());
}
