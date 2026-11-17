// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use rynvek_rts::{
    lab::{ExperimentSpec, ParameterModel, run_experiment, run_one_interruptible},
    world::{ArenaWorld, MatchConfig},
};
use serde_json::json;
fn model() -> ParameterModel {
    serde_json::from_str(include_str!("../../../examples/lab/parameter-counter.json")).unwrap()
}
fn spec() -> ExperimentSpec {
    serde_json::from_value(json!({"name":"参数真实扫描","seeds":[57],"ticks":4,"sample_every":2,"variants":[{"label":"slow","mode":"parameters","policy":"idle","model":model()}]})).unwrap()
}
#[test]
fn parameter_models_execute_original_engine_and_snapshot_roots_recover() {
    let mut spec = spec();
    let mut fast = spec.variants[0].clone();
    fast.label = "fast".into();
    fast.model
        .as_mut()
        .unwrap()
        .instances
        .get_mut("counter.one")
        .unwrap()
        .parameters
        .insert("param.rate".into(), 7);
    spec.variants.push(fast);
    spec.validate().unwrap();
    let a = run_experiment(&spec, &|_, _| {});
    let b = run_experiment(&spec, &|_, _| {});
    assert_eq!(a, b);
    let totals: Vec<_> = a
        .metrics
        .iter()
        .filter(|r| r.parameter_component.as_deref() == Some("state.total"))
        .map(|r| (r.run, r.tick, r.parameter_value))
        .collect();
    assert_eq!(
        totals,
        vec![(0, 2, Some(6)), (0, 4, Some(12)), (1, 2, Some(14)), (1, 4, Some(28))]
    );
    assert!(a.lineage.is_empty());
    assert!(a.energy.iter().all(|e| !e.enabled && e.live_bodies == 0));
    assert!(
        a.metrics
            .iter()
            .all(|m| m.faction.is_none() && m.units.is_none() && m.score_milli.is_none())
    );
    assert_ne!(a.runs[0].model_digest, a.runs[1].model_digest);
    assert_ne!(a.runs[0].final_state_root, a.runs[1].final_state_root);
    assert!(
        a.runs
            .iter()
            .all(|r| r.end_reason == "tick_budget" && r.players == 0 && r.natural_births == 0)
    );
    let mut world = ArenaWorld::new(MatchConfig::parameters(57));
    model().install(&mut world).unwrap();
    world.run(2);
    let (bytes, _) = world.encode_snapshot([38; 16], 2, [0; 32]).unwrap();
    let mut restored = ArenaWorld::decode_snapshot(&bytes).unwrap();
    assert_eq!(world.state_hash(), restored.state_hash());
    world.run(2);
    restored.run(2);
    assert_eq!(world.state_hash(), restored.state_hash());
    assert_eq!(world.state_hash().0, a.runs[0].final_state_root);
    assert!(world.units.is_empty() && world.factions.is_empty() && world.buildings.is_empty());
}
#[test]
fn parameter_models_are_bounded_authoritative_and_cancellable() {
    let mut s = spec();
    s.variants[0].players = Some(1);
    assert!(s.validate().is_err());
    let mut s = spec();
    s.variants[0].policy = rynvek_rts::lab::PolicyKind::Economy;
    assert!(s.validate().is_err());
    let mut s = spec();
    s.ticks = 108000;
    s.sample_every = 1;
    assert!(s.validate().is_err());
    let mut m = model();
    m.outputs.push(m.outputs[0].clone());
    assert!(m.compile().is_err());
    let mut m = model();
    m.instances
        .get_mut("counter.one")
        .unwrap()
        .parameters
        .insert("state.total".into(), 7);
    assert!(m.compile().is_err());
    let mut m = model();
    m.bindings
        .insert("duplicate.writer".into(), m.bindings["counter.step"].clone());
    assert!(m.compile().is_err());
    let mut m = model();
    m.rules.get_mut("counter").unwrap().push_str("not JSON");
    assert!(m.compile().is_err());
    let m = model();
    assert!(m.install(&mut ArenaWorld::new(MatchConfig::pve(57, 1))).is_err());
    let s = spec();
    let calls = std::cell::Cell::new(0);
    let r = run_one_interruptible(&s.runs()[0], 4, 2, &|| {
        let n = calls.get();
        calls.set(n + 1);
        n >= 2
    });
    assert!(r.is_none());
}
#[cfg(feature = "arrow")]
#[test]
fn parameter_four_tables_are_actual_parquet_with_exact_raw_values_and_null_rts_metrics() {
    use rynvek_rts::lab::arrow_io::{LabTable, parquet_page, table_batch, to_parquet};
    let s = spec();
    s.validate().unwrap();
    let r = run_experiment(&s, &|_, _| {});
    for t in LabTable::ALL {
        let batch = table_batch(&r, t).unwrap();
        let bytes = to_parquet(&batch).unwrap();
        assert_eq!(bytes, to_parquet(&batch).unwrap());
        let page = parquet_page(bytes, 0, 100).unwrap();
        assert_eq!(page.total_rows, batch.num_rows());
        if t == LabTable::Metrics {
            assert!(
                page.rows
                    .iter()
                    .any(|r| r["parameter_type"] == "Q8_8" && r["parameter_value"] == -128)
            );
            assert!(
                page.rows
                    .iter()
                    .all(|r| r["faction"].is_null() && r["biomass_milli"].is_null())
            );
        }
        if t == LabTable::Lineage {
            assert_eq!(page.total_rows, 0);
        }
        if t == LabTable::Energy {
            assert!(page.rows.iter().all(|r| r["enabled"] == false));
        }
    }
}
