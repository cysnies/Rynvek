// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 浏览器 JSON 适配必须经过现有认证、席位和 Actor，而非第二份仿真。
mod common;
use common::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;

#[tokio::test]
async fn browser_json_stream_uses_the_existing_authorized_actor() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "web-alice", "human", 91).await;
    let bob = register(&base, "web-bob", "human", 92).await;
    common::arena::queue(&base, &alice).await;
    let (_, matched) = common::arena::queue(&base, &bob).await;
    let (_, matched_a) = common::arena::queue(&base, &alice).await;
    let a = common::arena::matched(&matched_a);
    let _b = common::arena::matched(&matched);
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{}?format=arena-json", a.ws_url))
        .await
        .unwrap();
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        json!({
            "type":"hello","protocol":"rynvek-arena/1","principal_id":alice.principal_id.to_string(),
            "kind":"human","faction":a.faction,"capability_token":alice.token
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        frame.is_text(),
        "浏览器必须收到真实 Arena welcome JSON，而不是二进制违例拒绝：{frame:?}"
    );
    let welcome: serde_json::Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["faction"], a.faction);
    drop(ws);
    server.shutdown().await;
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn recv_type(ws: &mut Ws, kind: &str) -> serde_json::Value {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let frame = ws.next().await.expect("帧").expect("连接");
            if let Ok(text) = frame.to_text() {
                let v: serde_json::Value = serde_json::from_str(text).unwrap();
                if v["type"] == kind {
                    return v;
                }
            }
        }
    })
    .await
    .expect("等待 JSON 超时")
}
async fn send_json(ws: &mut Ws, body: serde_json::Value) {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(body.to_string().into()))
        .await
        .unwrap();
}
async fn local_server() -> rynvek_server::RunningServer {
    rynvek_server::start(rynvek_server::ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: rynvek_server::AppConfig {
            public_base_url: String::new(),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            local_practice_origin: Some("http://127.0.0.1:5173".into()),
            ..Default::default()
        },
        store: rynvek_server::store::Store::memory(),
        signer: server_key(),
    })
    .await
    .unwrap()
}
async fn create_practice(server: &rynvek_server::RunningServer) -> serde_json::Value {
    let r = rynvek_server::client::request(
        "POST",
        &format!("{}/api/v1/local/practice", server.base_url()),
        &[("Origin", "http://127.0.0.1:5173"), ("X-Rynvek-Local", "1")],
        Some(b""),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 201, "{:?}", r.json());
    assert_eq!(r.header("cache-control"), Some("no-store"));
    r.json().unwrap()
}
async fn json_connect(
    server: &rynvek_server::RunningServer,
    session: &serde_json::Value,
    token: &str,
    faction: u32,
) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!(
        "{}/api/v1/worlds/{}/stream?format=arena-json",
        server.base_url().replace("http:", "ws:"),
        session["world_id"].as_str().unwrap()
    ))
    .await
    .unwrap();
    send_json(&mut ws,json!({"type":"hello","protocol":"rynvek-arena/1","principal_id":"伪造字段不得成为权威","kind":"Human","faction":faction,"capability_token":token})).await;
    ws
}

#[tokio::test]
async fn local_practice_is_disabled_by_default_and_rejects_other_origins() {
    let normal = start_server(false).await;
    assert_eq!(
        rynvek_server::client::request(
            "POST",
            &format!("{}/api/v1/local/practice", normal.base_url()),
            &[],
            Some(b"")
        )
        .await
        .unwrap()
        .status,
        404
    );
    normal.shutdown().await;
    let server = local_server().await;
    for headers in [
        vec![],
        vec![("Origin", "http://evil.example"), ("X-Rynvek-Local", "1")],
        vec![("Origin", "http://127.0.0.1:5173")],
    ] {
        let r = rynvek_server::client::request(
            "POST",
            &format!("{}/api/v1/local/practice", server.base_url()),
            &headers,
            Some(b""),
        )
        .await
        .unwrap();
        assert_eq!(r.status, 403);
    }
    let mut cfg = rynvek_server::AppConfig {
        local_practice_origin: Some("http://127.0.0.1:5173".into()),
        ..Default::default()
    };
    assert!(
        rynvek_server::start(rynvek_server::ServerConfig {
            http_addr: "0.0.0.0:0".parse().unwrap(),
            webtransport_addr: None,
            app: cfg.clone(),
            store: rynvek_server::store::Store::memory(),
            signer: server_key()
        })
        .await
        .is_err()
    );
    cfg.local_practice_origin = Some("http://evil.example".into());
    assert!(
        rynvek_server::start(rynvek_server::ServerConfig {
            http_addr: "127.0.0.1:0".parse().unwrap(),
            webtransport_addr: None,
            app: cfg,
            store: rynvek_server::store::Store::memory(),
            signer: server_key()
        })
        .await
        .is_err()
    );
    server.shutdown().await;
}

