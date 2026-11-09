// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 端到端：匹配 → WebSocket / WebTransport 入场 → 租约与投降 → 终局签名回放下载与验证 → Glicko-2 天梯更新；
//! 以及握手拒绝、迷雾不泄漏、回放篡改检测。

mod common;

use common::*;
use rynvek_network::StatePacket;
use rynvek_network::control::ControlBody;
use rynvek_network::link::Link;
use rynvek_protocol::{WireChannelId, WireUuid};
use rynvek_security::Keyring;
use rynvek_server::client;
use rynvek_server::replay::{self, ReplayError};
use rynvek_server::store::MatchState;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transport {
    WebSocket,
    WebTransport,
}

struct Matched {
    match_id: String,
    world_id: WireUuid,
    faction: u32,
    ws_url: String,
    wt: Option<(String, [u8; 32])>,
}

async fn queue(base: &str, user: &User) -> (u16, Value) {
    let r = client::send_json(
        "POST",
        &format!("{base}/api/v1/matches/queue"),
        Some(&user.token),
        &json!({ "league": "human", "ruleset_hash": format!("blake3:{}", ruleset_hex()) }),
    )
    .await
    .unwrap();
    (r.status, r.json().unwrap())
}

fn matched(v: &Value) -> Matched {
    assert_eq!(v["status"], "matched", "{v}");
    let wt = v["stream"]["webtransport"].as_object().map(|o| {
        (
            o["url"].as_str().unwrap().to_owned(),
            rynvek_server::ids::parse_hex::<32>(o["certificate_sha256"].as_str().unwrap()).unwrap(),
        )
    });
    Matched {
        match_id: v["match_id"].as_str().unwrap().to_owned(),
        world_id: v["world_id"].as_str().unwrap().parse().unwrap(),
        faction: v["faction"].as_u64().unwrap() as u32,
        ws_url: v["stream"]["websocket"].as_str().unwrap().to_owned(),
        wt,
    }
}

/// 打开链路（WebTransport 的会话对象需要随客户端存活）
async fn open(t: Transport, m: &Matched) -> (Link, Option<Box<dyn std::any::Any + Send>>) {
    match t {
        Transport::WebSocket => (rynvek_network::websocket::connect(&m.ws_url).await.unwrap(), None),
        Transport::WebTransport => {
            let (url, pin) = m.wt.clone().expect("服务端已启用 WebTransport");
            let s = rynvek_network::webtransport::connect(&url, Some(pin)).await.unwrap();
            let rynvek_network::webtransport::ClientSession { link, connection, .. } = s;
            (link, Some(Box::new(connection)))
        }
    }
}

