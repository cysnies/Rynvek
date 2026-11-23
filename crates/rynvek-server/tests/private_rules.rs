// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 私有签名发布→发现/下载验签→显式Actor安装→有源计价Tick→文件CAS恢复；不冒称PG多机灾备。
mod common;
use common::*;
use rynvek_core::Blake3Hash;
use rynvek_protocol::{CapabilityScope, CommandEnvelope, IdempotencyKey, WireUuid};
use rynvek_server::{
    AppConfig, RunningServer, ServerConfig, client,
    general_host::{GeneralInput, GeneralTickRecord},
    ids::new_uuid_v7,
    store::Store,
};
use rynvek_simulation::physics::PhysicsCommand;
use rynvek_world::general::GeneralWorldConfig;
use serde_json::{Value, json};

fn rules() -> GeneralWorldConfig {
    serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap()
}
async fn start(store: Store, path: &std::path::Path) -> RunningServer {
    rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: AppConfig {
            public_base_url: String::new(),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            lobby_timeout: None,
            blob_dir: Some(path.to_owned()),
            ..AppConfig::default()
        },
        store,
        signer: server_key(),
    })
    .await
    .unwrap()
}
async fn create(s: &RunningServer, u: &User, c: &GeneralWorldConfig) -> WireUuid {
    let bytes = c.build_package("world.test.general-authority", "0.1.0").unwrap();
    let r = client::request(
        "POST",
        &format!("{}/api/v1/worlds/general", s.base_url()),
        &[
            ("Authorization", &format!("Bearer {}", u.token)),
            ("Content-Type", "application/vnd.rynvek.rworld"),
        ],
        Some(&bytes),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 201, "{:?}", r.json());
    r.json().unwrap()["world_id"].as_str().unwrap().parse().unwrap()
}
async fn action(s: &RunningServer, u: &User, id: WireUuid, v: Value) -> client::HttpResponse {
    client::send_json(
        "POST",
        &format!("{}/api/v1/worlds/{id}/general", s.base_url()),
        Some(&u.token),
        &v,
    )
    .await
    .unwrap()
}
async fn ok_action(s: &RunningServer, u: &User, id: WireUuid, v: Value) -> Value {
    let r = action(s, u, id, v).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
async fn connect(s: &RunningServer, u: &User, id: WireUuid) -> u64 {
    ok_action(s, u, id, json!({"action":"connect"})).await["host_fence"]
        .as_u64()
        .unwrap()
}
async fn acquire(s: &RunningServer, u: &User, id: WireUuid, h: u64, level: &str) -> u64 {
    ok_action(s, u, id, json!({"action":"acquire","host_fence":h,"level":level})).await["lease"]["fencing_token"]
        .as_u64()
        .unwrap()
}
fn input(u: &User, h: u64, l: u64, tick: u64, commands: Vec<PhysicsCommand>) -> GeneralInput {
    GeneralInput {
        install: None,
        recovery: None,
        host_fence: h,
        control_fence: l,
        envelope: CommandEnvelope {
            idempotency_key: IdempotencyKey::new(new_uuid_v7()).unwrap(),
            principal_id: u.principal_id,
            issued_at_tick: tick,
            target_tick: tick + 1,
            payload: commands,
        },
    }
}
async fn step(s: &RunningServer, u: &User, id: WireUuid, i: &GeneralInput) -> client::HttpResponse {
    action(s, u, id, json!({"action":"tick","input":i})).await
}
async fn view(s: &RunningServer, u: &User, id: WireUuid) -> Value {
    let r = client::get(&format!("{}/api/v1/worlds/{id}/general", s.base_url()), Some(&u.token))
        .await
        .unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
use base64::Engine as _;
use rynvek_rules::package::{Tier0RulePackage, build_tier0_package};
use rynvek_server::rule_package::{
    self as rp, ExactRuleRef, RuleInstallRequest, RuleReleaseManifest, SignedRuleRelease,
};
use std::collections::BTreeMap;
fn root_graph() -> String {
    json!({"format":"rynvek.rule-graph.v1","name":"付费半推力","reads":[{"component":"param.thrust","ty":"Q8_8"}],"writes":[{"component":"state.accel_x","ty":"Q8_8"}],"nodes":[{"op":"LoadComponent","component":"param.thrust"},{"op":"ConstQ8_8","imm":"128"},{"op":"Mul","args":[0,1]},{"op":"EmitComponentWrite","component":"state.accel_x","args":[2]}]}).to_string()
}
fn support_graph() -> String {
    rules().components.unwrap().rule_graphs["steer"].replace("state.accel_x", "state.accel_y")
}
fn bundle_world() -> GeneralWorldConfig {
    let mut c = rules();
    let r = c.components.as_mut().unwrap();
    r.templates
        .get_mut("model.steer")
        .unwrap()
        .components
        .get_mut("param.thrust")
        .unwrap()
        .default = 128;
    r.bindings
        .insert("support.step".into(), r.bindings["steer.step"].clone());
    r.bindings.get_mut("support.step").unwrap().module_id = "support".into();
    r.rule_graphs.insert("support".into(), support_graph());
    c
}
fn pack(id: &str, graph: &str) -> Vec<u8> {
    build_tier0_package(id, "1.0.0", "MIT", graph).unwrap()
}
fn manifest(u: &User, bytes: &[u8], dependencies: Vec<ExactRuleRef>) -> RuleReleaseManifest {
    let p = Tier0RulePackage::parse(bytes).unwrap();
    RuleReleaseManifest {
        format: "rynvek-private-rule-release/1".into(),
        owner_id: u.principal_id.to_string(),
        package_id: p.id,
        version: p.version,
        artifact_cid: Blake3Hash::of(bytes).to_hex(),
        dependencies,
    }
}
fn signed(u: &User, manifest: &RuleReleaseManifest, bytes: &[u8]) -> SignedRuleRelease {
    SignedRuleRelease {
        manifest: manifest.clone(),
        publisher_key: rynvek_server::ids::hex(&u.key.public_key()),
        signature: rynvek_server::ids::hex(&u.key.sign(&rp::signing_message(manifest, bytes).unwrap())),
    }
}
async fn request(s: &RunningServer, u: Option<&User>, method: &str, path: &str, body: Value) -> client::HttpResponse {
    client::send_json(
        method,
        &format!("{}{path}", s.base_url()),
        u.map(|u| u.token.as_str()),
        &body,
    )
    .await
    .unwrap()
}
async fn publish(
    s: &RunningServer,
    u: &User,
    bytes: &[u8],
    deps: Vec<ExactRuleRef>,
) -> (RuleReleaseManifest, client::HttpResponse) {
    let m = manifest(u, bytes, deps);
    let sig = signed(u, &m, bytes);
    let r=request(s,Some(u),"PUT",&format!("/api/v1/private-hub/rules/{}/{}",m.package_id,m.version),json!({"artifact_b64":base64::engine::general_purpose::STANDARD.encode(bytes),"manifest":m,"signature":sig.signature})).await;
    (m, r)
}
async fn published(s: &RunningServer, u: &User, bytes: &[u8], deps: Vec<ExactRuleRef>) -> RuleReleaseManifest {
    let (m, r) = publish(s, u, bytes, deps).await;
    assert_eq!(r.status, 201, "{:?}", r.json());
    m
}
fn install(u: &User, h: u64, l: u64, tick: u64, root: &RuleReleaseManifest, modules: &[(&str, &str)]) -> GeneralInput {
    let mut i = input(u, h, l, tick, vec![]);
    i.install = Some(RuleInstallRequest {
        root: root.reference(),
        modules: modules.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
    });
    i
}
async fn first_three(s: &RunningServer, u: &User, id: WireUuid, h: u64, l: u64) {
    for t in 0..3 {
        let r = step(s, u, id, &input(u, h, l, t, vec![])).await;
        assert_eq!(r.status, 200, "{:?}", r.json());
    }
}
async fn install_action(s: &RunningServer, u: &User, id: WireUuid, i: &GeneralInput) -> client::HttpResponse {
    action(s, u, id, json!({"action":"install","input":i})).await
}
async fn run_online_chain(graph: &str, anchor: bool) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "rule-chain-owner", "human", 101).await;
    let other = register(&s.base_url(), "rule-chain-other", "human", 102).await;
    let cfg = bundle_world();
    let id = create(&s, &u, &cfg).await;
    let initial = view(&s, &u, id).await;
    let dep_bytes = pack("rules.private.support", &support_graph());
    let dep = published(&s, &u, &dep_bytes, vec![]).await;
    let bytes = pack("rules.private.steer", graph);
    let root = published(&s, &u, &bytes, vec![dep.reference()]).await;
    assert_eq!(view(&s, &u, id).await, initial, "发布/发现不取得控制权或隐式执行");
    let list = request(&s, Some(&u), "GET", "/api/v1/private-hub/rules", json!({})).await;
    assert_eq!(list.status, 200);
    assert_eq!(list.json().unwrap()["packages"].as_array().unwrap().len(), 2);
    let base = format!("/api/v1/private-hub/rules/{}/{}", root.package_id, root.version);
    assert_eq!(request(&s, None, "GET", &base, json!({})).await.status, 401);
    assert_eq!(request(&s, Some(&other), "GET", &base, json!({})).await.status, 404);
    assert_eq!(
        request(&s, Some(&other), "GET", &format!("{base}/artifact"), json!({}))
            .await
            .status,
        404
    );
    let d = request(&s, Some(&u), "GET", &format!("{base}/resolved"), json!({})).await;
    assert_eq!(d.status, 200, "{:?}", d.json());
    assert_eq!(d.json().unwrap()["releases"].as_array().unwrap().len(), 2);
    assert_eq!(d.json().unwrap()["installed"], false);
    let meta = request(&s, Some(&u), "GET", &base, json!({})).await.json().unwrap();
    let release: SignedRuleRelease = serde_json::from_value(meta["release"].clone()).unwrap();
    let download = client::get(&format!("{}{base}/artifact", s.base_url()), Some(&u.token))
        .await
        .unwrap();
    assert_eq!(download.status, 200);
    assert_eq!(download.body, bytes);
    assert_eq!(
        rp::verify_download(u.principal_id, &u.key.public_key(), &release, &download.body)
            .unwrap()
            .graph_json,
        Tier0RulePackage::parse(&bytes).unwrap().graph_json
    );
    let mut bad = release.clone();
    bad.manifest.dependencies.clear();
    assert!(rp::verify_download(u.principal_id, &u.key.public_key(), &bad, &bytes).is_err());
    assert!(rp::verify_download(u.principal_id, &other.key.public_key(), &release, &bytes).is_err());
    let mut corrupt = bytes.clone();
    let n = corrupt.len() - 1;
    corrupt[n] ^= 1;
    assert!(rp::verify_download(u.principal_id, &u.key.public_key(), &release, &corrupt).is_err());
    assert!(s.state.inner.blobs.get(&Blake3Hash::of(&bytes)).is_err());
    assert_eq!(
        request(
            &s,
            Some(&u),
            "GET",
            &format!("/api/v1/hub/blobs/{}", root.artifact_cid),
            json!({})
        )
        .await
        .status,
        404
    );
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    first_three(&s, &u, id, h, l).await;
    let before = view(&s, &u, id).await;
    let i = install(
        &u,
        h,
        l,
        3,
        &root,
        &[("rules.private.steer", "steer"), ("rules.private.support", "support")],
    );
    let r = install_action(&s, &u, id, &i).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    let after = view(&s, &u, id).await;
    assert_eq!(after["commit"]["tick"], 4);
    assert_eq!(after["installed_releases"].as_object().unwrap().len(), 2);
    assert_eq!(after["bodies"][0]["body"]["acceleration_q32"][0], 1i64 << 30);
    assert!(after["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_eq!(after["life"]["genesis_energy_q"], after["life"]["universe_energy_q"]);
    assert_ne!(before["bodies"], after["bodies"]);
    assert_eq!(install_action(&s, &u, id, &i).await.json().unwrap()["duplicate"], true);
    let mut collision = i.clone();
    collision
        .install
        .as_mut()
        .unwrap()
        .modules
        .insert("rules.private.steer".into(), "support".into());
    assert_eq!(install_action(&s, &u, id, &collision).await.status, 409);
    let commits = store.list_commits(id).await.unwrap();
    let raw = s
        .state
        .inner
        .world_snapshots
        .get(&Blake3Hash(commits[4].journal_segment_cid))
        .unwrap();
    let record: GeneralTickRecord = serde_json::from_slice(&raw).unwrap();
    assert_eq!(record.format, "rynvek-general-tick/2");
    let mut original = cfg.instantiate(id.0).unwrap();
    for _ in 0..3 {
        original.tick().unwrap();
    }
    let replay = record.replay(&original).unwrap();
    assert_eq!(
        after["commit"]["state_root"],
        rynvek_server::ids::hex(&replay.parent_commit_hash)
    );
    let end = if anchor { 301 } else { 5 };
    for tick in 4..end {
        let r = step(&s, &u, id, &input(&u, h, l, tick, vec![])).await;
        assert_eq!(r.status, 200, "Tick {tick}: {:?}", r.json());
    }
    if anchor {
        assert!(store.list_commits(id).await.unwrap()[300].snapshot_cid.is_some());
    }
    let expected = view(&s, &u, id).await;
    s.shutdown().await;
    let s = start(store.clone(), dir.path()).await;
    assert_eq!(view(&s, &u, id).await, expected);
    let h2 = connect(&s, &u, id).await;
    let l2 = acquire(&s, &u, id, h2, "Sovereign").await;
    let mut duplicate = i.clone();
    duplicate.host_fence = h2;
    duplicate.control_fence = l2;
    assert_ne!(install_action(&s, &u, id, &i).await.status, 200);
    assert_eq!(
        install_action(&s, &u, id, &duplicate).await.json().unwrap()["duplicate"],
        true
    );
    // 撤销不抹除原私有历史；后续活Tick/下载必须拒绝，世界根不动。
    let revoke = request(
        &s,
        Some(&u),
        "DELETE",
        &format!("/api/v1/private-hub/rules/{}/{}", dep.package_id, dep.version),
        json!({}),
    )
    .await;
    assert_eq!(revoke.status, 200);
    assert_eq!(view(&s, &u, id).await, expected);
    assert_ne!(step(&s, &u, id, &input(&u, h2, l2, end, vec![])).await.status, 200);
    assert_eq!(view(&s, &u, id).await, expected);
    assert_ne!(
        request(&s, Some(&u), "GET", &format!("{base}/artifact"), json!({}))
            .await
            .status,
        200
    );
    assert_eq!(publish(&s, &u, &dep_bytes, vec![]).await.1.status, 409);
    s.shutdown().await;
}
#[tokio::test]
async fn signed_online_dependency_bundle_paid_tick_anchor_suffix_and_private_recovery() {
    run_online_chain(&root_graph(), true).await;
}
#[tokio::test]
#[ignore = "跨Node真实AST前置：先构建@rynvek/rules，再显式--ignored；不能冒称Node22已验"]
async fn actual_ts_ast_reproducible_build_then_online_signed_chain() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let mut outputs = vec![];
    for n in 0..2 {
        let out = dir.path().join(format!("graph-{n}.json"));
        let result = std::process::Command::new("node")
            .arg(root.join("packages/rules/bin/rynvek-rule-ast.mjs"))
            .arg(root.join("examples/rules/general-steer.ast.ts"))
            .arg(&out)
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        outputs.push(std::fs::read_to_string(out).unwrap());
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(
        pack("rules.private.steer", &outputs[0]),
        pack("rules.private.steer", &outputs[1])
    );
    run_online_chain(&outputs[0], false).await;
}

#[tokio::test]
async fn signatures_exact_dependencies_private_scopes_and_immutable_tombstones_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "rule-guards", "human", 103).await;
    let other = register(&s.base_url(), "rule-guards-other", "human", 104).await;
    let dep_bytes = pack("rules.guard.dep", &support_graph());
    let dep = published(&s, &u, &dep_bytes, vec![]).await;
    let bytes = pack("rules.guard.root", &root_graph());
    let m = manifest(&u, &bytes, vec![dep.reference()]);
    let signature = signed(&u, &m, &bytes).signature;
    let path = format!("/api/v1/private-hub/rules/{}/{}", m.package_id, m.version);
    let mut changed = m.clone();
    changed.dependencies.clear();
    let body = json!({"manifest":changed,"artifact_b64":base64::engine::general_purpose::STANDARD.encode(&bytes),"signature":signature});
    assert_ne!(
        request(&s, Some(&u), "PUT", &path, body).await.status,
        201,
        "锁不是签名外的自报附件"
    );
    let body = json!({"manifest":m,"artifact_b64":base64::engine::general_purpose::STANDARD.encode(&bytes),"signature":signature});
    assert_ne!(
        request(&s, Some(&other), "PUT", &path, body.clone()).await.status,
        201,
        "不能跨owner重放签名"
    );
    let limited_token = s.state.inner.tokens.write().unwrap().issue(
        u.principal_id,
        rynvek_protocol::CapabilityScopeSet::default().with(CapabilityScope::WorldRead),
        rynvek_server::ids::unix_now_s(),
        60,
    );
    let limited = User {
        principal_id: u.principal_id,
        token: rynvek_server::app::encode_token_text(&limited_token),
        key: server_key(),
    };
    assert_eq!(
        request(&s, Some(&limited), "PUT", &path, body.clone()).await.status,
        403
    );
    let mut absent = dep.reference();
    absent.id = "rules.guard.missing".into();
    assert_eq!(publish(&s, &u, &bytes, vec![absent]).await.1.status, 404);
    let mut wrong = dep.reference();
    wrong.blake3_256 = "00".repeat(32);
    assert_eq!(publish(&s, &u, &bytes, vec![wrong]).await.1.status, 409);
    let mut floating = dep.reference();
    floating.version = "^1.0.0".into();
    let invalid = manifest(&u, &bytes, vec![floating]);
    assert!(rp::signing_message(&invalid, &bytes).is_err());
    let mut repeated = manifest(&u, &bytes, vec![dep.reference(), dep.reference()]);
    assert!(rp::signing_message(&repeated, &bytes).is_err());
    repeated.dependencies = vec![];
    repeated.owner_id = format!("0{}", u.principal_id);
    assert!(rp::signing_message(&repeated, &bytes).is_err());
    assert_eq!(request(&s, Some(&u), "PUT", &path, body.clone()).await.status, 201);
    assert_eq!(request(&s, Some(&u), "PUT", &path, body).await.status, 409);
    request(
        &s,
        Some(&u),
        "DELETE",
        &format!("/api/v1/private-hub/rules/{}/{}", dep.package_id, dep.version),
        json!({}),
    )
    .await;
    let next = pack("rules.guard.next", &root_graph());
    assert_ne!(publish(&s, &u, &next, vec![dep.reference()]).await.1.status, 201);
    assert_ne!(
        request(&s, Some(&u), "GET", &format!("{path}/resolved"), json!({}))
            .await
            .status,
        200
    );
    s.state.store().tombstone_principal(u.principal_id).await.unwrap();
    assert_eq!(
        request(&s, Some(&u), "GET", "/api/v1/private-hub/rules", json!({}))
            .await
            .status,
        403
    );
    s.shutdown().await;
}
#[tokio::test]
async fn install_uses_original_control_fences_epoch_bound_modules_and_complete_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "rule-install-fences", "human", 105).await;
    let dep = published(&s, &u, &pack("rules.install.dep", &support_graph()), vec![]).await;
    let root = published(
        &s,
        &u,
        &pack("rules.install.root", &root_graph()),
        vec![dep.reference()],
    )
    .await;
    let id = create(&s, &u, &bundle_world()).await;
    let h = connect(&s, &u, id).await;
    let low = acquire(&s, &u, id, h, "Produce").await;
    let full = [("rules.install.root", "steer"), ("rules.install.dep", "support")];
    let mut i = install(&u, h, low, 0, &root, &full);
    assert_eq!(install_action(&s, &u, id, &i).await.status, 403);
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    i.control_fence = l;
    assert_ne!(install_action(&s, &u, id, &i).await.status, 200, "非Epoch不提前应用");
    assert_eq!(view(&s, &u, id).await["commit"]["tick"], 0);
    first_three(&s, &u, id, h, l).await;
    let before = view(&s, &u, id).await;
    i = install(&u, h, l, 3, &root, &full);
    let mut stale = i.clone();
    stale.control_fence = low;
    assert_ne!(install_action(&s, &u, id, &stale).await.status, 200);
    stale = i.clone();
    stale.host_fence = h + 1;
    assert_ne!(install_action(&s, &u, id, &stale).await.status, 200);
    assert_ne!(step(&s, &u, id, &i).await.status, 200, "普通tick不能偷带安装");
    let mut wrong = i.clone();
    wrong.install.as_mut().unwrap().modules.remove("rules.install.dep");
    assert_ne!(install_action(&s, &u, id, &wrong).await.status, 200);
    let mut wrong = i.clone();
    wrong
        .install
        .as_mut()
        .unwrap()
        .modules
        .insert("rules.install.dep".into(), "steer".into());
    assert_ne!(install_action(&s, &u, id, &wrong).await.status, 200);
    let mut wrong = i.clone();
    wrong
        .install
        .as_mut()
        .unwrap()
        .modules
        .insert("rules.install.dep".into(), "missing".into());
    assert_ne!(install_action(&s, &u, id, &wrong).await.status, 200);
    assert_eq!(view(&s, &u, id).await, before);
    assert_eq!(install_action(&s, &u, id, &i).await.status, 200);
    for t in 4..7 {
        assert_eq!(step(&s, &u, id, &input(&u, h, l, t, vec![])).await.status, 200);
    }
    let replacement = published(&s, &u, &pack("rules.install.replacement", &root_graph()), vec![]).await;
    let before = view(&s, &u, id).await;
    let partial = install(&u, h, l, 7, &replacement, &[("rules.install.replacement", "steer")]);
    assert_ne!(
        install_action(&s, &u, id, &partial).await.status,
        200,
        "不可拆散原签名精确依赖包组"
    );
    assert_eq!(view(&s, &u, id).await, before);
    s.shutdown().await;
}
fn heavy_graph() -> String {
    let mut nodes = vec![
        json!({"op":"LoadComponent","component":"param.thrust"}),
        json!({"op":"ConstQ8_8","imm":"256"}),
    ];
    for n in 2..92 {
        nodes.push(json!({"op":"Mul","args":[if n==2{0}else{n-1},1]}));
    }
    nodes.push(json!({"op":"EmitComponentWrite","component":"state.accel_x","args":[91]}));
    json!({"format":"rynvek.rule-graph.v1","name":"有界但超过可用生物能量","reads":[{"component":"param.thrust","ty":"Q8_8"}],"writes":[{"component":"state.accel_x","ty":"Q8_8"}],"nodes":nodes}).to_string()
}
#[tokio::test]
async fn install_tick_insufficient_sourced_energy_rolls_back_config_components_root_and_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "rule-install-atomic", "human", 106).await;
    let root = published(&s, &u, &pack("rules.atomic.expensive", &heavy_graph()), vec![]).await;
    let mut c = rules();
    c.components.as_mut().unwrap().rule_cost_micro_per_gas = Some(10000);
    let id = create(&s, &u, &c).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    first_three(&s, &u, id, h, l).await;
    let before = view(&s, &u, id).await;
    let i = install(&u, h, l, 3, &root, &[("rules.atomic.expensive", "steer")]);
    let r = install_action(&s, &u, id, &i).await;
    assert_ne!(r.status, 200);
    assert!(
        r.json().unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("ConservationLawMismatch"),
        "{:?}",
        r.json()
    );
    assert_eq!(view(&s, &u, id).await, before);
    assert_eq!(s.state.store().list_commits(id).await.unwrap().len(), 4);
    let mut retry = i.clone();
    retry.install = None;
    assert_eq!(step(&s, &u, id, &retry).await.status, 200, "未提交失败不占用幂等回执");
    assert_eq!(view(&s, &u, id).await["installed_releases"], json!({}));
    s.shutdown().await;
}
#[tokio::test]
async fn revocation_after_resolution_is_rechecked_inside_original_store_commit_lock() {
    use rynvek_server::store::{CommitRow, EditorPermit};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "rule-commit-revoke", "human", 107).await;
    let root = published(&s, &u, &pack("rules.commit.root", &root_graph()), vec![]).await;
    let c = rules();
    let id = create(&s, &u, &c).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    first_three(&s, &u, id, h, l).await;
    let request = RuleInstallRequest {
        root: root.reference(),
        modules: BTreeMap::from([("rules.commit.root".into(), "steer".into())]),
    };
    let plan = rp::prepare_install(&s.state, u.principal_id, &request).await.unwrap();
    let mut base = c.instantiate(id.0).unwrap();
    for _ in 0..3 {
        base.tick().unwrap();
    }
    let mut candidate = base
        .candidate_with_rule_install(plan.evidence_cid, &plan.graphs)
        .unwrap();
    let permits = rp::installed_permits(&s.state, u.principal_id, &candidate)
        .await
        .unwrap();
    candidate.tick().unwrap();
    let before = view(&s, &u, id).await;
    assert_eq!(request_revoke(&s, &u, &root).await, 200);
    let result = store
        .append_commit_guarded(
            CommitRow {
                world_id: id,
                world_revision: 4,
                tick: 4,
                parent_revision: Some(3),
                state_root_blake3: candidate.parent_commit_hash,
                journal_segment_cid: [0; 32],
                snapshot_cid: None,
            },
            h,
            vec![EditorPermit {
                purpose: rynvek_server::store::AdmissionPurpose::GeneralRuleDrive,
                principal_id: u.principal_id,
                grant_id: None,
            }],
            permits,
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.get_world(id).await.unwrap().unwrap().latest_revision, 3);
    assert_eq!(view(&s, &u, id).await, before);
    s.shutdown().await;
}
async fn request_revoke(s: &RunningServer, u: &User, m: &RuleReleaseManifest) -> u16 {
    request(
        s,
        Some(u),
        "DELETE",
        &format!("/api/v1/private-hub/rules/{}/{}", m.package_id, m.version),
        json!({}),
    )
    .await
    .status
}

