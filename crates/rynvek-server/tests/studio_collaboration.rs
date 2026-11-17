// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
mod common;
use common::*;
use rynvek_server::{
    ServerConfig,
    app::AppConfig,
    client,
    host::TickDriver,
    store::{CommitRow, EditorPermit, Store},
    transactions::{TickCommitter, restore_committed},
};
use serde_json::{Value, json};
async fn boot(store: Store, dir: &std::path::Path) -> rynvek_server::RunningServer {
    rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        store,
        signer: server_key(),
        app: AppConfig {
            blob_dir: Some(dir.into()),
            tick_driver: TickDriver::Manual,
            lobby_timeout: None,
            ..AppConfig::default()
        },
    })
    .await
    .unwrap()
}
async fn create(base: &str, token: &str) -> String {
    let r = client::send_json(
        "POST",
        &format!("{base}/api/v1/worlds"),
        Some(token),
        &json!({"title":"真实协作私有世界","template":"arena_pve","map_seed":42,"visibility":"Private"}),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 201);
    r.json().unwrap()["world_id"].as_str().unwrap().into()
}
async fn sdk(input: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut c = std::process::Command::new("node")
            .arg("packages/studio/test/collaboration-e2e.mjs")
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let out = c.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "真实协作SDK：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn real_private_multi_editor_sync_undo_revoke_regrant_and_same_store_context_recovery() {
    let store = Store::memory();
    let dir = std::env::temp_dir().join(format!("rynvek-studio-{}", rynvek_server::ids::new_uuid_v7()));
    let server = boot(store.clone(), &dir).await;
    let base = server.base_url();
    let owner = register(&base, "studio-owner", "human", 201).await;
    let editor = register(&base, "studio-editor", "human", 202).await;
    let world = create(&base, &owner.token).await;
    let other = create(&base, &owner.token).await;
    let wid = world.parse().unwrap();
    let row = store.get_world(wid).await.unwrap().unwrap();
    let initial = restore_committed(&store, server.state.inner.world_snapshots.as_ref(), &row)
        .await
        .unwrap()
        .checkpoint()
        .unwrap();
    assert_eq!(
        client::send_json(
            "POST",
            &format!("{base}/api/v1/worlds/{world}/arena"),
            Some(&owner.token),
            &json!({})
        )
        .await
        .unwrap()
        .status,
        200
    );
    let host = server.state.registry().by_world(&wid).unwrap();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let run = flag.clone();
    let drive = tokio::spawn(async move {
        while run.load(std::sync::atomic::Ordering::Acquire) {
            if host.status().state == rynvek_server::store::MatchState::Running {
                host.advance(1).await.unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });
    let out=sdk(json!({"base":base,"world":world,"otherWorld":other,"owner":owner.token,"editor":editor.token,"ownerId":owner.principal_id.to_string(),"editorId":editor.principal_id.to_string()})).await;
    flag.store(false, std::sync::atomic::Ordering::Release);
    drive.await.unwrap();
    assert_eq!(out["synced"], true);
    let row = store.get_world(wid).await.unwrap().unwrap();
    let restored = restore_committed(&store, server.state.inner.world_snapshots.as_ref(), &row)
        .await
        .unwrap();
    let records = TickCommitter {
        store: store.clone(),
        snapshots: server.state.inner.world_snapshots.clone(),
        world_id: wid,
        fencing_token: row.fencing_token,
    }
    .records()
    .await
    .unwrap();
    let mut replay = rynvek_rts::runtime::ArenaRuntime::restore(wid.0, 0, initial.state_root, &initial.bytes).unwrap();
    for r in records {
        replay = r.replay(&replay).unwrap();
    }
    assert_eq!(replay.state_root(), restored.state_root());
    // 原Store同一事务重验准入版本，不能只信HTTP开始时的许可。
    let denied = store
        .append_commit_checked(
            CommitRow {
                world_id: wid,
                world_revision: row.latest_revision + 1,
                tick: row.latest_tick,
                parent_revision: Some(row.latest_revision),
                state_root_blake3: row.latest_state_root,
                journal_segment_cid: [0; 32],
                snapshot_cid: None,
            },
            row.fencing_token,
            vec![EditorPermit {
                principal_id: editor.principal_id,
                grant_id: Some(out["oldGrant"].as_str().unwrap().parse().unwrap()),
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(denied.status, 403);
    assert_eq!(
        store.get_world(wid).await.unwrap().unwrap().latest_revision,
        row.latest_revision
    );
    server.shutdown().await;
    let restarted = boot(store.clone(), &dir).await;
    let proof=sdk(json!({"phase":"recover","base":restarted.base_url(),"world":world,"owner":owner.token,"editor":editor.token,"grant":out["grant"]})).await;
    assert_eq!(proof["recovered"], true);
    restarted.shutdown().await;
}
#[tokio::test]
async fn queued_editor_revocation_rejects_patch_without_aborting_owner_actor() {
    let s = start_server(false).await;
    let base = s.base_url();
    let a = register(&base, "queued-owner", "human", 203).await;
    let b = register(&base, "queued-editor", "human", 204).await;
    let world = create(&base, &a.token).await;
    let wid = world.parse().unwrap();
    let grant=client::send_json("POST",&format!("{base}/api/v1/worlds/{world}/authoring/editors"),Some(&a.token),&json!({"principal_id":b.principal_id.to_string(),"expected_grant_id":null,"active":true,"consent":"studio-source-edit/1"})).await.unwrap();
    assert_eq!(grant.status, 200);
    let gid = grant.json().unwrap()["grant_id"].as_str().unwrap().to_owned();
    assert_eq!(
        client::send_json(
            "POST",
            &format!("{base}/api/v1/worlds/{world}/arena"),
            Some(&a.token),
            &json!({})
        )
        .await
        .unwrap()
        .status,
        200
    );
    let mut link = StreamClient::new(
        rynvek_network::websocket::connect(&format!("{}/api/v1/worlds/{world}/stream", base.replace("http", "ws")))
            .await
            .unwrap(),
        a.principal_id,
    );
    link.hello(a.token_bytes(), wid, 0).await;
    let host = s.state.registry().by_world(&wid).unwrap();
    host.wait_for(|h| h.state == rynvek_server::store::MatchState::Running)
        .await;
    let patch:rynvek_world::WorldPatch=serde_json::from_value(json!({"patch_id":vec![99;16],"base_authoring_revision":host.status().revision,"hlc":{"wall_time_ms":1,"logical_counter":0,"author_node_id":2},"safety_class":"EpochBoundarySafe","operations":[{"SetWorldParam":{"key_path":"rules.templates.counter","new_value_json":json!({"schema":"rynvek.parameter-template.v1","components":{"state.x":{"ty":"I32","default":0}}}).to_string()}}]})).unwrap();
    let h = host.clone();
    let task = tokio::spawn(async move {
        h.apply_patch_as(
            patch,
            Some(EditorPermit {
                principal_id: b.principal_id,
                grant_id: Some(gid.parse().unwrap()),
            }),
        )
        .await
    });
    host.wait_for(|h| h.pending_inputs > 0).await;
    let g = s.state.store().editor_grants(wid).await.unwrap().remove(0);
    assert_eq!(client::send_json("POST",&format!("{base}/api/v1/worlds/{world}/authoring/editors"),Some(&a.token),&json!({"principal_id":g.principal_id.to_string(),"expected_grant_id":g.grant_id.to_string(),"active":false,"consent":"studio-source-edit/1"})).await.unwrap().status,200);
    host.advance(4).await.unwrap();
    assert_eq!(task.await.unwrap().unwrap_err().status, 403);
    assert_eq!(host.status().state, rynvek_server::store::MatchState::Running);
    let row = s.state.store().get_world(wid).await.unwrap().unwrap();
    let r = restore_committed(s.state.store(), s.state.inner.world_snapshots.as_ref(), &row)
        .await
        .unwrap();
    assert!(r.world().authoring.parameter_templates.is_empty());
    s.shutdown().await;
}
