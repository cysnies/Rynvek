// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
mod common;
use common::*;
use rynvek_rts::runtime::ArenaRuntime;
use rynvek_server::{
    client,
    transactions::{TickCommitter, restore_committed},
};
use serde_json::{Value, json};
#[tokio::test]
async fn real_studio_sdk_actor_epoch_param_edit_undo_and_same_wal_restore() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "parameter-owner", "human", 191).await;
    let bob = register(&base, "parameter-other", "human", 192).await;
    let response = client::send_json(
        "POST",
        &format!("{base}/api/v1/worlds"),
        Some(&alice.token),
        &json!({"title":"一般参数模板真实执行","template":"arena_pve","map_seed":42,"visibility":"Private"}),
    )
    .await
    .unwrap();
    assert_eq!(response.status, 201);
    let world = response.json().unwrap()["world_id"].as_str().unwrap().to_string();
    let world_id = world.parse().unwrap();
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let initial = restore_committed(server.state.store(), server.state.inner.world_snapshots.as_ref(), &row)
        .await
        .unwrap()
        .checkpoint()
        .unwrap();
    let opened = client::send_json(
        "POST",
        &format!("{base}/api/v1/worlds/{world}/arena"),
        Some(&alice.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(opened.status, 200);
    let host = server.state.registry().by_world(&world_id).unwrap();
    let live = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running = live.clone();
    let driver = tokio::spawn(async move {
        for _ in 0..900 {
            if !running.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if host.status().state == rynvek_server::store::MatchState::Running {
                host.advance(1).await.unwrap();
            }
        }
    });
    let input = json!({"base":base,"world":world,"principalId":alice.principal_id.to_string(),"token":alice.token});
    let output = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("node")
            .arg("packages/studio/test/parameter-registry-e2e.mjs")
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
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "真实SDK消费失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    })
    .await;
    live.store(false, std::sync::atomic::Ordering::SeqCst);
    driver.await.unwrap();
    let output = output.unwrap();
    assert_eq!(output["templateDiscovered"], true);
    assert_eq!(output["stateContinues"], true);
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let restored = restore_committed(server.state.store(), server.state.inner.world_snapshots.as_ref(), &row)
        .await
        .unwrap();
    assert!(restored.world().parameter_state.entities["counter.one"].values["state.total"] > 0);
    let committer = TickCommitter {
        store: server.state.store().clone(),
        snapshots: server.state.inner.world_snapshots.clone(),
        world_id,
        fencing_token: row.fencing_token,
    };
    let records = committer.records().await.unwrap();
    let mut replayed = ArenaRuntime::restore(world_id.0, 0, initial.state_root, &initial.bytes).unwrap();
    for record in &records {
        replayed = record.replay(&replayed).unwrap();
    }
    assert_eq!(replayed.state_root(), restored.state_root());
    assert_eq!(replayed.world().parameter_state, restored.world().parameter_state);
    assert!(records.iter().any(|r| {
        r.inputs
            .iter()
            .any(|i| matches!(i, rynvek_server::transactions::TickInput::Patch { .. }))
    }));
    let mut suffix = restored.clone();
    let cp = suffix.checkpoint().unwrap();
    let mut from_cp = ArenaRuntime::restore(world_id.0, suffix.revision(), suffix.state_root(), &cp.bytes).unwrap();
    suffix.step().unwrap();
    from_cp.step().unwrap();
    assert_eq!(suffix.state_root(), from_cp.state_root());
    let read = format!("{base}/api/v1/worlds/{world}/authoring");
    assert_eq!(client::get(&read, None).await.unwrap().status, 401);
    assert_eq!(client::get(&read, Some(&bob.token)).await.unwrap().status, 404);
    let cid = rynvek_server::ids::hex(
        &server
            .state
            .store()
            .list_commits(world_id)
            .await
            .unwrap()
            .iter()
            .rev()
            .find_map(|c| c.snapshot_cid)
            .unwrap(),
    );
    assert_eq!(
        client::get(&format!("{base}/api/v1/hub/blobs/{cid}"), None)
            .await
            .unwrap()
            .status,
        404
    );
    server.shutdown().await;
}