#[tokio::test]
async fn exact_dag_rejects_conflicting_diamond_cycles_and_depth_without_new_parser() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "rule-dag", "human", 108).await;
    let v1 = published(&s, &u, &pack("rules.dag.leaf", &root_graph()), vec![]).await;
    let v2bytes = build_tier0_package("rules.dag.leaf", "2.0.0", "MIT", &root_graph()).unwrap();
    let v2 = published(&s, &u, &v2bytes, vec![]).await;
    let a = published(&s, &u, &pack("rules.dag.a", &root_graph()), vec![v1.reference()]).await;
    let b = published(&s, &u, &pack("rules.dag.b", &root_graph()), vec![v2.reference()]).await;
    assert_eq!(
        publish(
            &s,
            &u,
            &pack("rules.dag.conflict", &root_graph()),
            vec![a.reference(), b.reference()]
        )
        .await
        .1
        .status,
        400
    );
    let c = published(&s, &u, &pack("rules.dag.c", &root_graph()), vec![v1.reference()]).await;
    let diamond = published(
        &s,
        &u,
        &pack("rules.dag.diamond", &root_graph()),
        vec![a.reference(), c.reference()],
    )
    .await;
    let d = request(
        &s,
        Some(&u),
        "GET",
        &format!(
            "/api/v1/private-hub/rules/{}/{}/resolved",
            diamond.package_id, diamond.version
        ),
        json!({}),
    )
    .await;
    assert_eq!(d.status, 200);
    assert_eq!(d.json().unwrap()["pins"].as_array().unwrap().len(), 4);
    let mut previous = v1;
    for level in 1..=8 {
        previous = published(
            &s,
            &u,
            &pack(&format!("rules.depth.level{level}"), &root_graph()),
            vec![previous.reference()],
        )
        .await;
    }
    assert_eq!(
        publish(
            &s,
            &u,
            &pack("rules.depth.too-deep", &root_graph()),
            vec![previous.reference()]
        )
        .await
        .1
        .status,
        400
    );
    // 故意模拟损坏私库元数据：正常HTTP不允许前向依赖，仍防守性复验签名DAG的环。
    let ca = pack("rules.cycle.a", &root_graph());
    let cb = pack("rules.cycle.b", &root_graph());
    let ma0 = manifest(&u, &ca, vec![]);
    let mb = manifest(&u, &cb, vec![ma0.reference()]);
    let ma = manifest(&u, &ca, vec![mb.reference()]);
    for (m, bytes) in [(ma, ca), (mb, cb)] {
        let release = signed(&u, &m, &bytes);
        let cid = s.state.inner.world_snapshots.put(&bytes).unwrap();
        s.state
            .store()
            .put_private_rule(rynvek_server::store::PrivateArtifactRow {
                owner_id: u.principal_id,
                package_id: m.package_id.clone(),
                version: m.version.clone(),
                cid: cid.0,
                signature: rynvek_server::ids::parse_hex::<64>(&release.signature)
                    .unwrap()
                    .to_vec(),
                publisher_key: u.key.public_key(),
                profile: serde_json::to_value(&m).unwrap(),
                dependencies: vec![],
                verification: json!({}),
                revoked: false,
            })
            .await
            .unwrap();
    }
    assert_eq!(
        request(
            &s,
            Some(&u),
            "GET",
            "/api/v1/private-hub/rules/rules.cycle.a/1.0.0/resolved",
            json!({})
        )
        .await
        .status,
        400
    );
    s.shutdown().await;
}

