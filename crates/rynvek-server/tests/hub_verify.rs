// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! M8 分发闭环集成测试：Hub 静态扫描与 1,000 Tick 沙箱（`.rview` / `.rpack` / `.rworld`）、
//! `.rworld` 上传创建世界、文件系统 CAS 持久化。

mod common;

use base64::Engine as _;
use common::*;
use rynvek_server::client::{self, HttpResponse};
use serde_json::{Value, json};

async fn send(base: &str, method: &str, path: &str, token: Option<&str>, body: Value) -> HttpResponse {
    client::send_json(method, &format!("{base}{path}"), token, &body)
        .await
        .expect("HTTP 请求")
}

fn assert_error(r: &HttpResponse, status: u16, code: u16) {
    assert_eq!(r.status, status, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json().unwrap();
    assert_eq!(v["code"].as_u64(), Some(u64::from(code)), "{v}");
}

fn sign(user: &User, id: &str, ver: &str, artifact: &[u8]) -> String {
    let msg = rynvek_server::api::hub::package_signing_message(id, ver, artifact);
    rynvek_security::to_hex(&user.key.sign(&msg))
}

#[allow(clippy::too_many_arguments)] // 测试辅助：逐字段对应发布请求
async fn publish(
    base: &str,
    user: &User,
    kind: &str,
    id: &str,
    ver: &str,
    license: &str,
    artifact: &[u8],
    lock: Value,
) -> HttpResponse {
    send(
        base,
        "PUT",
        &format!("/api/v1/hub/packages/{id}/{ver}"),
        Some(&user.token),
        json!({
            "org_id": user.principal_id.to_string(),
            "package_kind": kind,
            "min_engine_version": "1.0.0",
            "license_spdx": license,
            "artifact_b64": base64::engine::general_purpose::STANDARD.encode(artifact),
            "signature": sign(user, id, ver, artifact),
            "manifest": { "name": id },
            "lockfile": lock,
        }),
    )
    .await
}

fn empty_lock() -> Value {
    json!({ "lockfile_version": 1, "packages": [] })
}

#[tokio::test]
async fn rview_shaders_are_statically_reviewed() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    // 存储缓冲越过只读表现层 → 0x9004
    let bad = "@group(0) @binding(0) var<storage, read_write> buf: array<u32>;\n".to_owned() + OK_SHADER;
    let r = publish(
        &base,
        &alice,
        "rview",
        "alice.bad",
        "1.0.0",
        "AGPL-3.0-only",
        &rview_pack("alice.bad", "1.0.0", &bad),
        empty_lock(),
    )
    .await;
    assert_error(&r, 422, 0x9004);
    // 审查器通过但 naga 校验失败（类型错误）→ 0x9004
    let typo = "@fragment fn m() -> @location(0) vec4<f32> { return 1; }";
    let r = publish(
        &base,
        &alice,
        "rview",
        "alice.typo",
        "1.0.0",
        "AGPL-3.0-only",
        &rview_pack("alice.typo", "1.0.0", typo),
        empty_lock(),
    )
    .await;
    assert_error(&r, 422, 0x9004);
    // 包内清单与发布路径不一致 → 0x7001；被篡改 → 0x7002
    let pack = rview_pack("alice.other", "1.0.0", OK_SHADER);
    let r = publish(
        &base,
        &alice,
        "rview",
        "alice.mismatch",
        "1.0.0",
        "AGPL-3.0-only",
        &pack,
        empty_lock(),
    )
    .await;
    assert_error(&r, 400, 0x7001);
    let mut tampered = rview_pack("alice.t", "1.0.0", OK_SHADER);
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    let r = publish(
        &base,
        &alice,
        "rview",
        "alice.t",
        "1.0.0",
        "AGPL-3.0-only",
        &tampered,
        empty_lock(),
    )
    .await;
    assert_error(&r, 400, 0x7002);
    // 非容器类型（audio）不扫描，保持 TrustLevel 3
    let r = publish(
        &base,
        &alice,
        "audio",
        "alice.sfx",
        "1.0.0",
        "CC0-1.0",
        b"RIFF....",
        empty_lock(),
    )
    .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.json().unwrap()["verification"]["static_scan"], "not_applicable");
    assert_eq!(r.json().unwrap()["trust_level"], 3);
    server.shutdown().await;
}