#[tokio::test]
async fn browser_pve_moves_builds_rejects_and_downloads_verifiable_replay() {
    use rynvek_rts::{BuildingKind, FactionId};
    let server = local_server().await;
    let session = create_practice(&server).await;
    let token = session["token"].as_str().unwrap();
    let world_id = session["world_id"].as_str().unwrap().parse().unwrap();
    let host = server.state.registry().by_world(&world_id).unwrap();
    let mut ws = json_connect(&server, &session, token, 0).await;
    assert_eq!(recv_type(&mut ws, "welcome").await["mode"], "PvE_Survival");
    let initial = recv_type(&mut ws, "observation").await["observation"].clone();
    assert!(initial["visibility"].as_array().unwrap().iter().any(|v| v == 0));
    assert!(initial["units"].as_array().unwrap().iter().all(|u| u["faction"] == 0));
    send_json(&mut ws, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let lease = recv_type(&mut ws, "lease").await["lease"]["fencing_token"].clone();
    let id = initial["units"][0]["id"].clone();
    let pos = initial["units"][0]["position"].clone();
    let target = json!([pos[0].as_i64().unwrap() + (3i64 << 32), pos[1].as_i64().unwrap()]);
    send_json(&mut ws,json!({"type":"command","seq":2,"fencing_token":lease,"command":{"type":"issue_order","units":{"kind":"ids","value":[id]},"order":"Move","target":target}})).await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(20).await.unwrap();
    let ack = recv_type(&mut ws, "ack").await;
    assert_eq!(ack["ack"]["type"], "order_issued");
    // 查询实际持久化根，不根据客户端动画判断移动。
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let runtime = rynvek_server::transactions::restore_committed(
        server.state.store(),
        server.state.inner.world_snapshots.as_ref(),
        &row,
    )
    .await
    .unwrap();
    assert_eq!(runtime.state_root(), host.status().state_root);
    assert_ne!(
        serde_json::to_value(runtime.observe(FactionId(0)).unwrap().units[0].position).unwrap(),
        pos
    );
    let before = runtime.observe(FactionId(0)).unwrap();
    let origin = (0..runtime.map.height)
        .flat_map(|y| (0..runtime.map.width).map(move |x| (x, y)))
        .find(|o| {
            runtime
                .validate_placement(FactionId(0), BuildingKind::SignalBeacon, *o)
                .is_ok()
        })
        .expect("初始世界有合法信标位置");
    send_json(&mut ws,json!({"type":"command","seq":3,"fencing_token":lease,"command":{"type":"place_building","kind":"bld.signal_beacon","origin":origin}})).await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert_eq!(recv_type(&mut ws, "ack").await["ack"]["type"], "building_placed");
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let after = rynvek_server::transactions::restore_committed(
        server.state.store(),
        server.state.inner.world_snapshots.as_ref(),
        &row,
    )
    .await
    .unwrap()
    .observe(FactionId(0))
    .unwrap();
    assert_eq!(after.buildings.len(), before.buildings.len() + 1);
    assert!(after.ledger.balance.biomass < before.ledger.balance.biomass);
    // 相同占地再次建造，必须真实拒绝而不添加建筑。
    send_json(&mut ws,json!({"type":"command","seq":4,"fencing_token":lease,"command":{"type":"place_building","kind":"bld.signal_beacon","origin":origin}})).await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert!(recv_type(&mut ws, "ack").await["error"]["code"].is_number());
    send_json(
        &mut ws,
        json!({"type":"command","seq":5,"fencing_token":lease,"command":{"type":"surrender"}}),
    )
    .await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert_eq!(recv_type(&mut ws, "ack").await["ack"]["type"], "surrendered");
    loop {
        let o = recv_type(&mut ws, "observation").await;
        if !o["observation"]["outcome"].is_null() {
            break;
        }
    }
    let mid = session["match_id"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let row = server
                .state
                .store()
                .get_match(mid.parse().unwrap())
                .await
                .unwrap()
                .unwrap()
                .0;
            if row.replay_cid_blake3.is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let replay = rynvek_server::client::get(
        &format!("{}/api/v1/matches/{mid}/replay", server.base_url()),
        Some(token),
    )
    .await
    .unwrap();
    assert_eq!(replay.status, 200);
    let mut keys = rynvek_security::Keyring::new();
    keys.insert(SERVER_KEY_ID, server_key().public_key()).unwrap();
    rynvek_server::replay::verify(&replay.body, &keys).unwrap();
    let stranger = register(&server.base_url(), "web-stranger", "human", 93).await;
    let mut denied = json_connect(&server, &session, &stranger.token, 0).await;
    assert_eq!(recv_type(&mut denied, "error").await["type"], "error");
    drop(denied);
    drop(ws);
    server.shutdown().await;
}

async fn authored_post(
    server: &rynvek_server::RunningServer,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> rynvek_server::client::HttpResponse {
    rynvek_server::client::send_json("POST", &format!("{}{path}", server.base_url()), Some(token), &body)
        .await
        .unwrap()
}
async fn archived(
    server: &rynvek_server::RunningServer,
    id: rynvek_protocol::WireUuid,
) -> rynvek_server::store::MatchRow {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let row = server.state.store().get_match(id).await.unwrap().unwrap().0;
            if row.replay_cid_blake3.is_some() {
                return row;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("真实归档提交超时")
}
#[tokio::test]
async fn studio_world_runs_reconnects_and_replays_full_private_history() {
    use rynvek_server::client;
    let server = start_server(false).await; // practice默认关闭，此链不得调用它。
    let owner = register(&server.base_url(), "arena-author", "human", 81).await;
    let other = register(&server.base_url(), "arena-outsider", "human", 82).await;
    let create = authored_post(
        &server,
        "/api/v1/worlds",
        &owner.token,
        json!({"title":"真实已有创作世界","template":"arena_pve","map_seed":73,"visibility":"Private"}),
    )
    .await;
    assert_eq!(create.status, 201, "{:?}", create.json());
    let world = create.json().unwrap()["world_id"].as_str().unwrap().to_owned();
    let patch=authored_post(&server,&format!("/api/v1/worlds/{world}/patches"),&owner.token,json!({"patch_id":vec![23;16],"base_authoring_revision":0,"hlc":{"wall_time_ms":1,"logical_counter":0,"author_node_id":1},"safety_class":"TickBoundarySafe","operations":[{"PaintTerrainRect":{"x0":100,"y0":100,"width":1,"height":1,"tile_kind":4}}]})).await;
    assert_eq!(patch.status, 200, "{:?}", patch.json());
    let baseline = patch.json().unwrap();
    let path = format!("/api/v1/worlds/{world}/arena");
    assert_eq!(authored_post(&server, &path, &other.token, json!({})).await.status, 404);
    let (one, two) = tokio::join!(
        authored_post(&server, &path, &owner.token, json!({})),
        authored_post(&server, &path, &owner.token, json!({}))
    );
    assert_eq!(one.status, 200, "{:?}", one.json());
    assert_eq!(two.status, 200, "{:?}", two.json());
    assert_eq!(one.header("cache-control"), Some("no-store"));
    let run = one.json().unwrap();
    assert_eq!(run["match_id"], two.json().unwrap()["match_id"]);
    assert_eq!(run["world_id"], world);
    assert!(run.get("token").is_none());
    assert_eq!(
        server.state.store().matches_of(owner.principal_id).await.unwrap().len(),
        1
    );
    assert_eq!(
        server
            .state
            .store()
            .list_worlds(Some(owner.principal_id), 100, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    let world_id = world.parse().unwrap();
    let host = server.state.registry().by_world(&world_id).unwrap();
    assert_eq!(host.status().revision, baseline["world_revision"].as_u64().unwrap());
    assert_eq!(
        rynvek_server::ids::hex(&host.status().state_root),
        baseline["state_root"].as_str().unwrap()
    );
    let match_id = run["match_id"].as_str().unwrap();
    let metadata = format!("{}/api/v1/matches/{match_id}", server.base_url());
    assert_eq!(client::get(&metadata, None).await.unwrap().status, 404);
    assert_eq!(client::get(&metadata, Some(&other.token)).await.unwrap().status, 404);
    assert_eq!(
        client::get(&metadata, Some(&owner.token))
            .await
            .unwrap()
            .header("cache-control"),
        Some("no-store")
    );
    let mut ws = json_connect(&server, &run, &owner.token, 0).await;
    assert_eq!(recv_type(&mut ws, "welcome").await["mode"], "PvE_Survival");
    let initial = recv_type(&mut ws, "observation").await["observation"].clone();
    assert!(!initial.to_string().contains("w_in"));
    send_json(&mut ws, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let lease = recv_type(&mut ws, "lease").await["lease"]["fencing_token"].clone();
    let pos = initial["units"][0]["position"].clone();
    let unit = initial["units"][0]["id"].clone();
    send_json(&mut ws,json!({"type":"command","seq":2,"fencing_token":lease,"command":{"type":"issue_order","units":{"kind":"ids","value":[unit]},"order":"Move","target":[pos[0].as_i64().unwrap()+(3i64<<32),pos[1]]}})).await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(3).await.unwrap();
    assert_eq!(recv_type(&mut ws, "ack").await["ack"]["type"], "order_issued");
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let actual = rynvek_server::transactions::restore_committed(
        server.state.store(),
        server.state.inner.world_snapshots.as_ref(),
        &row,
    )
    .await
    .unwrap();
    assert_ne!(
        serde_json::to_value(actual.observe(rynvek_rts::FactionId(0)).unwrap().units[0].position).unwrap(),
        pos
    );
    drop(ws);
    let reconnect = authored_post(&server, &path, &owner.token, json!({}))
        .await
        .json()
        .unwrap();
    assert_eq!(reconnect["match_id"], run["match_id"]);
    assert_eq!(host.status().state_root, actual.state_root());
    let mut ws = json_connect(&server, &run, &owner.token, 0).await;
    recv_type(&mut ws, "welcome").await;
    recv_type(&mut ws, "observation").await;
    send_json(&mut ws, json!({"type":"acquire_lease","seq":3,"level":"Sovereign"})).await;
    let lease = recv_type(&mut ws, "lease").await["lease"]["fencing_token"].clone();
    let before = host.status();
    send_json(
        &mut ws,
        json!({"type":"command","seq":4,"fencing_token":lease,"command":{"type":"surrender"}}),
    )
    .await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert_eq!(recv_type(&mut ws, "ack").await["ack"]["type"], "surrendered");
    let final_tick = host.status().tick;
    assert!(final_tick == before.tick || final_tick == before.tick + 1);
    assert_ne!(host.status().state_root, before.state_root);
    let row = archived(&server, match_id.parse().unwrap()).await;
    let archive_url = format!("{metadata}/replay");
    assert_eq!(client::get(&archive_url, None).await.unwrap().status, 404);
    assert_eq!(client::get(&archive_url, Some(&other.token)).await.unwrap().status, 404);
    let archive = client::get(&archive_url, Some(&owner.token)).await.unwrap();
    assert_eq!(archive.status, 200);
    assert_eq!(archive.header("cache-control"), Some("no-store"));
    let mut keys = rynvek_security::Keyring::new();
    keys.insert(SERVER_KEY_ID, server_key().public_key()).unwrap();
    let report = rynvek_server::replay::verify(&archive.body, &keys).unwrap();
    assert_eq!(
        report.final_state_root,
        rynvek_server::ids::hex(&host.status().state_root)
    );
    let opened = rynvek_server::replay::open(&archive.body, Some(&keys)).unwrap();
    assert_eq!(
        opened.body.runtime.as_ref().unwrap().initial_revision,
        baseline["world_revision"].as_u64().unwrap()
    );
    assert_eq!(
        rynvek_server::ids::hex(&opened.body.runtime.as_ref().unwrap().initial_root),
        baseline["state_root"].as_str().unwrap()
    );
    assert_eq!(opened.body.commands.len(), 2);
    assert_eq!(opened.body.runtime.as_ref().unwrap().ticks.len(), 4);
    // 即便模拟旧部署误放公共CAS，也不能按公开Hub CID绕开私有回放权限。
    let cid = server.state.inner.blobs.put(&archive.body).unwrap();
    assert_eq!(row.replay_cid_blake3, Some(cid.0));
    assert_eq!(
        client::get(
            &format!("{}/api/v1/hub/blobs/{}", server.base_url(), cid.to_hex()),
            None
        )
        .await
        .unwrap()
        .status,
        404
    );
    let playback_path = format!("/api/v1/matches/{match_id}/playback");
    assert_eq!(
        authored_post(&server, &playback_path, &other.token, json!({"faction":0}))
            .await
            .status,
        404
    );
    let first = authored_post(&server, &playback_path, &owner.token, json!({"faction":0})).await;
    assert_eq!(first.status, 200, "{:?}", first.json());
    assert_eq!(first.header("cache-control"), Some("no-store"));
    let first = first.json().unwrap();
    assert_eq!(first["signature_verified"], true);
    assert_eq!(first["complete_verified"], false);
    let cursor_path = format!("{playback_path}/{}", first["cursor_id"].as_str().unwrap());
    assert_eq!(
        authored_post(
            &server,
            &cursor_path,
            &other.token,
            json!({"tick":final_tick,"max_steps":1})
        )
        .await
        .status,
        404
    );
    assert_eq!(
        authored_post(
            &server,
            &cursor_path,
            &owner.token,
            json!({"tick":final_tick,"max_steps":121})
        )
        .await
        .status,
        400
    );
    let partial = authored_post(
        &server,
        &cursor_path,
        &owner.token,
        json!({"tick":final_tick,"max_steps":1}),
    )
    .await
    .json()
    .unwrap();
    assert_eq!(partial["ready"], false);
    assert_eq!(partial["verified_records"], 1);
    assert_eq!(partial["complete_verified"], false);
    let end = authored_post(
        &server,
        &cursor_path,
        &owner.token,
        json!({"tick":final_tick,"max_steps":120}),
    )
    .await
    .json()
    .unwrap();
    assert_eq!(end["complete_verified"], true, "{end}");
    assert_eq!(end["state_root"], report.final_state_root);
    assert!(end["observation"]["outcome"].is_object()); // 真终局来自重演，不从元数据虚构。
    let rewind = authored_post(
        &server,
        &cursor_path,
        &owner.token,
        json!({"tick":first["initial_tick"]}),
    )
    .await
    .json()
    .unwrap();
    assert_eq!(rewind["tick"], first["initial_tick"]);
    assert_eq!(rewind["verified_records"], 0);
    assert_eq!(rewind["complete_verified"], false);
    let authority_root = host.status().state_root;
    let end_again = authored_post(&server, &cursor_path, &owner.token, json!({"tick":final_tick}))
        .await
        .json()
        .unwrap();
    assert_eq!(end_again["state_root"], end["state_root"]);
    assert_eq!(host.status().state_root, authority_root);
    let replaced = authored_post(&server, &playback_path, &owner.token, json!({"faction":0})).await;
    assert_eq!(replaced.status, 200);
    assert_eq!(
        authored_post(
            &server,
            &cursor_path,
            &owner.token,
            json!({"tick":first["initial_tick"]})
        )
        .await
        .status,
        404
    );
    let closed = client::send_json(
        "DELETE",
        &format!("{}{playback_path}", server.base_url()),
        Some(&owner.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(closed.status, 200);
    // 已签名但日志根被故意改坏：只读重演失败，不得动权威世界。
    let mut bad = opened.body;
    bad.runtime.as_mut().unwrap().ticks[1].state_root[0] ^= 1;
    let bad = rynvek_server::replay::encode(&bad, world_id.0, &server_key()).unwrap();
    let mut bad_row = row.clone();
    bad_row.replay_cid_blake3 = Some(server.state.inner.world_snapshots.put(&bad).unwrap().0);
    server.state.store().update_match(bad_row).await.unwrap();
    let bad_open = authored_post(&server, &playback_path, &owner.token, json!({"faction":0}))
        .await
        .json()
        .unwrap();
    let bad_path = format!("{playback_path}/{}", bad_open["cursor_id"].as_str().unwrap());
    let failed = authored_post(&server, &bad_path, &owner.token, json!({"tick":final_tick})).await;
    assert_ne!(failed.status, 200);
    // 同批先前的好记录也不能偷偷推进游标；只取一条应仍可从原起点成功。
    let retry = authored_post(
        &server,
        &bad_path,
        &owner.token,
        json!({"tick":final_tick,"max_steps":1}),
    )
    .await;
    assert_eq!(retry.status, 200, "{:?}", retry.json());
    assert_eq!(retry.json().unwrap()["verified_records"], 1);
    assert_eq!(host.status().state_root, authority_root);
    drop(ws);
    server.shutdown().await;
}

#[tokio::test]
async fn studio_arena_recovery_keeps_same_match_and_prefence_wal() {
    use rynvek_server::{client, host::TickDriver, store::Store};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let config = rynvek_server::AppConfig {
        public_base_url: String::new(),
        blob_dir: Some(dir.path().to_owned()),
        tick_driver: TickDriver::Manual,
        lobby_timeout: None,
        ..Default::default()
    };
    let start = |app| {
        rynvek_server::start(rynvek_server::ServerConfig {
            http_addr: "127.0.0.1:0".parse().unwrap(),
            webtransport_addr: None,
            app,
            store: store.clone(),
            signer: server_key(),
        })
    };
    let original = start(config.clone()).await.unwrap();
    let owner = register(&original.base_url(), "recover-author", "human", 83).await;
    let created = authored_post(
        &original,
        "/api/v1/worlds",
        &owner.token,
        json!({"title":"持久恢复","template":"arena_pve","map_seed":93,"visibility":"Private"}),
    )
    .await
    .json()
    .unwrap();
    let world = created["world_id"].as_str().unwrap();
    let path = format!("/api/v1/worlds/{world}/arena");
    let run = authored_post(&original, &path, &owner.token, json!({}))
        .await
        .json()
        .unwrap();
    let world_id = world.parse().unwrap();
    let old = original.state.registry().by_world(&world_id).unwrap();
    let mut ws = json_connect(&original, &run, &owner.token, 0).await;
    recv_type(&mut ws, "welcome").await;
    let first = recv_type(&mut ws, "observation").await["observation"].clone();
    send_json(&mut ws, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let lease = recv_type(&mut ws, "lease").await["lease"]["fencing_token"].clone();
    let unit = first["units"][0]["id"].clone();
    let pos = first["units"][0]["position"].clone();
    send_json(&mut ws,json!({"type":"command","seq":2,"fencing_token":lease,"command":{"type":"issue_order","units":{"kind":"ids","value":[unit]},"order":"Move","target":[pos[0].as_i64().unwrap()+(2i64<<32),pos[1]]}})).await;
    old.wait_for(|s| s.pending_inputs == 1).await;
    old.advance(3).await.unwrap();
    recv_type(&mut ws, "ack").await;
    drop(ws);
    let before = original.state.store().get_world(world_id).await.unwrap().unwrap();
    // 新AppState+同一Store/磁盘CAS模拟节点上下文恢复，不声称PG断电或浏览器验收。
    let resumed = start(config).await.unwrap();
    let result = authored_post(&resumed, &path, &owner.token, json!({})).await;
    assert_eq!(result.status, 200, "{:?}", result.json());
    let recovered = result.json().unwrap();
    assert_eq!(recovered["match_id"], run["match_id"]);
    assert_eq!(recovered["recovered"], true);
    let host = resumed.state.registry().by_world(&world_id).unwrap();
    assert_eq!(host.status().state_root, before.latest_state_root);
    assert_eq!(host.status().tick, 3);
    assert_eq!(
        resumed
            .state
            .store()
            .get_world(world_id)
            .await
            .unwrap()
            .unwrap()
            .fencing_token,
        before.fencing_token + 1
    );
    // 旧宿主真实推进会被fence拒绝；其迟到Aborted也不得终止新对局。
    assert!(old.advance(1).await.is_err());
    let mut ws = json_connect(&resumed, &recovered, &owner.token, 0).await;
    recv_type(&mut ws, "welcome").await;
    recv_type(&mut ws, "observation").await;
    send_json(&mut ws, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let lease = recv_type(&mut ws, "lease").await["lease"]["fencing_token"].clone();
    host.advance(2).await.unwrap();
    send_json(
        &mut ws,
        json!({"type":"command","seq":2,"fencing_token":lease,"command":{"type":"surrender"}}),
    )
    .await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    recv_type(&mut ws, "ack").await;
    let match_id = run["match_id"].as_str().unwrap();
    let row = archived(&resumed, match_id.parse().unwrap()).await;
    assert_eq!(row.match_state, rynvek_server::store::MatchState::Completed);
    let archive = client::get(
        &format!("{}/api/v1/matches/{match_id}/replay", resumed.base_url()),
        Some(&owner.token),
    )
    .await
    .unwrap();
    assert_eq!(archive.status, 200);
    let mut keys = rynvek_security::Keyring::new();
    keys.insert(SERVER_KEY_ID, server_key().public_key()).unwrap();
    let verified = rynvek_server::replay::verify(&archive.body, &keys).unwrap();
    assert_eq!(
        verified.final_state_root,
        rynvek_server::ids::hex(&host.status().state_root)
    );
    let body = rynvek_server::replay::open(&archive.body, Some(&keys)).unwrap().body;
    assert_eq!(body.commands.len(), 2, "恢复前Move和恢复后投降均必须存在");
    assert_eq!(body.runtime.as_ref().unwrap().initial_revision, 0);
    assert_eq!(body.runtime.as_ref().unwrap().ticks.len(), 6);
    assert_eq!(
        resumed
            .state
            .store()
            .matches_of(owner.principal_id)
            .await
            .unwrap()
            .len(),
        1
    );
    drop(ws);
    resumed.shutdown().await;
    original.shutdown().await;
}

/// 仅本机两个注册夹具；不联系外部对手、不向真实服务发布对局。
#[tokio::test]
async fn ranked_entry_discovers_real_identity_cancels_and_reconnects_authorized_seat() {
    use rynvek_server::client;
    let server = local_server().await;
    let base = server.base_url();
    let path = format!("{base}/api/v1/matches/queue");
    let alice = register(&base, "rank-entry-a", "human", 101).await;
    let bob = register(&base, "rank-entry-b", "human", 102).await;
    let bot = register(&base, "rank-entry-agent", "agent", 103).await;
    assert_eq!(client::get(&path, None).await.unwrap().status, 401);
    assert_eq!(client::get(&path, Some(&bot.token)).await.unwrap().status, 403);
    let context = client::get(&path, Some(&alice.token)).await.unwrap();
    assert_eq!(context.header("cache-control"), Some("private, no-store"));
    let context = context.json().unwrap();
    assert_eq!(context["principal_id"], alice.principal_id.to_string());
    assert_eq!(context["status"], "idle");
    let req = json!({"league":"human", "ruleset_hash":context["ruleset_hash"]});
    let poll = json!({"poll":true,"league":"human", "ruleset_hash":context["ruleset_hash"]});
    let cancel = json!({"cancel":true,"league":"human", "ruleset_hash":context["ruleset_hash"]});
    assert_eq!(
        client::send_json("POST", &path, Some(&alice.token), &poll)
            .await
            .unwrap()
            .json()
            .unwrap()["status"],
        "not_queued"
    );
    assert_eq!(server.state.matchmaker().len(), 0);
    assert_eq!(
        client::send_json("POST", &path, Some(&alice.token), &req)
            .await
            .unwrap()
            .status,
        202
    );
    assert_eq!(
        client::get(&path, Some(&alice.token)).await.unwrap().json().unwrap()["status"],
        "queued"
    );
    assert_eq!(
        client::send_json("POST", &path, Some(&alice.token), &cancel)
            .await
            .unwrap()
            .json()
            .unwrap()["status"],
        "cancelled"
    );
    client::send_json("POST", &path, Some(&alice.token), &req)
        .await
        .unwrap();
    // 并发的刷新与第二身份入队只发布一个宿主，取消后返回实际已配对席位。
    let (_, b) = tokio::join!(
        client::send_json("POST", &path, Some(&alice.token), &poll),
        client::send_json("POST", &path, Some(&bob.token), &req)
    );
    let b = b.unwrap().json().unwrap();
    assert_eq!(b["principal_id"], bob.principal_id.to_string());
    let a = client::send_json("POST", &path, Some(&alice.token), &cancel)
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(a["status"], "matched");
    assert_eq!(a["match_id"], b["match_id"]);
    assert_eq!(a["mode"], "PvP_Ranked");
    assert_ne!(a["faction"], b["faction"]);
    assert_eq!(server.state.registry().active_count(), 1);
    let host = server
        .state
        .registry()
        .by_world(&a["world_id"].as_str().unwrap().parse().unwrap())
        .unwrap();
    let mut wa = json_connect(&server, &a, &alice.token, a["faction"].as_u64().unwrap() as u32).await;
    assert_eq!(recv_type(&mut wa, "welcome").await["faction"], a["faction"]);
    let mut wb = json_connect(&server, &b, &bob.token, b["faction"].as_u64().unwrap() as u32).await;
    assert_eq!(recv_type(&mut wb, "welcome").await["faction"], b["faction"]);
    host.wait_for(|s| s.state == rynvek_server::store::MatchState::Running)
        .await;
    let obs_b = recv_type(&mut wb, "observation").await;
    let enemy = obs_b["observation"]["units"][0]["id"].clone();
    send_json(&mut wa, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let fence = recv_type(&mut wa, "lease").await["lease"]["fencing_token"].clone();
    send_json(&mut wa,json!({"type":"command","seq":2,"fencing_token":fence,"command":{"type":"issue_order","units":{"kind":"ids","value":[enemy]},"order":"Move","target":[0,0]}})).await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert!(
        recv_type(&mut wa, "ack").await["error"].is_object(),
        "不得指挥另一真实席位的单位"
    );
    // 同身份重连取代旧连接；浏览器不保存旧租约，先由服务器重新确认。
    // 原租约按既有协议可保留到过期；不把同身份续租伪称撤销。
    let mut again = json_connect(&server, &a, &alice.token, a["faction"].as_u64().unwrap() as u32).await;
    recv_type(&mut again, "welcome").await;
    send_json(
        &mut again,
        json!({"type":"command","seq":3,"fencing_token":0,"command":{"type":"surrender"}}),
    )
    .await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert!(recv_type(&mut again, "ack").await["error"].is_object());
    send_json(&mut again, json!({"type":"acquire_lease","seq":4,"level":"Sovereign"})).await;
    let new_fence = recv_type(&mut again, "lease").await["lease"]["fencing_token"].clone();
    send_json(
        &mut again,
        json!({"type":"release_lease","seq":6,"fencing_token":new_fence}),
    )
    .await;
    recv_type(&mut again, "lease").await;
    send_json(&mut again, json!({"type":"acquire_lease","seq":7,"level":"Sovereign"})).await;
    let latest = recv_type(&mut again, "lease").await["lease"]["fencing_token"].clone();
    assert_ne!(latest, new_fence);
    send_json(
        &mut again,
        json!({"type":"command","seq":8,"fencing_token":new_fence,"command":{"type":"surrender"}}),
    )
    .await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert!(recv_type(&mut again, "ack").await["error"].is_object());
    let new_fence = latest;
    send_json(
        &mut again,
        json!({"type":"command","seq":5,"fencing_token":new_fence,"command":{"type":"surrender"}}),
    )
    .await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert_eq!(recv_type(&mut again, "ack").await["ack"]["type"], "surrendered");
    host.wait_for(|s| s.state.is_terminal()).await;
    assert_eq!(
        client::send_json("POST", &path, Some(&alice.token), &poll)
            .await
            .unwrap()
            .json()
            .unwrap()["status"],
        "not_queued"
    );
    assert_eq!(server.state.matchmaker().len(), 0);
    server.shutdown().await;
}

#[tokio::test]
async fn private_observer_admission_revoke_leave_and_node_recovery() {
    use rynvek_server::{AppConfig, ServerConfig, host::TickDriver, store::Store, client};
    let dir=tempfile::tempdir().unwrap();let store=Store::memory();
    let config=AppConfig{public_base_url:String::new(),blob_dir:Some(dir.path().to_owned()),tick_driver:TickDriver::Manual,lobby_timeout:None,..Default::default()};
    let start=|app|rynvek_server::start(ServerConfig{http_addr:"127.0.0.1:0".parse().unwrap(),webtransport_addr:None,app,store:store.clone(),signer:server_key()});
    let server=start(config.clone()).await.unwrap();let base=server.base_url();
    let owner=register(&base,"observer-owner","human",101).await;
    let viewer=register(&base,"observer-viewer","human",102).await;
    let outsider=register(&base,"observer-outsider","human",103).await;
    let created=authored_post(&server,"/api/v1/worlds",&owner.token,json!({"title":"私有阵营观战","template":"arena_pve","visibility":"Private"})).await.json().unwrap();
    let world=created["world_id"].as_str().unwrap();let id=world.parse().unwrap();
    let arena_path=format!("/api/v1/worlds/{world}/arena");let grants_path=format!("/api/v1/worlds/{world}/observers");let observe_path=format!("/api/v1/worlds/{world}/observation");
    let get=|server:&rynvek_server::RunningServer,path:&str,token:&str|{let url=format!("{}{path}",server.base_url());let token=token.to_owned();async move {client::get(&url,Some(&token)).await.unwrap()}};
    assert_eq!(get(&server,&observe_path,&viewer.token).await.status,404);
    let run=authored_post(&server,&arena_path,&owner.token,json!({})).await.json().unwrap();
    let host=server.state.registry().by_world(&id).unwrap();let before=host.status();
    assert_eq!(authored_post(&server,&grants_path,&outsider.token,json!({"principal_id":viewer.principal_id.to_string(),"faction":0})).await.status,404);
    assert_eq!(authored_post(&server,&grants_path,&owner.token,json!({"principal_id":viewer.principal_id.to_string(),"faction":1})).await.status,403);
    let granted=authored_post(&server,&grants_path,&owner.token,json!({"principal_id":viewer.principal_id.to_string(),"faction":0})).await;
    assert_eq!(granted.status,200,"{:?}",granted.json());let grant=granted.json().unwrap();
    let first=get(&server,&format!("{observe_path}?faction=1"),&viewer.token).await;
    assert_eq!(first.status,200);assert_eq!(first.header("cache-control"),Some("private, no-store"));assert_eq!(first.header("vary"),Some("Authorization"));
    let frame=first.json().unwrap();assert_eq!(get(&server,&observe_path,&outsider.token).await.status,404);assert_eq!(frame["role"],"observer");assert_eq!(frame["faction"],0);assert_eq!(frame["messages"]["observation"]["observation"]["faction"],0);
    assert_eq!(frame["messages"]["welcome"]["tools"],json!([]));
    assert_eq!(host.status(),before,"观战不能入座、取得租约或自动开局");
    for suffix in ["authoring","commits","observers"] {assert_eq!(get(&server,&format!("/api/v1/worlds/{world}/{suffix}"),&viewer.token).await.status,404);}
    assert_eq!(authored_post(&server,&arena_path,&viewer.token,json!({})).await.status,404);
    assert_eq!(get(&server,&format!("/api/v1/matches/{}/replay",run["match_id"].as_str().unwrap()),&viewer.token).await.status,404);
    let mut bad=json_connect(&server,&run,&viewer.token,0).await;recv_type(&mut bad,"error").await;drop(bad);
    assert_eq!(host.status().connections,0,"观战准入绝不转成WS控制席位");
    let mut ws=json_connect(&server,&run,&owner.token,0).await;recv_type(&mut ws,"welcome").await;recv_type(&mut ws,"observation").await;
    host.advance(3).await.unwrap();let row=server.state.store().get_world(id).await.unwrap().unwrap();
    let runtime=rynvek_server::transactions::restore_committed(server.state.store(),server.state.inner.world_snapshots.as_ref(),&row).await.unwrap();
    let projected=get(&server,&observe_path,&viewer.token).await.json().unwrap();
    assert_eq!(projected["messages"]["observation"]["observation"],json!(runtime.world().observe(rynvek_rts::FactionId(0)).unwrap()),"必须是原fog投影，不是全世界快照");
    let cid=server.state.store().list_commits(id).await.unwrap()[0].snapshot_cid.unwrap();
    assert_eq!(get(&server,&format!("/api/v1/hub/blobs/{}",rynvek_server::ids::hex(&cid)),&viewer.token).await.status,404);
    assert!(projected.get("state_root").is_none());assert!(projected.get("snapshot_cid").is_none());
    // 下方通过新节点同磁盘CAS读取原Actor；不声称真实PG/断电验收。
    let resumed=start(config).await.unwrap();
    assert_eq!(get(&resumed,&observe_path,&viewer.token).await.status,409,"观战不能自行恢复宿主");
    let persistent=get(&resumed,&grants_path,&owner.token).await.json().unwrap();assert_eq!(persistent["grants"][0],grant);
    let recovered=authored_post(&resumed,&arena_path,&owner.token,json!({})).await.json().unwrap();assert_eq!(recovered["match_id"],run["match_id"]);assert_eq!(recovered["recovered"],true);
    let second=get(&resumed,&observe_path,&viewer.token).await.json().unwrap();assert_eq!(second["grant_id"],grant["grant_id"]);assert_eq!(second["messages"]["welcome"]["tick"],3);
    let revoke=format!("{}{grants_path}/{}",resumed.base_url(),viewer.principal_id);
    assert_eq!(client::send_json("DELETE",&revoke,Some(&owner.token),&json!({"grant_id":grant["grant_id"]})).await.unwrap().status,200);
    assert_eq!(get(&resumed,&observe_path,&viewer.token).await.status,404);
    let newer=authored_post(&resumed,&grants_path,&owner.token,json!({"principal_id":viewer.principal_id.to_string(),"faction":0})).await.json().unwrap();assert_ne!(newer["grant_id"],grant["grant_id"]);
    assert_eq!(client::send_json("DELETE",&revoke,Some(&owner.token),&json!({"grant_id":grant["grant_id"]})).await.unwrap().status,409,"旧版本撤销不得删掉新授权");
    let leave=format!("{}{observe_path}",resumed.base_url());assert_eq!(client::send_json("DELETE",&leave,Some(&viewer.token),&json!({"grant_id":newer["grant_id"]})).await.unwrap().status,200);
    assert_eq!(get(&resumed,&observe_path,&viewer.token).await.status,404);
    assert!(resumed.state.store().observer_grants(id).await.unwrap().is_empty());
    drop(runtime);drop(ws);host.abort("观战定向测试完成").await;resumed.state.registry().by_world(&id).unwrap().abort("观战恢复完成").await;
    server.shutdown().await;resumed.shutdown().await;
}

#[tokio::test]
async fn private_custom_pvp_full_seats_fog_reconnect_recovery_and_leave() {
    use rynvek_server::{AppConfig,ServerConfig,host::TickDriver,store::{Store,MatchState},client};
    let dir=tempfile::tempdir().unwrap();let store=Store::memory();let config=AppConfig{public_base_url:String::new(),blob_dir:Some(dir.path().to_owned()),tick_driver:TickDriver::Manual,lobby_timeout:None,..Default::default()};
    let start=|app|rynvek_server::start(ServerConfig{http_addr:"127.0.0.1:0".parse().unwrap(),webtransport_addr:None,app,store:store.clone(),signer:server_key()});
    let server=start(config.clone()).await.unwrap();let base=server.base_url();let owner=register(&base,"custom-owner","human",111).await;let guest=register(&base,"custom-player","human",112).await;let viewer=register(&base,"custom-outsider","human",113).await;
    let created=authored_post(&server,"/api/v1/worlds",&owner.token,json!({"title":"真实私有双人","template":"arena_duel","visibility":"Private","map_seed":42})).await.json().unwrap();let world=created["world_id"].as_str().unwrap();let id=world.parse().unwrap();let path=format!("/api/v1/worlds/{world}/multiplayer");
    let get=|s:&rynvek_server::RunningServer,p:&str,t:&str|{let url=format!("{}{p}",s.base_url());let token=t.to_owned();async move{client::get(&url,Some(&token)).await.unwrap()}};
    assert_eq!(get(&server,&path,&guest.token).await.status,404);
    let invitation=authored_post(&server,&path,&owner.token,json!({"action":"invite","guest_id":guest.principal_id.to_string()})).await;assert_eq!(invitation.status,200,"{:?}",invitation.json());let invitation=invitation.json().unwrap();
    assert_eq!(get(&server,&path,&viewer.token).await.status,404);
    assert_eq!(authored_post(&server,&path,&guest.token,json!({"action":"connect","version":invitation["version"]})).await.status,409);
    assert_eq!(authored_post(&server,&path,&owner.token,json!({"action":"accept","version":invitation["version"]})).await.status,403);
    let accepted=authored_post(&server,&path,&guest.token,json!({"action":"accept","version":invitation["version"]})).await.json().unwrap();assert_eq!(accepted["status"],"ready");
    assert_eq!(authored_post(&server,&path,&owner.token,json!({"action":"connect","version":invitation["version"]})).await.status,409);
    let opened=authored_post(&server,&path,&owner.token,json!({"action":"connect","version":accepted["version"]})).await;assert_eq!(opened.status,200,"{:?}",opened.json());let opened=opened.json().unwrap();let run=opened["seat"].clone();assert_eq!(run["custom"],true);
    let host=server.state.registry().by_world(&id).unwrap();assert_eq!(host.seats.len(),2);
    let mut a=json_connect(&server,&run,&owner.token,0).await;recv_type(&mut a,"welcome").await;let own_obs=recv_type(&mut a,"observation").await["observation"].clone();assert_eq!(host.status().state,MatchState::Lobby);
    send_json(&mut a,json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;recv_type(&mut a,"lease").await;
    let mut b=json_connect(&server,&run,&guest.token,1).await;let welcome=recv_type(&mut b,"welcome").await;assert_eq!(welcome["faction"],1);let obs=recv_type(&mut b,"observation").await["observation"].clone();assert_eq!(host.status().state,MatchState::Running);
    assert!(own_obs["units"].as_array().unwrap().iter().all(|u|u["faction"]==0));assert!(obs["units"].as_array().unwrap().iter().all(|u|u["faction"]==1),"远处对手必须经过原fog裁剪");
    let mut wrong=json_connect(&server,&run,&guest.token,0).await;recv_type(&mut wrong,"error").await;drop(wrong);assert_eq!(host.status().connections,2);
    for suffix in ["authoring","commits","observers"]{assert_eq!(get(&server,&format!("/api/v1/worlds/{world}/{suffix}"),&guest.token).await.status,404);}
    let observer_path=format!("/api/v1/worlds/{world}/observers");
    assert_eq!(authored_post(&server,&observer_path,&owner.token,json!({"principal_id":viewer.principal_id.to_string(),"faction":0})).await.status,200);
    let projection=get(&server,&format!("/api/v1/worlds/{world}/observation"),&viewer.token).await.json().unwrap();assert_eq!(projection["faction"],0);
    let mut forbidden=json_connect(&server,&run,&viewer.token,0).await;recv_type(&mut forbidden,"error").await;drop(forbidden);
    send_json(&mut b,json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;let lease=recv_type(&mut b,"lease").await["lease"]["fencing_token"].clone();let unit=obs["units"][0]["id"].clone();let pos=obs["units"][0]["position"].clone();
    send_json(&mut b,json!({"type":"command","seq":2,"fencing_token":lease,"command":{"type":"issue_order","units":{"kind":"ids","value":[unit]},"order":"Move","target":[pos[0].as_i64().unwrap()+(2i64<<32),pos[1]]}})).await;
    host.wait_for(|s|s.pending_inputs==1).await;host.advance(3).await.unwrap();let ack=recv_type(&mut b,"ack").await;assert!(ack["error"].is_null());let before=server.state.store().get_world(id).await.unwrap().unwrap();
    let resumed=start(config).await.unwrap();let recovered=authored_post(&resumed,&path,&guest.token,json!({"action":"connect","version":opened["version"]})).await;assert_eq!(recovered.status,200,"{:?}",recovered.json());let recovered=recovered.json().unwrap();assert_eq!(recovered["seat"]["match_id"],run["match_id"]);assert_eq!(recovered["seat"]["faction"],1);assert_eq!(recovered["seat"]["recovered"],true);
    let current=resumed.state.registry().by_world(&id).unwrap();assert_eq!(current.status().state_root,before.latest_state_root);assert_eq!(current.status().tick,3);assert!(host.advance(1).await.is_err());
    let mut a2=json_connect(&resumed,&run,&owner.token,0).await;recv_type(&mut a2,"welcome").await;recv_type(&mut a2,"observation").await;
    let mut b2=json_connect(&resumed,&run,&guest.token,1).await;recv_type(&mut b2,"welcome").await;let continued=recv_type(&mut b2,"observation").await;assert_ne!(continued["observation"]["units"][0]["position"],pos,"原Actor命令必须改变实际位置并从WAL恢复");
    send_json(&mut b2,json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;let fence=recv_type(&mut b2,"lease").await["lease"]["fencing_token"].clone();
    send_json(&mut b2,json!({"type":"command","seq":2,"fencing_token":fence,"command":{"type":"issue_order","units":{"kind":"ids","value":[unit]},"order":"Move","target":pos}})).await;current.wait_for(|s|s.pending_inputs==1).await;
    let root=current.status().state_root;let closed=authored_post(&resumed,&path,&guest.token,json!({"action":"close","version":opened["version"]})).await;assert_eq!(closed.status,200,"{:?}",closed.json());assert_eq!(closed.json().unwrap()["status"],"closed");assert_eq!(current.status().state,MatchState::Aborted);assert_eq!(current.status().connections,0);assert_eq!(current.status().pending_inputs,0);assert_eq!(current.status().state_root,root);assert!(current.advance(1).await.is_err());
    let mut denied=json_connect(&resumed,&run,&guest.token,1).await;recv_type(&mut denied,"error").await;drop(denied);
    let row=resumed.state.store().get_match(run["match_id"].as_str().unwrap().parse().unwrap()).await.unwrap().unwrap().0;assert_eq!(row.match_state,MatchState::Aborted);assert!(row.winner_faction_id.is_none());
    assert_eq!(resumed.state.store().get_rating(guest.principal_id,rynvek_security::League::Human).await.unwrap().matches_played,0);
    drop(a);drop(b);drop(a2);drop(b2);server.shutdown().await;resumed.shutdown().await;
}

#[tokio::test]
async fn custom_owner_cancel_pending_and_completed_match_remains_private_unrated() {
    use rynvek_server::{client,store::MatchState};
    let server=start_server(false).await;let base=server.base_url();let owner=register(&base,"custom-end-owner","human",121).await;let guest=register(&base,"custom-end-player","human",122).await;
    for cancel_before_accept in [true,false] {
        let made=authored_post(&server,"/api/v1/worlds",&owner.token,json!({"title":"私有席位生命周期","template":"arena_duel","visibility":"Private"})).await.json().unwrap();let world=made["world_id"].as_str().unwrap();let id=world.parse().unwrap();let path=format!("/api/v1/worlds/{world}/multiplayer");
        let invited=authored_post(&server,&path,&owner.token,json!({"action":"invite","guest_id":guest.principal_id.to_string()})).await.json().unwrap();
        if cancel_before_accept {
            let before=server.state.store().get_world(id).await.unwrap().unwrap();
            let ended=authored_post(&server,&path,&owner.token,json!({"action":"close","version":invited["version"]})).await.json().unwrap();assert_eq!(ended["status"],"closed");
            assert_eq!(authored_post(&server,&path,&guest.token,json!({"action":"accept","version":invited["version"]})).await.status,409);
            assert!(server.state.registry().by_world(&id).is_none());let after=server.state.store().get_world(id).await.unwrap().unwrap();assert_eq!(before.latest_state_root,after.latest_state_root);assert_eq!(before.fencing_token+1,after.fencing_token);
            continue;
        }
        let accepted=authored_post(&server,&path,&guest.token,json!({"action":"accept","version":invited["version"]})).await.json().unwrap();let opened=authored_post(&server,&path,&owner.token,json!({"action":"connect","version":accepted["version"]})).await.json().unwrap();let run=opened["seat"].clone();let host=server.state.registry().by_world(&id).unwrap();
        let mut a=json_connect(&server,&run,&owner.token,0).await;recv_type(&mut a,"welcome").await;recv_type(&mut a,"observation").await;let mut b=json_connect(&server,&run,&guest.token,1).await;recv_type(&mut b,"welcome").await;recv_type(&mut b,"observation").await;
        send_json(&mut a,json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;let fence=recv_type(&mut a,"lease").await["lease"]["fencing_token"].clone();send_json(&mut a,json!({"type":"command","seq":2,"fencing_token":fence,"command":{"type":"surrender"}})).await;host.wait_for(|s|s.pending_inputs==1).await;host.advance(1).await.unwrap();recv_type(&mut a,"ack").await;
        let match_id=run["match_id"].as_str().unwrap();let completed=archived(&server,match_id.parse().unwrap()).await;assert_eq!(completed.match_state,MatchState::Completed);assert_eq!(completed.winner_faction_id,Some(1));
        assert_eq!(server.state.store().get_rating(owner.principal_id,rynvek_security::League::Human).await.unwrap().matches_played,0);assert_eq!(server.state.store().get_rating(guest.principal_id,rynvek_security::League::Human).await.unwrap().matches_played,0);
        let replay=format!("{base}/api/v1/matches/{match_id}/replay");assert_eq!(client::get(&replay,Some(&guest.token)).await.unwrap().status,404);assert_eq!(client::get(&replay,Some(&owner.token)).await.unwrap().status,200);
        let closed=authored_post(&server,&path,&owner.token,json!({"action":"close","version":opened["version"]})).await;assert_eq!(closed.status,200);let preserved=server.state.store().get_match(match_id.parse().unwrap()).await.unwrap().unwrap().0;assert_eq!(preserved.match_state,MatchState::Completed);assert_eq!(preserved.winner_faction_id,Some(1));
        drop(a);drop(b);
    }
    server.shutdown().await;
}