#[tokio::test]
async fn explicit_paid_quarantine_reaches_epoch_without_revoked_vm_free_ticks_or_automatic_resume() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "rule-quarantine", "human", 109).await;
    let old = published(&s, &u, &pack("rules.quarantine.old", &root_graph()), vec![]).await;
    let new = published(&s, &u, &pack("rules.quarantine.new", &root_graph()), vec![]).await;
    let id = create(&s, &u, &rules()).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    first_three(&s, &u, id, h, l).await;
    let old_input = install(&u, h, l, 3, &old, &[("rules.quarantine.old", "steer")]);
    assert_eq!(install_action(&s, &u, id, &old_input).await.status, 200);
    assert_eq!(step(&s, &u, id, &input(&u, h, l, 4, vec![])).await.status, 200);
    let client = rynvek_server::private_rule_client::PrivateRuleClient::new(
        &s.base_url(),
        u.principal_id,
        u.key.public_key(),
        u.token.clone(),
    )
    .unwrap();
    let req = RuleInstallRequest {
        root: new.reference(),
        modules: BTreeMap::from([("rules.quarantine.new".into(), "steer".into())]),
    };
    let before = view(&s, &u, id).await;
    let key = IdempotencyKey::new(new_uuid_v7()).unwrap();
    assert!(
        client
            .install(&id.to_string(), h, l, 5, key, req.clone(), true)
            .await
            .is_err(),
        "未撤销不可廉价跳过规则"
    );
    assert_eq!(request_revoke(&s, &u, &old).await, 200);
    assert_ne!(step(&s, &u, id, &input(&u, h, l, 5, vec![])).await.status, 200);
    let q = client
        .install(&id.to_string(), h, l, 5, key, req.clone(), true)
        .await
        .unwrap();
    assert_eq!(q["commit"]["tick"], 6);
    let paused = view(&s, &u, id).await;
    assert_eq!(paused["rule_meter"]["tier0_gas"], 0);
    assert_eq!(paused["rule_meter"]["paid_q"], 0);
    assert_eq!(paused["rule_recovery"]["maintenance_paid_q"], 4);
    assert_eq!(paused["rule_recovery"]["epoch_tick"], 8);
    assert_eq!(paused["bodies"][0]["body"]["acceleration_q32"], json!([0, 0]));
    assert_eq!(paused["components"], before["components"]);
    assert_eq!(paused["life"]["genesis_energy_q"], paused["life"]["universe_energy_q"]);
    assert_ne!(step(&s, &u, id, &input(&u, h, l, 6, vec![])).await.status, 200);
    assert_eq!(view(&s, &u, id).await, paused);
    s.shutdown().await;
    let s = start(store.clone(), dir.path()).await;
    assert_eq!(view(&s, &u, id).await, paused);
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    let client = rynvek_server::private_rule_client::PrivateRuleClient::new(
        &s.base_url(),
        u.principal_id,
        u.key.public_key(),
        u.token.clone(),
    )
    .unwrap();
    assert_eq!(
        client
            .install(&id.to_string(), h, l, 5, key, req.clone(), true)
            .await
            .unwrap()["duplicate"],
        true
    );
    client
        .install(
            &id.to_string(),
            h,
            l,
            6,
            IdempotencyKey::new(new_uuid_v7()).unwrap(),
            req.clone(),
            true,
        )
        .await
        .unwrap();
    let paused7 = view(&s, &u, id).await;
    assert_eq!(paused7["commit"]["tick"], 7);
    assert!(
        client
            .install(
                &id.to_string(),
                h,
                l,
                7,
                IdempotencyKey::new(new_uuid_v7()).unwrap(),
                req.clone(),
                true
            )
            .await
            .is_err(),
        "不得以维护越过Epoch"
    );
    client
        .install(
            &id.to_string(),
            h,
            l,
            7,
            IdempotencyKey::new(new_uuid_v7()).unwrap(),
            req,
            false,
        )
        .await
        .unwrap();
    let active = view(&s, &u, id).await;
    assert_eq!(active["commit"]["tick"], 8);
    assert_eq!(active["rule_recovery"], Value::Null);
    assert!(active["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_eq!(active["life"]["genesis_energy_q"], active["life"]["universe_energy_q"]);
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(view(&s, &u, id).await, active);
    s.shutdown().await;
}

async fn actual_rule_sdk(payload: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("node")
            .arg("packages/agent-sdk/test/private-rule-e2e.mjs")
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    })
    .await
    .unwrap()
}
#[tokio::test]
#[ignore = "跨TS SDK真实HTTP：先pnpm --filter @rynvek/agent-sdk build，再显式运行；Node22另须实证"]
async fn actual_ts_sdk_signed_publish_trusted_download_lossless_u64_paid_quarantine_epoch_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "rule-sdk-user", "human", 110).await;
    assert!(u.principal_id > 9_007_199_254_740_991);
    let id = create(&s, &u, &bundle_world()).await;
    let dep = pack("rules.sdk.old-dep", &support_graph());
    let md = manifest(&u, &dep, vec![]);
    let root = pack("rules.sdk.old-root", &root_graph());
    let mr = manifest(&u, &root, vec![md.reference()]);
    let next_dep = pack("rules.sdk.new-dep", &support_graph());
    let nd = manifest(&u, &next_dep, vec![]);
    let next_root = pack("rules.sdk.new-root", &root_graph());
    let nr = manifest(&u, &next_root, vec![nd.reference()]);
    let old = RuleInstallRequest {
        root: mr.reference(),
        modules: BTreeMap::from([
            ("rules.sdk.old-root".into(), "steer".into()),
            ("rules.sdk.old-dep".into(), "support".into()),
        ]),
    };
    let next = RuleInstallRequest {
        root: nr.reference(),
        modules: BTreeMap::from([
            ("rules.sdk.new-root".into(), "steer".into()),
            ("rules.sdk.new-dep".into(), "support".into()),
        ]),
    };
    let packages=[(md.clone(),dep),(mr,root),(nd,next_dep),(nr,next_root)].into_iter().map(|(m,b)|json!({"signingMessage":rynvek_server::ids::hex(&rp::signing_message(&m,&b).unwrap()),"manifest":m,"bytes":base64::engine::general_purpose::STANDARD.encode(b)})).collect::<Vec<_>>();
    let r=actual_rule_sdk(json!({"mode":"start","base":s.base_url(),"owner":u.principal_id.to_string(),"trustedKey":rynvek_server::ids::hex(&u.key.public_key()),"testSeed":rynvek_server::ids::hex(&u.key.seed()),"token":u.token,"world":id.to_string(),"old":old,"next":next,"oldDependency":md.reference(),"packages":packages})).await;
    assert_eq!(r["observation"], view(&s, &u, id).await);
    s.shutdown().await;
    let s = start(store.clone(), dir.path()).await;
    let r=actual_rule_sdk(json!({"mode":"resume","base":s.base_url(),"owner":u.principal_id.to_string(),"trustedKey":rynvek_server::ids::hex(&u.key.public_key()),"token":u.token,"world":id.to_string(),"next":next})).await;
    let expected = view(&s, &u, id).await;
    assert_eq!(r["observation"], expected);
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(view(&s, &u, id).await, expected);
    s.shutdown().await;
}