async fn full_match(t: Transport) {
    let server = start_server(t == Transport::WebTransport).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let bob = register(&base, "bob", "human", 2).await;

    let (status, v) = queue(&base, &alice).await;
    assert_eq!(status, 202, "{v}");
    assert_eq!(v["status"], "queued");
    let (_, v) = queue(&base, &bob).await;
    let mb = matched(&v);
    // 幂等：再次排队得到同一对局
    let (_, v) = queue(&base, &alice).await;
    let ma = matched(&v);
    assert_eq!(ma.match_id, mb.match_id);
    assert_ne!(ma.faction, mb.faction);

    let handle = server.state.registry().by_world(&ma.world_id).unwrap();
    let (la, _ka) = open(t, &ma).await;
    let (lb, _kb) = open(t, &mb).await;
    let mut a = StreamClient::new(la, alice.principal_id);
    let mut b = StreamClient::new(lb, bob.principal_id);
    a.hello(alice.token_bytes(), ma.world_id, ma.faction).await;
    b.hello(bob.token_bytes(), mb.world_id, mb.faction).await;

    // 入场三件套：ServerHello / Welcome / 关键帧
    for (c, faction) in [(&mut a, ma.faction), (&mut b, mb.faction)] {
        let f = c.recv_on(WireChannelId::Control, |_| {}).await;
        let ControlBody::Welcome(hello) = control_body(&f) else {
            panic!("应为 ServerHello")
        };
        assert_eq!(hello.faction, faction);
        assert_eq!(hello.resume_token.len(), 32);
        let f = c.recv_on(WireChannelId::Command, |_| {}).await;
        let v: Value = serde_json::from_slice(&f.decoded_payload().unwrap()).unwrap();
        assert_eq!(v["message"]["type"], "welcome", "{v}");
        let f = c.recv_on(WireChannelId::StateStream, |_| {}).await;
        let packet = StatePacket::decode(&f.decoded_payload().unwrap()).unwrap();
        assert!(packet.keyframe);
        assert_eq!(u32::from(packet.faction), faction);
        // 开局双方基地相距很远：关键帧里不能出现任何敌方实体（物理裁剪）
        assert!(
            packet
                .upserts
                .iter()
                .all(|e| u32::from(e.faction) == faction || e.faction == u8::MAX),
            "开局关键帧泄漏了敌方实体"
        );
    }
    let st = handle.wait_for(|s| s.state == MatchState::Running).await;
    assert_eq!(st.connections, 2);

    // 客户端推进 Tick 被拒绝
    let r = a.command(0, json!({ "type": "advance", "seq": 1, "ticks": 5 })).await;
    assert_eq!(r["type"], "error");
    assert_eq!(r["error"]["code"], 0x8001);

    // 推进若干 Tick：两端都收到 StateStream
    handle.advance(30).await.unwrap();
    let f = b.recv_on(WireChannelId::StateStream, |_| {}).await;
    assert!(f.header.tick_low32 > 0);

    // Alice 获取最高租约并投降
    let tick = handle.status().tick;
    let r = a
        .command(tick, json!({ "type": "acquire_lease", "seq": 2, "level": "Sovereign" }))
        .await;
    assert_eq!(r["type"], "lease", "{r}");
    let token = r["lease"]["fencing_token"].as_u64().unwrap();
    let r = a
        .command(
            tick,
            json!({ "type": "command", "seq": 3, "fencing_token": token, "command": { "type": "surrender" } }),
        )
        .await;
    assert_eq!(r["type"], "ack", "{r}");
    assert!(r.get("error").is_none(), "{r}");
    for _ in 0..10 {
        if handle.status().state == MatchState::Completed {
            break;
        }
        let _ = handle.advance(1).await;
    }
    handle.wait_for(|s| s.state == MatchState::Completed).await;

    // 持久化是异步的：轮询对局详情直到落库
    let detail = loop {
        let v = client::get(&format!("{base}/api/v1/matches/{}", ma.match_id), Some(&alice.token))
            .await
            .unwrap()
            .json()
            .unwrap();
        if v["match_state"] == "Completed" && !v["replay_cid_blake3"].is_null() {
            break v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(detail["winner_faction_id"], mb.faction, "{detail}");
    let parts = detail["participants"].as_array().unwrap();
    let bob_row = parts
        .iter()
        .find(|p| p["principal_id"] == bob.principal_id.to_string())
        .unwrap();
    assert_eq!(bob_row["handle"], "bob");
    assert!(bob_row["rating_delta"].as_f64().unwrap() > 0.0, "{detail}");
    assert_eq!(detail["win_reason"], "Surrender");

    // 下载并独立验证回放
    let r = client::get(
        &format!("{base}/api/v1/matches/{}/replay", ma.match_id),
        Some(&bob.token),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("application/vnd.rynvek.rreplay"));
    assert_eq!(&r.body[..4], b"RYRP");
    let mut ring = Keyring::new();
    ring.insert(SERVER_KEY_ID, server_key().public_key()).unwrap();
    let report = replay::verify(&r.body, &ring).unwrap();
    assert_eq!(report.match_id, ma.match_id);
    assert!(report.commands >= 1);

    // 天梯：胜者涨分、败者掉分，偏差收缩
    let ranks = client::get(&format!("{base}/api/v1/rankings/human"), None)
        .await
        .unwrap()
        .json()
        .unwrap();
    let rows = ranks["entries"].as_array().unwrap();
    let find = |pid: u64| {
        rows.iter()
            .find(|r| r["principal_id"] == pid.to_string())
            .unwrap()
            .clone()
    };
    let (ra, rb) = (find(alice.principal_id), find(bob.principal_id));
    assert!(rb["rating"].as_f64().unwrap() > 1500.0 && ra["rating"].as_f64().unwrap() < 1500.0);
    assert!(rb["deviation"].as_f64().unwrap() < 350.0);
    assert_eq!(rows[0]["principal_id"], bob.principal_id.to_string());
    server.shutdown().await;
}

#[tokio::test]
async fn websocket_match_end_to_end() {
    full_match(Transport::WebSocket).await;
}

#[tokio::test]
async fn webtransport_match_end_to_end() {
    full_match(Transport::WebTransport).await;
}

#[tokio::test]
async fn handshake_rejects_bad_tokens_and_strangers() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let bob = register(&base, "bob", "human", 2).await;
    let eve = register(&base, "eve", "human", 3).await;
    queue(&base, &alice).await;
    let m = matched(&queue(&base, &bob).await.1);

    let expect_disconnect = |code: u32| {
        move |c: ControlBody| match c {
            ControlBody::Disconnect(d) => assert_eq!(d.error_code, code, "{}", d.reason),
            other => panic!("应为 Disconnect：{other:?}"),
        }
    };
    // 伪造令牌
    let mut c = StreamClient::new(rynvek_network::websocket::connect(&m.ws_url).await.unwrap(), 0);
    let mut bad = alice.token_bytes();
    bad[50] ^= 0xFF;
    c.hello(bad, m.world_id, m.faction).await;
    expect_disconnect(0x8001)(control_body(&c.recv().await.unwrap()));
    assert!(c.recv().await.is_none(), "拒绝后应关闭连接");
    // 非参赛者
    let mut c = StreamClient::new(rynvek_network::websocket::connect(&m.ws_url).await.unwrap(), 0);
    c.hello(eve.token_bytes(), m.world_id, 0).await;
    expect_disconnect(0x8001)(control_body(&c.recv().await.unwrap()));
    // 冒充对手阵营
    let other = if m.faction == 0 { 1 } else { 0 };
    let mut c = StreamClient::new(rynvek_network::websocket::connect(&m.ws_url).await.unwrap(), 0);
    c.hello(bob.token_bytes(), m.world_id, other).await;
    expect_disconnect(0x8001)(control_body(&c.recv().await.unwrap()));
    // 首帧不在 Control 通道
    let mut c = StreamClient::new(rynvek_network::websocket::connect(&m.ws_url).await.unwrap(), 0);
    c.send(WireChannelId::Command, b"{}").await;
    expect_disconnect(0x8001)(control_body(&c.recv().await.unwrap()));
    server.shutdown().await;
}

#[tokio::test]
async fn tampered_replays_are_rejected_with_stable_codes() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let bob = register(&base, "bob", "human", 2).await;
    queue(&base, &alice).await;
    let m = matched(&queue(&base, &bob).await.1);
    let handle = server.state.registry().by_world(&m.world_id).unwrap();
    // 无人入场时手动终止对局也会产出回放
    handle.abort("测试终止").await;
    let v = loop {
        let v = client::get(&format!("{base}/api/v1/matches/{}", m.match_id), Some(&alice.token))
            .await
            .unwrap()
            .json()
            .unwrap();
        if v["match_state"] == "Aborted" || !v["replay_cid_blake3"].is_null() {
            break v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    // Aborted 对局不提供回放：按规范只有 Completed 才可下载
    if v["match_state"] == "Aborted" {
        let r = client::get(
            &format!("{base}/api/v1/matches/{}/replay", m.match_id),
            Some(&alice.token),
        )
        .await
        .unwrap();
        assert_eq!(r.error_code(), Some(0x2002));
    }

    // 直接用库函数录制一局并验证各类篡改
    let config = rynvek_rts::MatchConfig::duel(42);
    let mut world = rynvek_rts::ArenaWorld::new(config);
    world.enable_command_log();
    for _ in 0..620 {
        world.step();
    }
    let body = replay::record(&world, "m".into(), "w".into(), "rs".into(), Vec::new()).unwrap();
    let signer = server_key();
    let bytes = replay::encode(&body, [3; 16], &signer).unwrap();
    let mut ring = Keyring::new();
    ring.insert(SERVER_KEY_ID, signer.public_key()).unwrap();
    let ok = replay::verify(&bytes, &ring).unwrap();
    assert_eq!(ok.final_tick, 620);
    let ticks: Vec<u64> = body.checkpoints.iter().map(|c| c.tick).collect();
    assert!(ticks.contains(&300) && ticks.contains(&600), "{ticks:?}");
    assert_eq!(ok.checkpoints_verified, body.checkpoints.len());

    let code = |r: Result<replay::VerifyReport, ReplayError>| r.unwrap_err().code.code();
    // 魔数 / 截断 → 0x7001
    let mut b = bytes.clone();
    b[0] = b'X';
    assert_eq!(code(replay::verify(&b, &ring)), 0x7001);
    assert_eq!(code(replay::verify(&bytes[..100], &ring)), 0x7001);
    // 负载被改 → 0x7002
    let mut b = bytes.clone();
    b[80] ^= 1;
    assert_eq!(code(replay::verify(&b, &ring)), 0x7002);
    // 签名被改 / 未知签名者 → 0x9001
    let mut b = bytes.clone();
    let n = b.len();
    b[n - 1] ^= 1;
    assert_eq!(code(replay::verify(&b, &ring)), 0x9001);
    assert_eq!(code(replay::verify(&bytes, &Keyring::new())), 0x9001);
    // 合法签名但内容与重新仿真不符 → 0x3003
    let mut forged = body.clone();
    forged.final_state_root = "00".repeat(32);
    let b = replay::encode(&forged, [3; 16], &signer).unwrap();
    assert_eq!(code(replay::verify(&b, &ring)), 0x3003);
    let mut forged = body.clone();
    forged.checkpoints.last_mut().unwrap().state_root = "11".repeat(32);
    let b = replay::encode(&forged, [3; 16], &signer).unwrap();
    assert_eq!(code(replay::verify(&b, &ring)), 0x3003);
    server.shutdown().await;
}