#[tokio::test]
async fn rpack_plugins_pass_a_1000_tick_sandbox() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let lic = "Apache-2.0 WITH LLVM-exception";
    let pack = rpack_with_plugin("alice.probe", "1.0.0", PROBE_PLUGIN);
    let r = publish(&base, &alice, "rpack", "alice.probe", "1.0.0", lic, &pack, empty_lock()).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json().unwrap();
    assert_eq!(v["trust_level"], 2);
    let sandbox = &v["verification"]["sandbox"];
    assert_eq!(sandbox["status"], "passed");
    assert_eq!(sandbox["plugins"][0]["ticks"], 1000);
    assert_eq!(sandbox["plugins"][0]["plugin_id"], "community.probe");
    assert!(sandbox["plugins"][0]["max_gas_per_call"].as_u64().unwrap() < 500_000);

    // 导入 WASI 的组件：静态扫描拒绝 → 0x9004
    let wasi = wat::parse_str(r#"(component (import "wasi:sockets/tcp@0.2.0" (instance)))"#).unwrap();
    let pack = rpack_with_plugin("alice.net", "1.0.0", &wasi);
    let r = publish(&base, &alice, "rpack", "alice.net", "1.0.0", lic, &pack, empty_lock()).await;
    assert_error(&r, 422, 0x9004);
    assert!(String::from_utf8_lossy(&r.body).contains("wasi:sockets"));
    // 核心模块冒充插件 → 0x9004
    let core = wat::parse_str("(module)").unwrap();
    let pack = rpack_with_plugin("alice.core", "1.0.0", &core);
    let r = publish(&base, &alice, "rpack", "alice.core", "1.0.0", lic, &pack, empty_lock()).await;
    assert_error(&r, 422, 0x9004);
    server.shutdown().await;
}