#[tokio::test]
async fn pending_quarantine_release_revoked_after_resolve_still_fails_original_commit_guard() {
    use rynvek_server::store::{CommitRow, EditorPermit};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "quarantine-guard", "human", 113).await;
    let old = published(&s, &u, &pack("rules.pause.old", &root_graph()), vec![]).await;
    let pending = published(&s, &u, &pack("rules.pause.pending", &root_graph()), vec![]).await;
    let final_root = published(&s, &u, &pack("rules.pause.final", &root_graph()), vec![]).await;
    let cfg = rules();
    let id = create(&s, &u, &cfg).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    first_three(&s, &u, id, h, l).await;
    assert_eq!(
        install_action(&s, &u, id, &install(&u, h, l, 3, &old, &[("rules.pause.old", "steer")]))
            .await
            .status,
        200
    );
    assert_eq!(step(&s, &u, id, &input(&u, h, l, 4, vec![])).await.status, 200);
    request_revoke(&s, &u, &old).await;
    let client = rynvek_server::private_rule_client::PrivateRuleClient::new(
        &s.base_url(),
        u.principal_id,
        u.key.public_key(),
        u.token.clone(),
    )
    .unwrap();
    let request = RuleInstallRequest {
        root: pending.reference(),
        modules: BTreeMap::from([("rules.pause.pending".into(), "steer".into())]),
    };
    client
        .install(
            &id.to_string(),
            h,
            l,
            5,
            IdempotencyKey::new(new_uuid_v7()).unwrap(),
            request.clone(),
            true,
        )
        .await
        .unwrap();
    let mut engine = cfg.instantiate(id.0).unwrap();
    for c in store.list_commits(id).await.unwrap().into_iter().skip(1) {
        let b = s
            .state
            .inner
            .world_snapshots
            .get(&Blake3Hash(c.journal_segment_cid))
            .unwrap();
        engine = serde_json::from_slice::<GeneralTickRecord>(&b)
            .unwrap()
            .replay(&engine)
            .unwrap();
    }
    let plan = rp::prepare_install(&s.state, u.principal_id, &request).await.unwrap();
    let mut candidate = engine
        .candidate_with_rule_recovery(plan.evidence_cid, &plan.graphs)
        .unwrap();
    let permits = rp::installed_permits(&s.state, u.principal_id, &candidate)
        .await
        .unwrap();
    assert_eq!(permits[0].pins[0].id, "rules.pause.pending");
    candidate.tick().unwrap();
    let before = view(&s, &u, id).await;
    request_revoke(&s, &u, &pending).await;
    assert!(
        store
            .append_commit_guarded(
                CommitRow {
                    world_id: id,
                    world_revision: 7,
                    tick: 7,
                    parent_revision: Some(6),
                    state_root_blake3: candidate.parent_commit_hash,
                    journal_segment_cid: [0; 32],
                    snapshot_cid: None
                },
                h,
                vec![EditorPermit {
                    purpose: rynvek_server::store::AdmissionPurpose::GeneralRuleDrive,
                    principal_id: u.principal_id,
                    grant_id: None
                }],
                permits
            )
            .await
            .is_err()
    );
    assert_eq!(view(&s, &u, id).await, before);
    let next = RuleInstallRequest {
        root: final_root.reference(),
        modules: BTreeMap::from([("rules.pause.final".into(), "steer".into())]),
    };
    client
        .install(
            &id.to_string(),
            h,
            l,
            6,
            IdempotencyKey::new(new_uuid_v7()).unwrap(),
            next.clone(),
            true,
        )
        .await
        .unwrap();
    client
        .install(
            &id.to_string(),
            h,
            l,
            7,
            IdempotencyKey::new(new_uuid_v7()).unwrap(),
            next,
            false,
        )
        .await
        .unwrap();
    assert_eq!(view(&s, &u, id).await["rule_recovery"], Value::Null);
    s.shutdown().await;
}

