// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 端到端：匹配 → WebSocket / WebTransport 入场 → 租约与投降 → 终局签名回放下载与验证 → Glicko-2 天梯更新；
//! 以及握手拒绝、迷雾不泄漏、回放篡改检测。

mod common;

use common::arena::*;
use common::*;
use rynvek_network::control::ControlBody;
use rynvek_protocol::WireChannelId;
use rynvek_security::Keyring;
use rynvek_server::client;
use rynvek_server::replay::{self, ReplayError};
use rynvek_server::store::Store;

#[tokio::test]
async fn websocket_match_end_to_end() {
    full_match(Transport::WebSocket, Store::memory()).await;
}

#[tokio::test]
async fn webtransport_match_end_to_end() {
    full_match(Transport::WebTransport, Store::memory()).await;
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
