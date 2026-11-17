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
