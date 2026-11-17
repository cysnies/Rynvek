// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 本机真实HTTP/WS、两次独立Node控制进程与原Actor。非外部LLM/PG多机/服务端断电测试。
mod common;
use base64::Engine as _;
use common::*;
use rynvek_server::{agent_package as agent, client, ids::hex};
use rynvek_world::package::{PackageInput, PackageInputEntry, PackageKind, write_package};
use serde_json::{Value, json};
async fn req(base: &str, method: &str, path: &str, user: &User, body: Value) -> client::HttpResponse {
    client::send_json(method, &format!("{base}{path}"), Some(&user.token), &body)
        .await
        .unwrap()
}
async fn sdk(payload: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        use std::io::Write as _;
        let mut child = std::process::Command::new("node")
            .arg("packages/agent-sdk/test/agent-run-e2e.mjs")
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
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "真实SDK失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    })
    .await
    .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_real_provider_paths_persist_events_cancel_and_resume_across_runner_processes() {
    let server = start_server(false).await;
    let base = server.base_url();
    let owner = register(&base, "agent-run-owner", "human", 81).await;
    let other = register(&base, "agent-run-other", "human", 82).await;
    let source = r#"function tick(input){return {call:input.memory.phase?{name:'issue_strategic_intent',arguments:{unit_archetype:'all',order_kind:'HoldPosition',target_world_x:1,target_world_y:1}}:{name:'query_visible_clusters',arguments:{include_enemy:false}},memory:{phase:(input.memory.phase||0)+1,saw_real_result:!!(input.last_result&&input.last_result.kind==='broker_result'),io:typeof fetch==='undefined'&&typeof process==='undefined'}};}"#;
    let files=[("agent.json","data","application/json",serde_json::to_vec(&json!({"schema":"rynvek.agent.v1","runtime":"quickjs-tools/1","role":"agent","entrypoint":"main.js","tools":["query_visible_clusters","issue_strategic_intent"]})).unwrap()),("dependencies.lock","data","application/json",serde_json::to_vec(&json!({"lockfile_version":1,"packages":[]})).unwrap()),("main.js","rule_source","text/javascript",source.as_bytes().to_vec())];
    let bytes = write_package(&PackageInput {
        kind: PackageKind::Rpack,
        id: "run.agent".into(),
        version: "1.0.0".into(),
        name: "独立受控运行快验".into(),
        license: "MIT".into(),
        engine_compat: "rynvek-agent-tools/1".into(),
        entries: files
            .into_iter()
            .map(|(path, kind, media_type, data)| PackageInputEntry {
                path: path.into(),
                kind: kind.into(),
                media_type: media_type.into(),
                data,
            })
            .collect(),
    })
    .unwrap();
    let upload = json!({"artifact_b64":base64::engine::general_purpose::STANDARD.encode(&bytes),"signature":hex(&owner.key.sign(&agent::signing_message(owner.principal_id,"run.agent","1.0.0",&bytes)))});
    assert_eq!(
        req(
            &base,
            "PUT",
            "/api/v1/private-hub/agents/run.agent/1.0.0",
            &owner,
            upload
        )
        .await
        .status,
        201
    );
    let world = req(
        &base,
        "POST",
        "/api/v1/worlds",
        &owner,
        json!({"title":"持久Agent治理真实世界","template":"arena_pve","map_seed":73,"visibility":"Private"}),
    )
    .await
    .json()
    .unwrap()["world_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        req(
            &base,
            "POST",
            &format!("/api/v1/worlds/{world}/arena"),
            &owner,
            json!({})
        )
        .await
        .status,
        200
    );
    let host = server.state.registry().by_world(&world.parse().unwrap()).unwrap();
    let driver_host = host.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let drive = tokio::spawn(async move {
        while !flag.load(std::sync::atomic::Ordering::Acquire) {
            if driver_host.status().state == rynvek_server::store::MatchState::Running
                && driver_host.status().tick < 600
            {
                driver_host.advance(1).await.unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });
    let receipt=sdk(json!({"phase":"start","base":base,"principalId":owner.principal_id.to_string(),"token":owner.token,"world":world})).await;
    assert_eq!(receipt["status"], "cancelled");
    let id = receipt["resumable"].as_str().unwrap();
    assert_eq!(
        req(&base, "GET", &format!("/api/v1/agent-runs/{id}"), &other, json!({}))
            .await
            .status,
        404
    );
    assert_eq!(
        req(
            &base,
            "GET",
            &format!("/api/v1/agent-runs/{id}/context"),
            &other,
            json!({})
        )
        .await
        .status,
        404
    );
    assert_eq!(
        req(&base, "GET", "/api/v1/agent-runs", &other, json!({}))
            .await
            .json()
            .unwrap()["runs"],
        json!([])
    );
    let persisted = server
        .state
        .store()
        .agent_run_get(owner.principal_id, id.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.calls_used, 1);
    assert_eq!(persisted.memory["phase"], 1);
    let done=sdk(json!({"phase":"resume","base":base,"principalId":owner.principal_id.to_string(),"token":owner.token,"world":world,"runId":id})).await;
    assert_eq!(done["status"], "completed");
    assert_eq!(done["calls"], 2);
    assert!(done["real_units"].as_u64().unwrap() > 0);
    assert!(done["events"].as_u64().unwrap() > receipt["events"].as_u64().unwrap());
    stop.store(true, std::sync::atomic::Ordering::Release);
    drive.await.unwrap();
    server.shutdown().await;
}
