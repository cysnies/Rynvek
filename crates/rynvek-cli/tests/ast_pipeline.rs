// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 显式运行的跨工具链实证；没有构建TS SDK时不把缺少dist假装成Rust回归。
use rynvek_protocol::{CommandEnvelope, IdempotencyKey};
use rynvek_security::ServerSigningKey;
use rynvek_server::{
    AppConfig, RunningServer, ServerConfig,
    client::{self, LoginTarget},
    store::Store,
};
use serde_json::{Value, json};
use std::{path::Path, process::Command};
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rynvek")).args(args).output().unwrap()
}
fn success(o: std::process::Output) {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
async fn server(store: Store, path: &Path) -> RunningServer {
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
        signer: ServerSigningKey::from_seed(1, [7; 32]),
    })
    .await
    .unwrap()
}
async fn act(s: &RunningServer, token: &str, id: &str, body: Value) -> Value {
    let r = client::send_json(
        "POST",
        &format!("{}/api/v1/worlds/{id}/general", s.base_url()),
        Some(token),
        &body,
    )
    .await
    .unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
#[tokio::test]
#[ignore = "跨Node/Rust真实链：先pnpm --filter @rynvek/rules build，再显式--ignored运行；Node22另须验收"]
async fn actual_ts_ast_cli_rpack_install_http_paid_tick_and_private_restore() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = tempfile::tempdir().unwrap();
    let source = root.join("examples/rules/general-steer.ast.ts");
    let frontend = root.join("packages/rules/bin/rynvek-rule-ast.mjs");
    let graph = tmp.path().join("graph.json");
    let graph2 = tmp.path().join("again.json");
    for out in [&graph, &graph2] {
        success(
            Command::new("node")
                .arg(&frontend)
                .arg(&source)
                .arg(out)
                .output()
                .unwrap(),
        );
    }
    assert_eq!(std::fs::read(&graph).unwrap(), std::fs::read(&graph2).unwrap());
    assert!(
        !Command::new("node")
            .arg(&frontend)
            .arg(&source)
            .arg(&graph)
            .status()
            .unwrap()
            .success()
    );
    let pack = tmp.path().join("steer.rpack");
    let pack2 = tmp.path().join("again.rpack");
    for out in [&pack, &pack2] {
        success(cli(&[
            "rule",
            "build",
            "--graph",
            graph.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--id",
            "rules.ast.steer",
        ]));
    }
    assert_eq!(std::fs::read(&pack).unwrap(), std::fs::read(&pack2).unwrap());
    let inspected = cli(&["rule", "inspect", "--package", pack.to_str().unwrap()]);
    assert!(inspected.status.success());
    let receipt: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(receipt["signed"], false);
    let config = root.join("examples/worlds/general-rules.world.json");
    let world = tmp.path().join("ast.rworld");
    let spec = format!("steer={}", pack.display());
    success(cli(&[
        "world",
        "build",
        "--config",
        config.to_str().unwrap(),
        "--out",
        world.to_str().unwrap(),
        "--id",
        "world.ast.steer",
        "--rule-pack",
        &spec,
    ]));
    let badout = tmp.path().join("bad.rworld");
    let badspec = format!("unknown={}", pack.display());
    assert!(
        !cli(&[
            "world",
            "build",
            "--config",
            config.to_str().unwrap(),
            "--out",
            badout.to_str().unwrap(),
            "--id",
            "world.ast.bad",
            "--rule-pack",
            &badspec
        ])
        .status
        .success()
    );
    assert!(!badout.exists());
    let bytes = std::fs::read(&world).unwrap();
    let installed = rynvek_world::general::GeneralWorldConfig::from_package_bytes(&bytes).unwrap();
    assert_eq!(
        installed.components.as_ref().unwrap().rule_graphs["steer"],
        rynvek_rules::package::Tier0RulePackage::parse(&std::fs::read(&pack).unwrap())
            .unwrap()
            .graph_json
    );
    let store = Store::memory();
    let s = server(store.clone(), tmp.path()).await;
    let key = ServerSigningKey::from_seed(0, [92; 32]);
    let (pid, token) = client::login(
        &s.base_url(),
        &key,
        LoginTarget::Register {
            handle: "ast-owner",
            kind: "human",
        },
    )
    .await
    .unwrap();
    // 原Hub扫描实际调用同一构建剖面复验；这里不是签名发布或私有发现链的替身。
    let artifact = std::fs::read(&pack).unwrap();
    let lock = json!({"lockfile_version":1,"packages":[]});
    let report = rynvek_server::verify::verify(
        &rynvek_server::verify::VerifyInput {
            kind: "rpack",
            package_id: "rules.ast.steer",
            semver: "1.0.0",
            license_spdx: "CC0-1.0",
            lockfile: &lock,
            artifact: &artifact,
            official: false,
        },
        &s.state.plugin_runtime().unwrap(),
    )
    .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f["graph_rir_recompiled_equal"] == true && f["ts_source_equivalence"] == false)
    );
    let created = client::request(
        "POST",
        &format!("{}/api/v1/worlds/general", s.base_url()),
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("Content-Type", "application/vnd.rynvek.rworld"),
        ],
        Some(&bytes),
    )
    .await
    .unwrap();
    assert_eq!(created.status, 201, "{:?}", created.json());
    let id = created.json().unwrap()["world_id"].as_str().unwrap().to_owned();
    let h = act(&s, &token, &id, json!({"action":"connect"})).await["host_fence"]
        .as_u64()
        .unwrap();
    let l = act(
        &s,
        &token,
        &id,
        json!({"action":"acquire","host_fence":h,"level":"Sovereign"}),
    )
    .await["lease"]["fencing_token"]
        .as_u64()
        .unwrap();
    let envelope = CommandEnvelope::<Vec<rynvek_simulation::physics::PhysicsCommand>> {
        idempotency_key: IdempotencyKey::new(rynvek_server::ids::new_uuid_v7()).unwrap(),
        principal_id: pid,
        issued_at_tick: 0,
        target_tick: 1,
        payload: vec![],
    };
    let response = act(
        &s,
        &token,
        &id,
        json!({"action":"tick","input":{"host_fence":h,"control_fence":l,"envelope":envelope}}),
    )
    .await;
    let actual = &response["observation"];
    assert!(actual["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_eq!(actual["bodies"][0]["body"]["acceleration_q32"][0], 1i64 << 31);
    assert_eq!(actual["life"]["universe_energy_q"], actual["life"]["genesis_energy_q"]);
    let mut reference = installed
        .instantiate(id.parse::<rynvek_protocol::WireUuid>().unwrap().0)
        .unwrap();
    reference.tick().unwrap();
    assert_eq!(
        actual["commit"]["state_root"],
        rynvek_server::ids::hex(&reference.parent_commit_hash)
    );
    s.shutdown().await;
    let s = server(store, tmp.path()).await;
    let restored = client::get(&format!("{}/api/v1/worlds/{id}/general", s.base_url()), Some(&token))
        .await
        .unwrap();
    assert_eq!(restored.status, 200);
    assert_eq!(restored.json().unwrap(), *actual);
    s.shutdown().await;
}
#[test]
fn bad_graph_cannot_build_or_overwrite_existing_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let graph = tmp.path().join("bad.json");
    let out = tmp.path().join("saved.rpack");
    std::fs::write(&graph,r#"{"format":"rynvek.rule-graph.v1","name":"bad","reads":[],"writes":[],"nodes":[{"op":"ConstI32","imm":"1"},{"op":"EmitComponentWrite","component":"bad","args":[0]}]}"#).unwrap();
    std::fs::write(&out, b"original").unwrap();
    let r = cli(&[
        "rule",
        "build",
        "--graph",
        graph.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--id",
        "rules.bad",
    ]);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("0x5002"));
    assert_eq!(std::fs::read(&out).unwrap(), b"original");
}
