// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use base64::Engine as _;
use rynvek_core::Blake3Hash;
use rynvek_rts::{
    genome::Genome,
    ids::UnitKind,
    runtime::ArenaRuntime,
    world::{ArenaWorld, MatchConfig, authoring::compile_action_rule},
};
use rynvek_world::{HlcTimestamp, LiveEditSafetyClass, WorldPatch, WorldPatchOp};
fn patch(ops: Vec<WorldPatchOp>) -> WorldPatch {
    WorldPatch {
        patch_id: [42; 16],
        base_authoring_revision: 0,
        hlc: HlcTimestamp::default(),
        safety_class: LiveEditSafetyClass::EpochBoundarySafe,
        operations: ops,
    }
}
fn graph() -> String {
    serde_json::json!({"format":"rynvek.rule-graph.v1","name":"动作规则","writes":[{"component":"unit.actions.move_x","ty":"Q8_8"}],"nodes":[{"op":"ConstQ8_8","imm":"128"},{"op":"EmitComponentWrite","component":"unit.actions.move_x","args":[0]}]}).to_string()
}
#[test]
fn actual_catalogue_rules_are_atomic_consumed_and_restore_replay_identically() {
    let mut runtime = ArenaRuntime::new([21; 16], ArenaWorld::new(MatchConfig::pve(42, 1)));
    let ids = runtime.world().units.keys().copied().collect();
    let start = runtime.world().units.values().next().unwrap().position;
    runtime
        .apply_command(
            rynvek_rts::FactionId(0),
            &rynvek_rts::commands::RtsCommand::IssueOrder {
                units: rynvek_rts::commands::UnitSelector::Ids(ids),
                order: rynvek_rts::ids::OrderKind::Move,
                target: start + rynvek_rts::math::Vec2::tile_center(5, 5),
            },
        )
        .unwrap();
    let source = graph();
    let module = compile_action_rule(&source).unwrap();
    let digest = Blake3Hash::of(&rynvek_rules::encode_rir(&module)).0;
    let mut genome = Genome::seed(UnitKind::Forager);
    genome.slug = "studio.seed".into();
    genome.w_in[0] = 37;
    let mut edit = patch(vec![
        WorldPatchOp::UpsertSeedGenome {
            genome_slug: genome.slug.clone(),
            genome_bytes_b64: base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&genome).unwrap()),
        },
        WorldPatchOp::UpdateRuleModule {
            module_id: "control".into(),
            rule_graph_json: source,
            compiled_rir_blake3: digest,
        },
        WorldPatchOp::RemoveEntityTemplate {
            template_id: "genome/seed.forager.v1".into(),
        },
    ]);
    edit.base_authoring_revision = runtime.revision();
    let mut control = runtime.clone();
    let mut control_edit = edit.clone();
    control_edit
        .operations
        .retain(|op| !matches!(op, WorldPatchOp::UpdateRuleModule { .. }));
    control
        .transact_authoring_at(4, |w| control_edit.apply_at_boundary(w, 4))
        .unwrap();
    let report = runtime
        .transact_authoring_at(4, |w| edit.apply_at_boundary(w, 4))
        .unwrap();
    assert_eq!(report.genomes_updated, 1);
    assert_eq!(report.rules_updated, 1);
    assert_eq!(report.templates_removed, 1);
    assert_eq!(runtime.world().factions[0].genomes["studio.seed"], genome);
    assert!(!runtime.world().factions[0].genomes.contains_key("seed.forager.v1"));
    assert!(runtime.world().units.values().all(|u| u.actions[0] == 128));
    assert!(
        runtime
            .world()
            .units
            .iter()
            .any(|(id, u)| u.position != control.world().units[id].position),
        "规则必须改变原运动系统的实际位置，不只写未消费属性"
    );
    let fork = runtime.clone().fork([22; 16]).unwrap();
    assert_eq!(fork.world().authoring.last_edit_revision, 0);
    assert_eq!(fork.world().authoring.rules, runtime.world().authoring.rules);
    fork.checkpoint().unwrap();
    let checkpoint = runtime.checkpoint().unwrap();
    let mut recovered =
        ArenaRuntime::restore([21; 16], runtime.revision(), runtime.state_root(), &checkpoint.bytes).unwrap();
    runtime.step().unwrap();
    recovered.step().unwrap();
    assert_eq!(runtime.state_root(), recovered.state_root());
    assert_eq!(runtime.world().authoring.rules, recovered.world().authoring.rules);
    let before = runtime.state_root();
    let bad = patch(vec![
        WorldPatchOp::PaintTerrainRect {
            x0: 1,
            y0: 1,
            width: 1,
            height: 1,
            tile_kind: 4,
        },
        WorldPatchOp::UpdateRuleModule {
            module_id: "bad".into(),
            rule_graph_json: graph(),
            compiled_rir_blake3: [0; 32],
        },
    ]);
    assert!(
        runtime
            .transact_authoring_at(8, |w| bad.apply_at_boundary(w, 8))
            .is_err()
    );
    assert_eq!(runtime.state_root(), before);
}
#[test]
fn untrusted_genome_dimensions_and_forbidden_rule_components_are_rejected_without_panic() {
    let mut g = Genome::seed(UnitKind::Forager);
    g.w_out.clear();
    assert!(rynvek_rts::brain::UnitBrain::from_genome(&g).is_err());
    let source = graph().replace("unit.actions.move_x", "energy.stored");
    assert!(compile_action_rule(&source).is_err());
}
