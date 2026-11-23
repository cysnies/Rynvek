// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use rynvek_core::{Blake3Hash,RynvekErrorCode};
use rynvek_rts::{ArenaWorld, MatchConfig, runtime::ArenaRuntime};
use rynvek_world::{HlcTimestamp, LiveEditSafetyClass, WorldPatch, WorldPatchOp};
use serde_json::{Value, json};
fn set(key: &str, v: Value) -> WorldPatchOp {
    WorldPatchOp::SetWorldParam {
        key_path: key.into(),
        new_value_json: v.to_string(),
    }
}
fn patch(ops: Vec<WorldPatchOp>) -> WorldPatch {
    WorldPatch {
        patch_id: [77; 16],
        base_authoring_revision: 0,
        hlc: HlcTimestamp::default(),
        safety_class: LiveEditSafetyClass::EpochBoundarySafe,
        operations: ops,
    }
}
fn template() -> Value {
    json!({"schema":"rynvek.parameter-template.v1","components":{"state.total":{"ty":"I32","default":0},"param.rate":{"ty":"I32","default":3}}})
}
fn graph() -> String {
    json!({"format":"rynvek.rule-graph.v1","name":"参数实体积分器","reads":[{"component":"state.total","ty":"I32"},{"component":"param.rate","ty":"I32"}],"writes":[{"component":"state.total","ty":"I32"}],"nodes":[{"op":"LoadComponent","component":"state.total"},{"op":"LoadComponent","component":"param.rate"},{"op":"Add","args":[0,1]},{"op":"EmitComponentWrite","component":"state.total","args":[2]}]}).to_string()
}
fn create() -> Vec<WorldPatchOp> {
    let source = graph();
    let m = rynvek_rts::world::authoring::compile_authored_rule_detailed(&source).unwrap();
    vec![
        set("rules.templates.model.counter", template()),
        set(
            "rules.instances.counter.one",
            json!({"template_id":"model.counter","parameters":{"param.rate":3}}),
        ),
        WorldPatchOp::UpdateRuleModule {
            module_id: "counter".into(),
            rule_graph_json: source,
            compiled_rir_blake3: Blake3Hash::of(&rynvek_rules::encode_rir(&m)).0,
        },
        set(
            "rules.bindings.counter.step",
            json!({"module_id":"counter","template_id":"model.counter"}),
        ),
    ]
}
#[test]
fn non_role_template_executes_in_engine_preserves_state_and_comp_roots_restore() {
    let mut r = ArenaRuntime::new([77; 16], ArenaWorld::new(MatchConfig::pve(42, 1)));
    let units = r.world().units.len();
    r.transact_authoring_at(4, |w| patch(create()).apply_at_boundary(w, 4))
        .unwrap();
    assert_eq!(r.world().units.len(), units);
    let v = r.world().parameter_state.entities["counter.one"].values["state.total"];
    r.step().unwrap();
    assert_eq!(
        r.world().parameter_state.entities["counter.one"].values["state.total"],
        v + 3
    );
    r.transact_authoring_at(8, |w| {
        patch(vec![set(
            "rules.instances.counter.one",
            json!({"template_id":"model.counter","parameters":{"param.rate":7}}),
        )])
        .apply_at_boundary(w, 8)
    })
    .unwrap();
    let v = r.world().parameter_state.entities["counter.one"].values["state.total"];
    r.step().unwrap();
    assert_eq!(
        r.world().parameter_state.entities["counter.one"].values["state.total"],
        v + 7
    );
    let canonical = r.world().canonical_state();
    let mut changed = r.world().clone();
    changed
        .parameter_state
        .entities
        .get_mut("counter.one")
        .unwrap()
        .values
        .insert("state.total".into(), 12345);
    assert_eq!(changed.canonical_state().config_root, canonical.config_root);
    assert_ne!(changed.canonical_state().entity_root, canonical.entity_root);
    let cp = r.checkpoint().unwrap();
    let mut restored = ArenaRuntime::restore([77; 16], r.revision(), r.state_root(), &cp.bytes).unwrap();
    for _ in 0..5 {
        r.step().unwrap();
        restored.step().unwrap();
        assert_eq!(r.state_root(), restored.state_root());
    }
    assert!(r.world().parameter_state.fault.is_none());
}
#[test]
fn registry_refs_permissions_types_resource_limits_and_delete_are_atomic() {
    let mut w = ArenaWorld::new(MatchConfig::pve(42, 1));
    patch(create()).apply_at_boundary(&mut w, 0).unwrap();
    let before = w.state_hash();
    let mut bad_shape = template();
    bad_shape["components"]["state.total"]["ty"] = json!("Bool");
    let bad_ops = vec![
        vec![set("rules.templates.model.counter", bad_shape)],
        vec![set(
            "rules.instances.counter.one",
            json!({"template_id":"missing","parameters":{}}),
        )],
        vec![set(
            "rules.instances.counter.one",
            json!({"template_id":"model.counter","parameters":{"state.total":9}}),
        )],
        vec![set(
            "rules.bindings.duplicate",
            json!({"module_id":"counter","template_id":"model.counter"}),
        )],
        vec![set("rules.templates.model.counter", Value::Null)],
        vec![WorldPatchOp::RemoveRuleModule {
            module_id: "counter".into(),
        }],
        (0..129)
            .map(|i| {
                set(
                    &format!("rules.instances.over{i}"),
                    json!({"template_id":"model.counter","parameters":{}}),
                )
            })
            .collect(),
    ];
    for mut ops in bad_ops {
        ops.insert(
            0,
            WorldPatchOp::PaintTerrainRect {
                x0: 1,
                y0: 1,
                width: 1,
                height: 1,
                tile_kind: 4,
            },
        );
        assert!(patch(ops).apply_at_boundary(&mut w, 0).is_err());
        assert_eq!(w.state_hash(), before);
    }
    assert!(rynvek_rts::world::authoring::compile_action_rule(&graph()).is_err());
    for source in [
        graph().replace("state.total", "unit.hp"),
        graph().replace("state.total", "param.total"),
    ] {
        assert!(rynvek_rts::world::authoring::compile_authored_rule_detailed(&source).is_err());
    }
    let del = WorldPatchOp::RemoveEntityTemplate {
        template_id: "parameter/model.counter".into(),
    };
    assert_eq!(del.required_safety_class(), LiveEditSafetyClass::EpochBoundarySafe);
    assert!(patch(vec![del.clone()]).apply_at_boundary(&mut w, 0).is_err());
    patch(vec![
        set("rules.bindings.counter.step", Value::Null),
        set("rules.instances.counter.one", Value::Null),
        del,
    ])
    .apply_at_boundary(&mut w, 0)
    .unwrap();
    assert!(w.parameter_state.entities.is_empty());
    assert!(w.authoring.parameter_templates.is_empty());
    let _ = RynvekErrorCode::PluginCapabilityDenied;
}
