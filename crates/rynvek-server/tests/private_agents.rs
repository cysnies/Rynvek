// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! HTTP私有包消费 + 原Actor/WS/租约/ToolBroker。内存Store定向集成，不是PG多机灾备或真实浏览器验收。
mod common;
use base64::Engine as _;
use common::*;
use futures_util::{SinkExt, StreamExt};
use rynvek_server::{
    agent_package as agent,
    client::{self, HttpResponse},
    ids::hex,
};
use rynvek_world::package::{PackageInput, PackageInputEntry, PackageKind, write_package};
use serde_json::{Value, json};
async fn req(base: &str, method: &str, path: &str, user: Option<&User>, body: Value) -> HttpResponse {
    client::send_json(method, &format!("{base}{path}"), user.map(|u| u.token.as_str()), &body)
        .await
        .unwrap()
}
fn pack(id: &str, role: &str, source: &str, lock: Value) -> Vec<u8> {
    let profile = json!({"schema":"rynvek.agent.v1","runtime":"quickjs-tools/1","role":role,"entrypoint":"main.js","tools":if role=="agent"{vec!["query_visible_clusters","issue_strategic_intent"]}else{vec![]}});
    write_package(&PackageInput {
        kind: PackageKind::Rpack,
        id: id.into(),
        version: "1.0.0".into(),
        name: id.into(),
        license: "MIT".into(),
        engine_compat: "rynvek-agent-tools/1".into(),
        entries: vec![
            PackageInputEntry {
                path: "agent.json".into(),
                kind: "data".into(),
                media_type: "application/json".into(),
                data: serde_json::to_vec(&profile).unwrap(),
            },
            PackageInputEntry {
                path: "dependencies.lock".into(),
                kind: "data".into(),
                media_type: "application/json".into(),
                data: serde_json::to_vec(&lock).unwrap(),
            },
            PackageInputEntry {
                path: "main.js".into(),
                kind: "rule_source".into(),
                media_type: "text/javascript".into(),
                data: source.as_bytes().to_vec(),
            },
        ],
    })
    .unwrap()
}
fn empty() -> Value {
    json!({"lockfile_version":1,"packages":[]})
}
async fn publish(base: &str, user: &User, bytes: &[u8]) -> HttpResponse {
    let parsed = agent::parse(bytes).unwrap();
    let message = agent::signing_message(user.principal_id, &parsed.id, &parsed.version, bytes);
    req(base,"PUT",&format!("/api/v1/private-hub/agents/{}/{}",parsed.id,parsed.version),Some(user),json!({"artifact_b64":base64::engine::general_purpose::STANDARD.encode(bytes),"signature":hex(&user.key.sign(&message))})).await
}
async fn sdk(payload: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        use std::io::Write as _;
        let mut child = std::process::Command::new("node")
            .arg("packages/agent-sdk/test/private-agent-e2e.mjs")
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Node22 SDK直接快验");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "SDK失败：{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("SDK只输出结构化非凭据回执")
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn sdk_container_signature_and_sandbox_rejections_are_real() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/golden_vectors/private-agent-v1.json")).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(fixture["artifact_b64"].as_str().unwrap())
        .unwrap();
    let root = agent::parse(&bytes).unwrap();
    assert_eq!(hex(blake3::hash(&bytes).as_bytes()), fixture["artifact_cid"]);
    assert_eq!(
        hex(&agent::signing_message(42, &root.id, &root.version, &bytes)),
        fixture["signing_message_hex_for_42"]
    );
    let server = start_server(false).await;
    let resolved = agent::resolve(&server.state, 42, root).await.unwrap();
    let scan = agent::scan(&resolved).unwrap();
    assert_eq!(scan["sandbox"]["steps"], 64);
    assert_eq!(scan["trust_level"], 3);
    for source in [
        "function tick(input){while(true){}}",
        "function tick(input){fetch('https://invalid.example');return {call:null,memory:{}};}",
        "function tick(input){return {call:{name:'raw_command',arguments:{}},memory:{}};}",
        "function tick(input){return {memory:{}};}",
        "function tick(input){return {call:{name:'query_visible_clusters',arguments:{include_enemy:true,unknown:true}},memory:{}};}",
    ] {
        let parsed = agent::parse(&pack("negative.probe", "agent", source, empty())).unwrap();
        let resolved = agent::resolve(&server.state, 42, parsed).await.unwrap();
        assert!(agent::scan(&resolved).is_err(), "应拒绝 {source}");
    }
    assert!(
        agent::parse(&pack(
            "negative.secret",
            "agent",
            "const api_key='not-a-real-key'; function tick(x){return {call:null,memory:{}};}",
            empty()
        ))
        .is_err()
    );
    server.shutdown().await;
}
type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn send(ws: &mut Ws, v: Value) {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(v.to_string().into()))
        .await
        .unwrap();
}
async fn recv(ws: &mut Ws, kind: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let f = ws.next().await.unwrap().unwrap();
            if let Ok(s) = f.to_text() {
                let v: Value = serde_json::from_str(s).unwrap();
                if v["type"] == kind {
                    return v;
                }
            }
        }
    })
    .await
    .expect("真实Actor JSON回执超时")
}
#[tokio::test]
#[ignore = "需先构建TS SDK/client并具备Node22；显式--include-ignored运行，见docs/PRIVATE_AGENT_PACKAGES.md"]
async fn private_signed_dependency_consumes_real_fog_and_existing_fenced_toolbroker() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "package-owner", "human", 135).await;
    let bob = register(&base, "package-other", "human", 136).await;
    let lib = pack(
        "owner.policy",
        "library",
        "function transform(input){return input.memory.phase?{unit_archetype:\"all\",order_kind:\"HoldPosition\",target_world_x:1,target_world_y:1}:{include_enemy:input.observation.tick%60===0};}",
        empty(),
    );
    let lib_cid = hex(blake3::hash(&lib).as_bytes());
    let r = publish(&base, &alice, &lib).await;
    assert_eq!(r.status, 201, "{:?}", r.json());
    let root = pack(
        "owner.agent",
        "agent",
        r#"function tick(input){return {call:{name:input.memory.phase?"issue_strategic_intent":"query_visible_clusters",arguments:deps["owner.policy"](input)},memory:{phase:(input.memory.phase||0)+1,tick:input.observation.tick,faction:input.observation.faction,units:input.observation.units.length,keys:Object.keys(input).sort(),io:typeof fetch==='undefined'&&typeof require==='undefined'&&typeof process==='undefined'}};}"#,
        json!({"lockfile_version":1,"packages":[{"id":"owner.policy","version":"1.0.0","blake3_256":lib_cid}]}),
    );
    let meta=sdk(json!({"mode":"publish","base":base,"principalId":alice.principal_id.to_string(),"token":alice.token,"upload":{"owner_id":alice.principal_id.to_string(),"package_id":"owner.agent","version":"1.0.0","artifact_cid":hex(blake3::hash(&root).as_bytes()),"artifact_b64":base64::engine::general_purpose::STANDARD.encode(&root),"signature":hex(&alice.key.sign(&agent::signing_message(alice.principal_id,"owner.agent","1.0.0",&root)))}})).await;
    assert_eq!(meta["visibility"], "Private");
    let agent_path = "/api/v1/private-hub/agents/owner.agent/1.0.0";
    let artifact_path = format!("{agent_path}/artifact");
    assert_eq!(publish(&base, &alice, &root).await.status, 409);
    assert_eq!(req(&base, "GET", &artifact_path, None, json!({})).await.status, 401);
    assert_eq!(
        req(&base, "GET", &artifact_path, Some(&bob), json!({})).await.status,
        404
    );
    let list = req(&base, "GET", "/api/v1/private-hub/agents", Some(&bob), json!({}))
        .await
        .json()
        .unwrap();
    assert_eq!(list["packages"], json!([]));
    let downloaded = req(&base, "GET", &artifact_path, Some(&alice), json!({})).await;
    assert_eq!(downloaded.body, root);
    assert_eq!(downloaded.header("cache-control"), Some("private, no-store"));
    let public = req(&base, "GET", "/api/v1/hub/packages", None, json!({})).await;
    assert!(!String::from_utf8_lossy(&public.body).contains("owner.agent"));
    assert_eq!(
        req(
            &base,
            "GET",
            &format!("/api/v1/hub/blobs/{}", meta["artifact_cid"].as_str().unwrap()),
            None,
            json!({})
        )
        .await
        .status,
        404
    );
    let created = req(
        &base,
        "POST",
        "/api/v1/worlds",
        Some(&alice),
        json!({"title":"私有工具委托真实世界","template":"arena_pve","map_seed":73,"visibility":"Private"}),
    )
    .await;
    assert_eq!(created.status, 201, "{:?}", created.json());
    let world = created.json().unwrap()["world_id"].as_str().unwrap().to_string();
    assert_eq!(
        req(
            &base,
            "POST",
            &format!("/api/v1/worlds/{world}/arena"),
            Some(&alice),
            json!({})
        )
        .await
        .status,
        200
    );
    let host = server.state.registry().by_world(&world.parse().unwrap()).unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!(
        "{}/api/v1/worlds/{world}/stream?format=arena-json",
        base.replace("http:", "ws:")
    ))
    .await
    .unwrap();
    send(&mut ws,json!({"type":"hello","protocol":"rynvek-arena/1","principal_id":alice.principal_id.to_string(),"kind":"human","faction":0,"capability_token":alice.token})).await;
    assert_eq!(recv(&mut ws, "welcome").await["mode"], "PvE_Survival");
    send(&mut ws, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let lease = recv(&mut ws, "lease").await;
    assert!(lease["lease"]["fencing_token"].as_u64().is_some());
    host.advance(30).await.unwrap();
    let before = host.status();
    let body = json!({"world_id":world,"artifact_cid":meta["artifact_cid"],"memory":{},"last_result":null});
    let evaluated = req(
        &base,
        "POST",
        &format!("{agent_path}/evaluate"),
        Some(&alice),
        body.clone(),
    )
    .await;
    assert_eq!(evaluated.status, 200, "{:?}", evaluated.json());
    let decision = evaluated.json().unwrap();
    assert_eq!(decision["executed"], false);
    assert_eq!(decision["tick"], 30);
    assert_eq!(decision["faction"], 0);
    assert_eq!(decision["output"]["memory"]["io"], true);
    assert_eq!(
        decision["output"]["memory"]["keys"],
        json!(["last_result", "memory", "observation"])
    );
    assert_eq!(
        decision["output"]["call"]["arguments"]["include_enemy"], false,
        "真实tick通过实际依赖函数决定参数"
    );
    assert_eq!(host.status().revision, before.revision);
    assert_eq!(host.status().pending_inputs, 0, "求值不暗中提交工具");
    let run = sdk(
        json!({"mode":"run","base":base,"principalId":alice.principal_id.to_string(),"token":alice.token,"world":world,"id":"owner.agent"}),
    );
    let (receipt, ()) = tokio::join!(run, async {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            host.wait_for(|s| s.pending_inputs > 0),
        )
        .await
        .unwrap();
        host.advance(1).await.unwrap();
        host.advance(30).await.unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(8),
            host.wait_for(|s| s.pending_inputs > 0),
        )
        .await
        .unwrap();
        host.advance(1).await.unwrap();
    });
    assert_eq!(receipt["downloaded"], true);
    assert_eq!(receipt["dispatched"], true);
    assert_eq!(receipt["accepted"], true, "{receipt}");
    assert_eq!(receipt["receipt"]["tool"], "query_visible_clusters");
    assert_eq!(
        receipt["decision"]["output"]["call"]["arguments"]["include_enemy"],
        false
    );
    assert_eq!(receipt["mutation"]["accepted"], true, "{receipt}");
    assert_eq!(receipt["mutation"]["receipt"]["tool"], "issue_strategic_intent");
    assert!(
        receipt["mutation"]["receipt"]["result"]["ack"]["units"]
            .as_u64()
            .unwrap()
            > 0,
        "实际己方单位受控写入，不是只返回候选"
    );
    assert!(
        receipt["orders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "HoldPosition"),
        "{receipt}"
    );
    assert_eq!(
        req(
            &base,
            "POST",
            &format!("{agent_path}/evaluate"),
            Some(&bob),
            body.clone()
        )
        .await
        .status,
        404
    );
    let mut forged = body.clone();
    forged["artifact_cid"] = json!("00".repeat(32));
    assert_ne!(
        req(&base, "POST", &format!("{agent_path}/evaluate"), Some(&alice), forged)
            .await
            .status,
        200
    );
    let revoke = req(
        &base,
        "DELETE",
        "/api/v1/private-hub/agents/owner.policy/1.0.0",
        Some(&alice),
        json!({}),
    )
    .await;
    assert_eq!(revoke.status, 200);
    assert_ne!(
        req(&base, "POST", &format!("{agent_path}/evaluate"), Some(&alice), body)
            .await
            .status,
        200,
        "撤销精确传递依赖阻断后续运行"
    );
    assert_eq!(publish(&base, &alice, &lib).await.status, 409, "撤销不允许换包覆盖");
    drop(ws);
    server.shutdown().await;
}
#[tokio::test]
async fn publication_cannot_spoof_owner_signature_or_public_hub() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "signature-owner", "human", 137).await;
    let bob = register(&base, "signature-other", "human", 138).await;
    let bytes = pack(
        "private.sample",
        "agent",
        "function tick(input){return {call:null,memory:{}};}",
        empty(),
    );
    let signed_for_alice = hex(&alice.key.sign(&agent::signing_message(
        alice.principal_id,
        "private.sample",
        "1.0.0",
        &bytes,
    )));
    let body =
        json!({"artifact_b64":base64::engine::general_purpose::STANDARD.encode(&bytes),"signature":signed_for_alice});
    assert_eq!(
        req(
            &base,
            "PUT",
            "/api/v1/private-hub/agents/private.sample/1.0.0",
            Some(&bob),
            body.clone()
        )
        .await
        .status,
        400
    );
    let mut unknown = body.clone();
    unknown["owner_id"] = json!(alice.principal_id.to_string());
    assert_ne!(
        req(
            &base,
            "PUT",
            "/api/v1/private-hub/agents/private.sample/1.0.0",
            Some(&alice),
            unknown
        )
        .await
        .status,
        201
    );
    for kind in ["agent", "rpack"] {
        let public_signature = hex(&alice.key.sign(&rynvek_server::api::hub::package_signing_message(
            "private.sample",
            "1.0.0",
            &bytes,
        )));
        let r=req(&base,"PUT","/api/v1/hub/packages/private.sample/1.0.0",Some(&alice),json!({"org_id":alice.principal_id.to_string(),"package_kind":kind,"min_engine_version":"1.0.0","license_spdx":"MIT","artifact_b64":body["artifact_b64"],"signature":public_signature,"manifest":{},"lockfile":empty()})).await;
        assert_eq!(r.status, 400, "{:?}", r.json());
    }
    assert!(
        server
            .state
            .store()
            .private_agents(alice.principal_id)
            .await
            .unwrap()
            .is_empty()
    );
    server.shutdown().await;
}