#[tokio::test]
#[ignore = "真实General Web控制器HTTP链：先tsc构建web-platform；无浏览器/GPU验收"]
async fn actual_general_web_identity_create_discover_fences_install_quarantine_and_private_restore() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "general-web-owner", "human", 113).await;
    published(&s, &u, &pack("rules.web.old", &root_graph()), vec![]).await;
    published(&s, &u, &pack("rules.web.new", &root_graph()), vec![]).await;
    let me = client::get(&format!("{}/api/v1/auth/session", s.base_url()), Some(&u.token))
        .await
        .unwrap();
    assert_eq!(me.status, 200);
    assert_eq!(me.json().unwrap()["principal_id"], u.principal_id.to_string());
    assert_eq!(
        client::get(&format!("{}/api/v1/auth/session", s.base_url()), None)
            .await
            .unwrap()
            .status,
        401
    );
    let payload = json!({"base":s.base_url(),"token":u.token,"owner":u.principal_id.to_string(),"key":rynvek_server::ids::hex(&u.key.public_key()),"world":rules().build_package("world.test.general-web", "0.1.0").unwrap()});
    let result = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut cmd = std::process::Command::new("node")
            .arg("apps/web-platform/test/general-http-e2e.mjs")
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        cmd.stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        let out = cmd.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "真实Web控制器失败: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    })
    .await
    .unwrap();
    let id = result["world_id"].as_str().unwrap().parse::<WireUuid>().unwrap();
    let before = view(&s, &u, id).await;
    assert_eq!(before["commit"]["state_root"], result["root"]);
    assert_eq!(before["commit"]["tick"], 9);
    s.shutdown().await;
    let restored = start(store, dir.path()).await;
    assert_eq!(view(&restored, &u, id).await, before);
    restored.shutdown().await;
}

