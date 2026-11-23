// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Memory只测状态机/真实HTTP后台；PG用独立OS子进程SIGKILL，不混称Memory重建。
mod common;
use common::*;
use rynvek_protocol::WireUuid;
use rynvek_server::{
    AppConfig, RunningServer, ServerConfig, client,
    general_runs::GeneralRun,
    ids::new_uuid_v7,
    store::{PostgresStore, Store},
};
use rynvek_world::general::GeneralWorldConfig;
use serde_json::{Value, json};
use std::time::Duration;
fn rules() -> GeneralWorldConfig {
    serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap()
}
async fn start(store: Store, path: &std::path::Path) -> RunningServer {
    rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: AppConfig {
            blob_dir: Some(path.to_owned()),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            lobby_timeout: None,
            ..AppConfig::default()
        },
        store,
        signer: server_key(),
    })
    .await
    .unwrap()
}
async fn post(base: &str, u: &User, path: &str, v: Value) -> client::HttpResponse {
    client::send_json("POST", &format!("{base}{path}"), Some(&u.token), &v)
        .await
        .unwrap()
}
async fn ok(base: &str, u: &User, path: &str, v: Value) -> Value {
    let r = post(base, u, path, v).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
async fn action(base: &str, u: &User, w: WireUuid, v: Value) -> Value {
    ok(base, u, &format!("/api/v1/worlds/{w}/general"), v).await
}
async fn create(base: &str, u: &User) -> WireUuid {
    let bytes = rules().build_package("world.test.durable", "0.1.0").unwrap();
    let r = client::request(
        "POST",
        &format!("{base}/api/v1/worlds/general"),
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
async fn setup(base: &str, u: &User, w: WireUuid, ticks: u32, interval: u32) -> Value {
    let c = action(base, u, w, json!({"action":"connect"})).await;
    let h = c["host_fence"].as_u64().unwrap();
    let l = action(
        base,
        u,
        w,
        json!({"action":"acquire","host_fence":h,"level":"Sovereign"}),
    )
    .await;
    json!({"action":"start_run","host_fence":h,"control_fence":l["lease"]["fencing_token"],"spec":{
  "submission_id":new_uuid_v7().to_string(),"expected_revision":c["observation"]["commit"]["revision"],"expected_root":c["observation"]["commit"]["state_root"],
  "ticks":ticks,"wall_seconds":120,"interval_ms":interval,"max_energy_q":"1000000000","consent":rynvek_server::general_runs::CONSENT}})
}
async fn get(base: &str, u: &User, w: WireUuid, id: &str) -> GeneralRun {
    let r = client::get(&format!("{base}/api/v1/worlds/{w}/general/runs/{id}"), Some(&u.token))
        .await
        .unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    assert_eq!(r.header("cache-control"), Some("private, no-store"));
    serde_json::from_value(r.json().unwrap()).unwrap()
}
async fn until(base: &str, u: &User, w: WireUuid, id: &str, check: impl Fn(&GeneralRun) -> bool) -> GeneralRun {
    tokio::time::timeout(Duration::from_secs(65), async {
        loop {
            let j = get(base, u, w, id).await;
            if check(&j) {
                return j;
            }
            assert!(!matches!(j.status.as_str(), "blocked" | "expired"), "{j:?}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("等待有界服务调度")
}
async fn control(base: &str, u: &User, w: WireUuid, j: &GeneralRun, op: &str, key: WireUuid) -> GeneralRun {
    serde_json::from_value(
        ok(
            base,
            u,
            &format!("/api/v1/worlds/{w}/general/runs/{}", j.id),
            json!({"operation_id":key.to_string(),"generation":j.generation,"action":op}),
        )
        .await,
    )
    .unwrap()
}
#[tokio::test]
async fn explicit_background_intent_paid_ticks_and_persistent_submission_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let b = s.base_url();
    let u = register(&b, "durable-owner", "human", 141).await;
    let w = create(&b, &u).await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        s.state.store().get_world(w).await.unwrap().unwrap().latest_tick,
        0,
        "未明确启动绝不自跑"
    );
    let req = setup(&b, &u, w, 4, 200).await;
    let mut no_consent = req.clone();
    no_consent["spec"]["consent"] = json!("");
    assert_eq!(
        post(&b, &u, &format!("/api/v1/worlds/{w}/general"), no_consent)
            .await
            .status,
        400
    );
    let j: GeneralRun = serde_json::from_value(action(&b, &u, w, req.clone()).await).unwrap();
    let receipt = client::get(
        &format!(
            "{b}/api/v1/worlds/{w}/general/runs?submission_id={}",
            j.spec.submission_id
        ),
        Some(&u.token),
    )
    .await
    .unwrap();
    assert_eq!(receipt.status, 200);
    assert_eq!(receipt.json().unwrap()["id"], j.id);
    let mut collision = req.clone();
    collision["spec"]["ticks"] = json!(5);
    assert_eq!(
        post(&b, &u, &format!("/api/v1/worlds/{w}/general"), collision)
            .await
            .status,
        409
    );
    let end = until(&b, &u, w, &j.id, |j| j.status == "completed").await;
    assert_eq!(end.storage_backend, "memory");
    assert_eq!(end.done, 4);
    assert!(end.energy_used_q > 0);
    let row = s.state.store().get_world(w).await.unwrap().unwrap();
    assert_eq!(row.latest_tick, 4);
    assert_eq!(rynvek_server::ids::hex(&row.latest_state_root), end.last_root);
    let old: GeneralRun = serde_json::from_value(action(&b, &u, w, req).await).unwrap();
    assert_eq!(old.id, j.id);
    assert_eq!(old.done, 4);
    let mut expected = rules().instantiate(w.0).unwrap();
    let mut paid = 0;
    for _ in 0..4 {
        expected.tick_with_commands(&[]).unwrap();
        paid += expected.component_rules().unwrap().paid_q;
    }
    assert_eq!(paid, end.energy_used_q);
    assert_eq!(
        expected.parent_commit_hash, row.latest_state_root,
        "原规则/有来源结算/提交根未另造"
    );
    let (_, root) = (end.done, end.last_root.clone());
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(get(&b, &u, w, &j.id).await.last_root, root);
    s.shutdown().await;
}
#[tokio::test]
async fn pause_cancel_fences_old_candidates_and_driver_cannot_start_background() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let b = s.base_url();
    let u = register(&b, "pause-owner", "human", 142).await;
    let other = register(&b, "pause-outsider", "human", 143).await;
    let w = create(&b, &u).await;
    let req = setup(&b, &u, w, 20, 500).await;
    let grant=ok(&b,&u,&format!("/api/v1/worlds/{w}/general/drivers"),json!({"principal_id":other.principal_id.to_string(),"expected_grant_id":null,"active":true,"consent":"general-installed-rule-drive/1"})).await;
    let driver = post(
        &b,
        &other,
        &format!("/api/v1/worlds/{w}/general/drive"),
        json!({"grant_id":grant["grant_id"],"command":req.clone()}),
    )
    .await;
    assert_eq!(
        driver.status,
        403,
        "合法目的请求仍不能升级后台启动：{:?}",
        driver.json()
    );
    let j: GeneralRun = serde_json::from_value(action(&b, &u, w, req).await).unwrap();
    let j = until(&b, &u, w, &j.id, |j| j.done >= 1).await;
    let claim = j.claim_of();
    let key = new_uuid_v7();
    let p = control(&b, &u, w, &j, "pause", key).await;
    assert_eq!(p.status, "paused");
    let replay = control(&b, &u, w, &j, "pause", key).await;
    assert_eq!(replay.generation, p.generation);
    let base = s.state.store().get_world(w).await.unwrap().unwrap();
    let fake = rynvek_server::store::CommitRow {
        world_id: w,
        world_revision: base.latest_revision + 1,
        tick: base.latest_tick + 1,
        parent_revision: Some(base.latest_revision),
        state_root_blake3: [1; 32],
        journal_segment_cid: [2; 32],
        snapshot_cid: None,
    };
    assert!(
        s.state
            .store()
            .append_commit_running(
                fake,
                base.fencing_token,
                vec![],
                vec![],
                Some(rynvek_server::general_runs::RunPermit {
                    claim,
                    step: j.done + 1,
                    energy_q: 1
                })
            )
            .await
            .is_err(),
        "暂停返回后旧候选最终锁必拒绝"
    );
    tokio::time::sleep(Duration::from_millis(650)).await;
    let still = get(&b, &u, w, &j.id).await;
    assert_eq!(still.done, p.done);
    assert_eq!(still.last_root, p.last_root);
    assert_eq!(
        client::get(
            &format!("{b}/api/v1/worlds/{w}/general/runs/{}", j.id),
            Some(&other.token)
        )
        .await
        .unwrap()
        .status,
        404
    );
    let r = control(&b, &u, w, &p, "resume", new_uuid_v7()).await;
    assert_eq!(r.deadline_s, p.deadline_s);
    let r = until(&b, &u, w, &j.id, |j| j.done > p.done).await;
    let c = control(&b, &u, w, &r, "cancel", new_uuid_v7()).await;
    assert_eq!(c.status, "cancelled");
    tokio::time::sleep(Duration::from_millis(650)).await;
    assert_eq!(get(&b, &u, w, &j.id).await.done, c.done);
    let h = action(&b, &u, w, json!({"action":"connect"})).await["host_fence"]
        .as_u64()
        .unwrap();
    let l = action(
        &b,
        &u,
        w,
        json!({"action":"acquire","host_fence":h,"level":"Sovereign"}),
    )
    .await;
    assert!(
        l["lease"]["fencing_token"].as_u64().unwrap() > 1,
        "不重置原控制fence计数"
    );
    s.shutdown().await;
}
#[tokio::test]
async fn energy_budget_failure_never_publishes_free_world_or_progress() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let b = s.base_url();
    let u = register(&b, "budget-owner", "human", 144).await;
    let w = create(&b, &u).await;
    let mut req = setup(&b, &u, w, 4, 200).await;
    req["spec"]["max_energy_q"] = json!("1");
    let j: GeneralRun = serde_json::from_value(action(&b, &u, w, req).await).unwrap();
    let j = until(&b, &u, w, &j.id, |j| j.status == "blocked").await;
    assert_eq!(j.done, 0);
    assert_eq!(j.energy_used_q, 0);
    assert_eq!(s.state.store().get_world(w).await.unwrap().unwrap().latest_tick, 0);
    s.shutdown().await;
}
// 下列辅助入口是独立OS服务进程，不在父测试重建Memory/AppState。
#[tokio::test]
#[ignore = "仅由SIGKILL持久恢复测试启动的辅助进程"]
async fn durable_server_subprocess() {
    if std::env::var("RYNVEK_DURABLE_CHILD").as_deref() != Ok("1") {
        return;
    }
    let pg = PostgresStore::connect(&std::env::var("RYNVEK_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    pg.migrate().await.unwrap();
    let s = start(
        Store::Postgres(pg),
        std::path::Path::new(&std::env::var("RYNVEK_DURABLE_CAS").unwrap()),
    )
    .await;
    println!("RYNVEK_CHILD_URL={}", s.base_url());
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    std::future::pending::<()>().await;
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(path: &std::path::Path) -> (Child, String) {
    use std::io::BufRead;
    let mut c = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "durable_server_subprocess",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("RYNVEK_DURABLE_CHILD", "1")
        .env("RYNVEK_DURABLE_CAS", path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = c.stdout.take().unwrap();
    let guard = Child(c);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some((_, url)) = line.split_once("RYNVEK_CHILD_URL=") {
                let _ = tx.send(url.to_owned());
                break;
            }
        }
    });
    let url = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("子进程启动须在20秒内报告真实地址");
    (guard, url)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "必须提供独立合成测试PG DSN，显式执行，不拿Memory重建冒称"]
async fn postgres_real_process_sigkill_restart_recovers_exact_once_paid_progress() {
    let dsn = std::env::var("RYNVEK_TEST_DATABASE_URL").expect("需独立测试数据库");
    let pg = PostgresStore::connect(&dsn).await.unwrap();
    pg.migrate().await.unwrap();
    let store = Store::Postgres(pg);
    let dir = tempfile::tempdir().unwrap().keep();
    let (mut first, b) = child(&dir);
    let key = rynvek_security::ServerSigningKey::from_seed(0, *blake3::hash(&new_uuid_v7().0).as_bytes());
    let handle = format!("durable-pg-{}", &new_uuid_v7().to_string()[24..]);
    let (principal_id, token) = client::login(
        &b,
        &key,
        client::LoginTarget::Register {
            handle: &handle,
            kind: "human",
        },
    )
    .await
    .unwrap();
    let u = User {
        principal_id,
        token,
        key,
    };
    let w = create(&b, &u).await;
    let req = setup(&b, &u, w, 8, 500).await;
    let j: GeneralRun = serde_json::from_value(action(&b, &u, w, req.clone()).await).unwrap();
    let j = until(&b, &u, w, &j.id, |j| j.done >= 2).await;
    let p = control(&b, &u, w, &j, "pause", new_uuid_v7()).await;
    tokio::time::sleep(Duration::from_millis(650)).await;
    assert_eq!(get(&b, &u, w, &j.id).await.done, p.done);
    control(&b, &u, w, &p, "resume", new_uuid_v7()).await;
    until(&b, &u, w, &j.id, |j| j.done > p.done).await;
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    let frozen = store
        .general_run_get(u.principal_id, j.id.clone())
        .await
        .unwrap()
        .unwrap();
    let before = store.get_world(w).await.unwrap().unwrap();
    assert_eq!(before.latest_tick, frozen.done as u64);
    assert_eq!(rynvek_server::ids::hex(&before.latest_state_root), frozen.last_root);
    assert!(frozen.done < 8);
    let old_host = before.fencing_token;
    let (_second, b2) = child(&dir);
    tokio::time::sleep(Duration::from_millis(350)).await;
    let waiting = store.get_world(w).await.unwrap().unwrap();
    assert_eq!(waiting.latest_tick, frozen.done as u64);
    assert_eq!(waiting.fencing_token, old_host, "原健康30秒宿主窗口内不得重置fence");
    let final_job = until(&b2, &u, w, &j.id, |j| j.status == "completed").await;
    assert_eq!(final_job.storage_backend, "postgres");
    assert_eq!(final_job.done, 8);
    assert!(final_job.fence > frozen.fence);
    assert_eq!(final_job.deadline_s, frozen.deadline_s);
    let after = store.get_world(w).await.unwrap().unwrap();
    assert_eq!(after.latest_tick, 8);
    assert!(after.fencing_token > old_host, "崩溃恢复不得复用旧宿主fence");
    let mut expected = rules().instantiate(w.0).unwrap();
    let mut paid = 0;
    for _ in 0..8 {
        expected.tick_with_commands(&[]).unwrap();
        paid += expected.component_rules().unwrap().paid_q;
    }
    assert_eq!(paid, final_job.energy_used_q);
    assert_eq!(expected.parent_commit_hash, after.latest_state_root);
    let records = store.list_commits(w).await.unwrap();
    assert_eq!(records.len(), 9, "每步只能有一个原权威提交");
    let old: GeneralRun = serde_json::from_value(action(&b2, &u, w, req).await).unwrap();
    assert_eq!(old.id, j.id);
    assert_eq!(old.done, 8);
    // 第二条PG运行检验暂停返回后的旧候选，使用独立父进程连接击中同一最终提交锁。
    let second_request = setup(&b2, &u, w, 4, 500).await;
    let second: GeneralRun = serde_json::from_value(action(&b2, &u, w, second_request).await).unwrap();
    let second = until(&b2, &u, w, &second.id, |j| j.done >= 1).await;
    let old_claim = second.claim_of();
    let paused = control(&b2, &u, w, &second, "pause", new_uuid_v7()).await;
    let base = store.get_world(w).await.unwrap().unwrap();
    let candidate = rynvek_server::store::CommitRow {
        world_id: w,
        world_revision: base.latest_revision + 1,
        tick: base.latest_tick + 1,
        parent_revision: Some(base.latest_revision),
        state_root_blake3: [1; 32],
        journal_segment_cid: [2; 32],
        snapshot_cid: None,
    };
    assert!(
        store
            .append_commit_running(
                candidate,
                base.fencing_token,
                vec![],
                vec![],
                Some(rynvek_server::general_runs::RunPermit {
                    claim: old_claim,
                    step: second.done + 1,
                    energy_q: 20
                })
            )
            .await
            .is_err()
    );
    assert_eq!(
        store.get_world(w).await.unwrap().unwrap().latest_state_root,
        base.latest_state_root
    );
    assert_eq!(
        store
            .general_run_get(u.principal_id, second.id.clone())
            .await
            .unwrap()
            .unwrap()
            .done,
        paused.done
    );
    control(&b2, &u, w, &paused, "cancel", new_uuid_v7()).await;
    println!("合成私有CAS保留：{}", dir.display());
    println!(
        "真实PG17/独立服务SIGKILL：崩溃进度{}，恢复至8；job fence {}→{}，host fence {}→{}，费用{}，9个含genesis提交",
        frozen.done, frozen.fence, final_job.fence, old_host, after.fencing_token, paid
    );
}

#[tokio::test]
async fn disabled_original_owner_revokes_background_at_commit_and_leaves_progress_frozen() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let b = s.base_url();
    let u = register(&b, "durable-disabled-owner", "human", 146).await;
    let w = create(&b, &u).await;
    let req = setup(&b, &u, w, 20, 500).await;
    let j: GeneralRun = serde_json::from_value(action(&b, &u, w, req).await).unwrap();
    let j = until(&b, &u, w, &j.id, |j| j.done >= 1).await;
    let claim = j.claim_of();
    s.state.store().tombstone_principal(u.principal_id).await.unwrap();
    let frozen = s.state.store().get_world(w).await.unwrap().unwrap();
    let candidate = rynvek_server::store::CommitRow {
        world_id: w,
        world_revision: frozen.latest_revision + 1,
        tick: frozen.latest_tick + 1,
        parent_revision: Some(frozen.latest_revision),
        state_root_blake3: [1; 32],
        journal_segment_cid: [2; 32],
        snapshot_cid: None,
    };
    assert!(
        s.state
            .store()
            .append_commit_running(
                candidate,
                frozen.fencing_token,
                vec![],
                vec![],
                Some(rynvek_server::general_runs::RunPermit {
                    claim,
                    step: j.done + 1,
                    energy_q: 20
                })
            )
            .await
            .is_err()
    );
    let stopped = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let j = s
                .state
                .store()
                .general_run_get(u.principal_id, j.id.clone())
                .await
                .unwrap()
                .unwrap();
            if j.status == "blocked" {
                break j;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(stopped.base_tick + stopped.done as u64, frozen.latest_tick);
    assert_eq!(
        s.state.store().get_world(w).await.unwrap().unwrap().latest_state_root,
        frozen.latest_state_root
    );
    assert_ne!(
        client::get(&format!("{b}/api/v1/worlds/{w}/general/runs/{}", j.id), Some(&u.token))
            .await
            .unwrap()
            .status,
        200
    );
    s.shutdown().await;
}