#[tokio::test]
async fn rworld_publish_upload_and_fs_cas_roundtrip() {
    let blob_dir = tempfile::tempdir().unwrap();
    let loopback: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = rynvek_server::start(rynvek_server::server::ServerConfig {
        http_addr: loopback,
        webtransport_addr: None,
        app: rynvek_server::app::AppConfig {
            public_base_url: String::new(),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            lobby_timeout: None,
            blob_dir: Some(blob_dir.path().to_path_buf()),
            ..rynvek_server::app::AppConfig::default()
        },
        store: rynvek_server::store::Store::memory(),
        signer: server_key(),
    })
    .await
    .unwrap();
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;

    // 先发布一个依赖，再发布锁定它的世界包
    let probe = rpack_with_plugin("alice.probe", "1.0.0", PROBE_PLUGIN);
    let r = publish(
        &base,
        &alice,
        "rpack",
        "alice.probe",
        "1.0.0",
        "Apache-2.0 WITH LLVM-exception",
        &probe,
        empty_lock(),
    )
    .await;
    assert_eq!(r.status, 201);
    let lock = json!({ "lockfile_version": 1, "packages": [{
        "id": "alice.probe", "version": "1.0.0", "kind": "rpack",
        "blake3_256": format!("blake3:{}", blake3::hash(&probe).to_hex()),
    }]});
    let world_json = r#"{"schema":"rynvek.world-config.v1","template":"arena_duel","map_seed":42}"#;
    let world = rworld_pack("alice.duel", "1.0.0", world_json, Some(&lock.to_string()));
    let r = publish(
        &base,
        &alice,
        "rworld",
        "alice.duel",
        "1.0.0",
        "CC0-1.0",
        &world,
        lock.clone(),
    )
    .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json().unwrap();
    assert_eq!(v["verification"]["sandbox"]["status"], "passed");
    assert_eq!(v["verification"]["sandbox"]["ticks"], 1000);
    // 包内 lockfile 与请求不一致 → 0x9003
    let r = publish(
        &base,
        &alice,
        "rworld",
        "alice.duel2",
        "1.0.0",
        "CC0-1.0",
        &rworld_pack("alice.duel2", "1.0.0", world_json, Some(&lock.to_string())),
        empty_lock(),
    )
    .await;
    assert_error(&r, 409, 0x9003);

    // 从 Hub 取回世界包（CAS 落盘）并上传创建世界
    let cid = v["artifact_cid_blake3"].as_str().unwrap();
    let fetched = client::request("GET", &format!("{base}/api/v1/hub/blobs/{cid}"), &[], None)
        .await
        .unwrap();
    assert_eq!(fetched.body, world);
    assert!(
        blob_dir.path().join(&cid[..2]).join(cid).is_file(),
        "Blob 应持久化到文件系统 CAS"
    );
    let auth = format!("Bearer {}", alice.token);
    let r = client::request(
        "POST",
        &format!("{base}/api/v1/worlds?title=%E5%86%B3%E6%96%97&visibility=Public"),
        &[
            ("Authorization", &auth),
            ("Content-Type", "application/vnd.rynvek.rworld"),
        ],
        Some(&fetched.body),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let w = r.json().unwrap();
    assert_eq!(w["title"], "决斗");
    assert_eq!(w["game_mode"], "PvP_Custom");
    let expected = rynvek_rts::ArenaWorld::new(rynvek_rts::MatchConfig::duel(42)).state_hash();
    assert_eq!(w["latest_state_root"], expected.to_hex());
    // 损坏的 .rworld → 0x7002；未知 Content-Type → 0x7001
    let mut bad = world.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    let r = client::request(
        "POST",
        &format!("{base}/api/v1/worlds"),
        &[
            ("Authorization", &auth),
            ("Content-Type", "application/vnd.rynvek.rworld"),
        ],
        Some(&bad),
    )
    .await
    .unwrap();
    assert_error(&r, 400, 0x7002);
    let r = client::request(
        "POST",
        &format!("{base}/api/v1/worlds"),
        &[("Authorization", &auth), ("Content-Type", "text/plain")],
        Some(b"x"),
    )
    .await
    .unwrap();
    assert_error(&r, 400, 0x7001);
    server.shutdown().await;
}

#[tokio::test]
async fn signed_tier0_build_recompiles_ir_and_rejects_self_attested_source_or_graph_swap() {
    use rynvek_core::package::{PackageInput, PackageInputEntry, PackageKind, read_package, write_package};
    let server = start_server(false).await;
    let base = server.base_url();
    let user = register(&base, "alice", "human", 1).await;
    let graph = r#"{"format":"rynvek.rule-graph.v1","name":"signed-ast-build","reads":[],"writes":[{"component":"state.a","ty":"I32"}],"nodes":[{"op":"ConstI32","imm":"1"},{"op":"EmitComponentWrite","component":"state.a","args":[0]}]}"#;
    let bytes = rynvek_rules::package::build_tier0_package("alice.ast", "1.0.0", "CC0-1.0", graph).unwrap();
    let r = publish(
        &base,
        &user,
        "rpack",
        "alice.ast",
        "1.0.0",
        "CC0-1.0",
        &bytes,
        empty_lock(),
    )
    .await;
    assert_eq!(r.status, 201, "{:?}", r.json());
    let response = r.json().unwrap();
    assert!(
        response["verification"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["graph_rir_recompiled_equal"] == true && f["ts_source_equivalence"] == false)
    );
    assert_eq!(response["verification"]["sandbox"]["status"], "not_applicable");
    let p = read_package(&bytes, Some(PackageKind::Rpack)).unwrap();
    for (version, source) in [("1.0.1", false), ("1.0.2", true)] {
        let mut entries: Vec<_> = p
            .manifest
            .entries
            .iter()
            .map(|e| PackageInputEntry {
                path: e.path.clone(),
                kind: e.kind.clone(),
                media_type: e.media_type.clone(),
                data: p.file(&e.path).unwrap().to_vec(),
            })
            .collect();
        if source {
            entries.push(PackageInputEntry {
                path: "rule.ts".into(),
                kind: "rule_source".into(),
                media_type: "text/typescript".into(),
                data: b"export default 0".to_vec(),
            });
        } else {
            entries.iter_mut().find(|e| e.path == "graph.json").unwrap().data =
                graph.replace("\"imm\":\"1\"", "\"imm\":\"2\"").into_bytes();
        }
        // 容器重封装及发布签名都合法，语义伪造仍须由唯一构建验证器拒绝。
        let forged = write_package(&PackageInput {
            kind: PackageKind::Rpack,
            id: "alice.ast".into(),
            version: version.into(),
            name: "signed-ast-build".into(),
            license: "CC0-1.0".into(),
            engine_compat: p.manifest.engine_compat.clone(),
            entries,
        })
        .unwrap();
        assert_error(
            &publish(
                &base,
                &user,
                "rpack",
                "alice.ast",
                version,
                "CC0-1.0",
                &forged,
                empty_lock(),
            )
            .await,
            422,
            0x9004,
        );
    }
    assert_eq!(
        server.state.store().package_versions("alice.ast").await.unwrap().len(),
        1
    );
    server.shutdown().await;
}