#[tokio::test]
async fn purpose_driver_runs_owner_installed_signed_bundle_without_private_package_access() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let owner = register(&s.base_url(), "private-driver-owner", "human", 154).await;
    let driver = register(&s.base_url(), "private-driver-guest", "human", 155).await;
    let id = create(&s, &owner, &bundle_world()).await;
    let dep = published(&s, &owner, &pack("rules.drive.dep", &support_graph()), vec![]).await;
    let root = published(
        &s,
        &owner,
        &pack("rules.drive.root", &root_graph()),
        vec![dep.reference()],
    )
    .await;
    let h = connect(&s, &owner, id).await;
    let l = acquire(&s, &owner, id, h, "Sovereign").await;
    first_three(&s, &owner, id, h, l).await;
    let i = install(
        &owner,
        h,
        l,
        3,
        &root,
        &[("rules.drive.root", "steer"), ("rules.drive.dep", "support")],
    );
    assert_eq!(install_action(&s, &owner, id, &i).await.status, 200);
    ok_action(
        &s,
        &owner,
        id,
        json!({"action":"release","host_fence":h,"control_fence":l}),
    )
    .await;
    let r=request(&s,Some(&owner),"POST",&format!("/api/v1/worlds/{id}/general/drivers"),json!({"principal_id":driver.principal_id.to_string(),"expected_grant_id":null,"active":true,"consent":"general-installed-rule-drive/1"})).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    let grant = r.json().unwrap()["grant_id"].clone();
    let path = format!("/api/v1/worlds/{id}/general/drive");
    let drive = |command: Value| json!({"grant_id":grant,"command":command});
    let r = request(
        &s,
        Some(&driver),
        "POST",
        &path,
        drive(json!({"action":"acquire","host_fence":h,"level":"Sovereign"})),
    )
    .await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    let l = r.json().unwrap()["lease"]["fencing_token"].as_u64().unwrap();
    let before = view(&s, &owner, id).await;
    let r = request(
        &s,
        Some(&driver),
        "POST",
        &path,
        drive(json!({"action":"tick","input":input(&driver,h,l,4,vec![])})),
    )
    .await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    assert!(r.json().unwrap().get("observation").is_none());
    let expected = view(&s, &owner, id).await;
    assert_eq!(expected["commit"]["tick"], 5);
    assert!(expected["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_ne!(before["bodies"], expected["bodies"]);
    assert_eq!(
        before["life"]["universe_energy_q"],
        expected["life"]["universe_energy_q"]
    );
    for suffix in ["", "/artifact", "/resolved"] {
        assert_eq!(
            request(
                &s,
                Some(&driver),
                "GET",
                &format!("/api/v1/private-hub/rules/{}/{}{suffix}", root.package_id, root.version),
                json!({})
            )
            .await
            .status,
            404
        );
    }
    assert_eq!(request_revoke(&s, &owner, &dep).await, 200);
    assert_ne!(
        request(
            &s,
            Some(&driver),
            "POST",
            &path,
            drive(json!({"action":"tick","input":input(&driver,h,l,5,vec![])}))
        )
        .await
        .status,
        200
    );
    assert_eq!(view(&s, &owner, id).await, expected);
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(
        view(&s, &owner, id).await,
        expected,
        "撤包不抹历史，也不把历史授权当当前准入"
    );
    s.shutdown().await;
}

#[tokio::test]
async fn durable_original_actor_stops_on_private_dependency_revocation_without_free_progress() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let owner = register(&s.base_url(), "durable-private-owner", "human", 156).await;
    let id = create(&s, &owner, &bundle_world()).await;
    let dep = published(
        &s,
        &owner,
        &pack("rules.durable.dep", &support_graph()),
        vec![],
    )
    .await;
    let root = published(
        &s,
        &owner,
        &pack("rules.durable.root", &root_graph()),
        vec![dep.reference()],
    )
    .await;
    let h = connect(&s, &owner, id).await;
    let l = acquire(&s, &owner, id, h, "Sovereign").await;
    first_three(&s, &owner, id, h, l).await;
    let i = install(
        &owner,
        h,
        l,
        3,
        &root,
        &[
            ("rules.durable.root", "steer"),
            ("rules.durable.dep", "support"),
        ],
    );
    assert_eq!(install_action(&s, &owner, id, &i).await.status, 200);
    let row = store.get_world(id).await.unwrap().unwrap();
    let v=ok_action(&s,&owner,id,json!({"action":"start_run","host_fence":h,"control_fence":l,"spec":{
      "submission_id":new_uuid_v7().to_string(),"expected_revision":row.latest_revision,"expected_root":rynvek_server::ids::hex(&row.latest_state_root),
      "ticks":20,"wall_seconds":60,"interval_ms":500,"max_energy_q":"1000000000","consent":rynvek_server::general_runs::CONSENT}})).await;
    let jid = v["id"].as_str().unwrap().to_owned();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let j = store
                .general_run_get(owner.principal_id, jid.clone())
                .await
                .unwrap()
                .unwrap();
            assert_ne!(j.status, "blocked", "{j:?}");
            if j.done >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(request_revoke(&s, &owner, &dep).await, 200);
    let frozen = store.get_world(id).await.unwrap().unwrap();
    let j = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let j = store
                .general_run_get(owner.principal_id, jid.clone())
                .await
                .unwrap()
                .unwrap();
            if j.status == "blocked" {
                break j;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(j.done >= 1 && j.done < 20);
    assert!(j.energy_used_q > 0);
    assert_eq!(
        store
            .get_world(id)
            .await
            .unwrap()
            .unwrap()
            .latest_state_root,
        frozen.latest_state_root,
        "撤包返回后不再发布候选Tick"
    );
    assert_eq!(j.base_tick + j.done as u64, frozen.latest_tick);
    let r=request(&s,Some(&owner),"POST",&format!("/api/v1/worlds/{id}/general/runs/{}",j.id),json!({"operation_id":new_uuid_v7().to_string(),"generation":j.generation,"action":"cancel"})).await;
    assert_eq!(r.status, 200, "撤销后仍能取消意图，不必执行已撤销VM");
    s.shutdown().await;
}
