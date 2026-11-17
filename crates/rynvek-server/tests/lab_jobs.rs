// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 本机真实认证HTTP + TS SDK + 私有文件CAS + 同Store重建定向用例。不是PG多机/断电灾备。
mod common;
use common::*;
use rynvek_server::{
    RunningServer, ServerConfig,
    app::AppConfig,
    client,
    ids::new_uuid_v7,
    lab_jobs::{KERNEL_PROFILE, WorkerAction},
    store::Store,
};
use serde_json::{Value, json};
async fn start(store: Store, dir: &std::path::Path) -> RunningServer {
    rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: AppConfig {
            blob_dir: Some(dir.into()),
            lab_max_concurrent: 1,
            ..AppConfig::default()
        },
        store,
        signer: server_key(),
    })
    .await
    .unwrap()
}
async fn sdk(payload: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        use std::io::Write as _;
        let mut child = std::process::Command::new("node")
            .arg("packages/lab/test/durable-e2e.mjs")
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
        assert!(
            output.status.success(),
            "SDK执行失败：{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    })
    .await
    .unwrap()
}
async fn get(base: &str, path: &str, user: &User) -> client::HttpResponse {
    client::send_json("GET", &format!("{base}{path}"), Some(&user.token), &json!({}))
        .await
        .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_sdk_owner_isolation_checkpoint_recovery_and_private_atomic_results() {
    let store = Store::memory();
    let dir = std::env::temp_dir().join(format!("rynvek-stage33-{}", new_uuid_v7()));
    let server = start(store.clone(), &dir).await;
    rynvek_server::lab_jobs::stop_workers(&server.state).await;
    let base = server.base_url();
    let owner = register(&base, "durable-lab-owner", "human", 71).await;
    let other = register(&base, "durable-lab-other", "human", 72).await;
    let key = new_uuid_v7().to_string();
    let receipt = sdk(json!({"phase":"submit","base":base,"token":owner.token,"key":key})).await;
    let id = receipt["id"].as_str().unwrap().to_owned();
    assert_eq!(receipt["owner"], owner.principal_id.to_string());
    let path = format!("/api/v1/lab/experiments/{id}");
    assert_eq!(get(&base, &path, &other).await.status, 404);
    assert_eq!(
        get(&base, &format!("{path}/rows?table=energy"), &other).await.status,
        404
    );
    assert_eq!(get(&base, &format!("{path}/parquet"), &other).await.status, 404);
    assert_eq!(
        get(&base, "/api/v1/lab/experiments", &other).await.json().unwrap()["experiments"],
        json!([])
    );
    let bad = client::send_json(
        "POST",
        &format!("{base}{path}/control"),
        Some(&other.token),
        &json!({"operation_id":new_uuid_v7().to_string(),"generation":1,"action":"cancel"}),
    )
    .await
    .unwrap();
    assert_eq!(bad.status, 404);
    // 模拟中断前已落盘的真实run，不制造实验数据。剩余run由重建服务真正计算。
    let job = store
        .lab_claim("interrupted-test-worker".into())
        .await
        .unwrap()
        .unwrap();
    let claim = job.claim_ref();
    let spec = job.experiment.spec();
    let plan = spec.runs().remove(0);
    let result = rynvek_rts::lab::run_one(&plan, spec.ticks, spec.sample_every);
    #[derive(serde::Serialize)]
    struct Checkpoint<'a> {
        profile: &'a str,
        spec_digest: &'a str,
        run: u32,
        result: rynvek_rts::lab::ExperimentResult,
    }
    let bytes = serde_json::to_vec(&Checkpoint {
        profile: KERNEL_PROFILE,
        spec_digest: &job.experiment.spec_digest,
        run: plan.run,
        result,
    })
    .unwrap();
    let cid = server.state.inner.world_snapshots.put(&bytes).unwrap().to_hex();
    store
        .lab_worker(
            claim.clone(),
            WorkerAction::Checkpoint {
                run: plan.run,
                cid: cid.clone(),
            },
        )
        .await
        .unwrap();
    store.lab_worker(claim.clone(), WorkerAction::Release).await.unwrap();
    assert!(store.lab_worker(claim, WorkerAction::Fail).await.is_err());
    assert!(
        store
            .lab_get(owner.principal_id, id.clone())
            .await
            .unwrap()
            .unwrap()
            .experiment
            .tables
            .is_empty()
    );
    assert_eq!(get(&base, &format!("{path}/parquet"), &owner).await.status, 404);
    server.shutdown().await;
    let restarted = start(store.clone(), &dir).await;
    let base = restarted.base_url();
    let done = sdk(json!({"phase":"download","base":base,"token":owner.token,"id":id})).await;
    assert_eq!(done["status"], "completed");
    assert_eq!(done["runs_done"], 2);
    assert_eq!(done["attempt"], 2);
    assert_eq!(done["tables"], json!(["energy", "lineage", "metrics", "runs"]));
    let persisted = store.lab_get(owner.principal_id, id.clone()).await.unwrap().unwrap();
    assert_eq!(persisted.chunks[&0], cid);
    assert_eq!(persisted.chunks.len(), 2);
    assert_eq!(persisted.experiment.tables.len(), 4);
    let public = format!("/api/v1/hub/blobs/{}", persisted.experiment.tables["metrics"]);
    assert_eq!(get(&base, &public, &other).await.status, 404);
    let exported = client::send_json(
        "POST",
        &format!("{base}/api/v1/user/data-export"),
        Some(&owner.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(exported.json().unwrap()["experiments"][0]["exp_id"], id);
    restarted.shutdown().await;
}
