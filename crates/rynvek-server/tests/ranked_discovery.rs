// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 真实回环HTTP/双WS + 同一MemoryStore/磁盘CAS；不是PG实库、多机灾备、掉电或浏览器硬件测试。
mod common;
use common::*;
use futures_util::{SinkExt, StreamExt};
use rynvek_server::{
    client,
    host::TickDriver,
    ranked::*,
    store::{MatchState, Store},
};
use serde_json::{Value, json};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn start(store: Store, path: &std::path::Path) -> rynvek_server::RunningServer {
    rynvek_server::start(rynvek_server::ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        store,
        signer: server_key(),
        app: rynvek_server::AppConfig {
            public_base_url: String::new(),
            blob_dir: Some(path.to_owned()),
            tick_driver: TickDriver::Manual,
            lobby_timeout: None,
            fixed_map_seed: Some(42),
            ..Default::default()
        },
    })
    .await
    .unwrap()
}
async fn context(server: &rynvek_server::RunningServer, user: &User) -> Value {
    let r = client::get(
        &format!("{}/api/v1/matches/queue", server.base_url()),
        Some(&user.token),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
async fn connect(server: &rynvek_server::RunningServer, user: &User, id: &str) -> Value {
    let r = client::send_json(
        "POST",
        &format!("{}/api/v1/matches/{id}/connect", server.base_url()),
        Some(&user.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
async fn send(ws: &mut Ws, v: Value) {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(v.to_string().into()))
        .await
        .unwrap();
}
async fn recv(ws: &mut Ws, kind: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let f = ws.next().await.unwrap().unwrap();
            if let Ok(t) = f.to_text() {
                let v: Value = serde_json::from_str(t).unwrap();
                if v["type"] == kind {
                    return v;
                }
            }
        }
    })
    .await
    .unwrap()
}
async fn ws(server: &rynvek_server::RunningServer, user: &User, seat: &Value) -> Ws {
    let (mut s, _) = tokio_tungstenite::connect_async(format!(
        "{}/api/v1/worlds/{}/stream?format=arena-json",
        server.base_url().replace("http:", "ws:"),
        seat["world_id"].as_str().unwrap()
    ))
    .await
    .unwrap();
    send(&mut s,json!({"type":"hello","protocol":"rynvek-arena/2","principal_id":"不得授权的客户端字段","kind":"human","faction":seat["faction"],"capability_token":user.token})).await;
    assert_eq!(recv(&mut s, "welcome").await["faction"], seat["faction"]);
    let observation = recv(&mut s, "observation").await;
    for entity in observation["observation"]["units"].as_array().unwrap() {
        assert_eq!(entity["faction"], seat["faction"], "不得泄露敌方单位");
    }
    s
}
async fn completed(store: &Store, id: rynvek_protocol::WireUuid) {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            if store.get_match(id).await.unwrap().unwrap().0.match_state == MatchState::Completed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn ranked_queue_survives_context_restart_unique_pair_remote_discovery_and_fenced_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let first = start(store.clone(), dir.path()).await;
    let alice = register(&first.base_url(), "persistent-rank-a", "human", 151).await;
    let bob = register(&first.base_url(), "persistent-rank-b", "human", 152).await;
    let outsider = register(&first.base_url(), "persistent-rank-c", "human", 153).await;
    assert_eq!(common::arena::queue(&first.base_url(), &alice).await.0, 202);
    first.shutdown().await;
    let a = start(store.clone(), dir.path()).await;
    let b = start(store.clone(), dir.path()).await;
    assert_eq!(context(&a, &alice).await["status"], "queued");
    assert_eq!(context(&b, &bob).await["status"], "idle");
    assert!(
        store.matches_of(alice.principal_id).await.unwrap().is_empty(),
        "只读发现不能自己配对"
    );
    let abase = a.base_url();
    let bbase = b.base_url();
    let (one, two) = tokio::join!(common::arena::queue(&abase, &bob), common::arena::queue(&bbase, &bob));
    assert_eq!(one.0, 200, "{:?}", one.1);
    assert_eq!(two.0, 200, "{:?}", two.1);
    assert_eq!(one.1["match_id"], two.1["match_id"]);
    let mid = one.1["match_id"].as_str().unwrap();
    let id = mid.parse().unwrap();
    let world = one.1["world_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(store.matches_of(alice.principal_id).await.unwrap().len(), 1);
    assert_eq!(store.matches_of(bob.principal_id).await.unwrap().len(), 1);
    let origin = store.ranked_host(world).await.unwrap().unwrap().origin;
    let (owner, standby) = if origin == a.base_url() { (&a, &b) } else { (&b, &a) };
    let remote = connect(standby, &alice, mid).await;
    assert_eq!(remote["host"]["state"], "remote");
    assert_eq!(remote["host"]["origin"], owner.base_url());
    assert!(standby.state.registry().by_world(&world).is_none());
    let denied = client::send_json(
        "POST",
        &format!("{}/api/v1/matches/{mid}/connect", standby.base_url()),
        Some(&outsider.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(denied.status, 403);
    let sa = context(owner, &alice).await["active"].clone();
    let sb = context(owner, &bob).await["active"].clone();
    let mut wa = ws(owner, &alice, &sa).await;
    let wb = ws(owner, &bob, &sb).await;
    let old = owner.state.registry().by_world(&world).unwrap();
    old.wait_for(|s| s.state == MatchState::Running).await;
    old.advance(3).await.unwrap();
    let before = store.get_world(world).await.unwrap().unwrap();
    let old_fence = old.fencing_token.unwrap();
    let (mut duplicate, parts) = store.get_match(id).await.unwrap().unwrap();
    duplicate.match_id = rynvek_server::ids::new_uuid_v7();
    let duplicate_parts = parts
        .iter()
        .map(|p| rynvek_server::store::ParticipantRow {
            match_id: duplicate.match_id,
            ..p.clone()
        })
        .collect();
    assert!(
        store.insert_match(duplicate, duplicate_parts).await.is_err(),
        "其它模式不能抢已有席位"
    );
    store
        .release_ranked_hosts(owner.state.inner.ranked_node.clone())
        .await
        .unwrap();
    let recovered = connect(standby, &alice, mid).await;
    assert_eq!(recovered["host"]["state"], "local");
    assert_eq!(recovered["match_id"], mid);
    let host = standby.state.registry().by_world(&world).unwrap();
    assert_ne!(host.fencing_token, old.fencing_token);
    assert_eq!(host.status().state_root, before.latest_state_root);
    assert_eq!(host.status().tick, 3);
    assert!(old.advance(1).await.is_err());
    assert!(
        !store.get_match(id).await.unwrap().unwrap().0.match_state.is_terminal(),
        "退役旧Actor不能误终局"
    );
    let mut abort = store.get_match(id).await.unwrap().unwrap().0;
    abort.match_state = MatchState::Aborted;
    assert!(store.update_match_fenced(abort, old_fence).await.is_err());
    let mut stale = store.list_commits(world).await.unwrap().last().unwrap().clone();
    stale.parent_revision = Some(stale.world_revision);
    stale.world_revision += 1;
    assert!(store.append_commit(stale, old_fence).await.is_err());
    drop(wa);
    drop(wb);
    let ra = context(standby, &alice).await["active"].clone();
    let rb = context(standby, &bob).await["active"].clone();
    wa = ws(standby, &alice, &ra).await;
    let wb = ws(standby, &bob, &rb).await;
    host.wait_for(|s| s.state == MatchState::Running).await;
    send(&mut wa, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"})).await;
    let lease = recv(&mut wa, "lease").await;
    send(&mut wa,json!({"type":"command","seq":2,"fencing_token":lease["lease"]["fencing_token"],"command":{"type":"surrender"}})).await;
    host.wait_for(|s| s.pending_inputs == 1).await;
    host.advance(1).await.unwrap();
    assert_eq!(recv(&mut wa, "ack").await["ack"]["type"], "surrendered");
    completed(&store, id).await;
    let (finished, parts) = store.get_match(id).await.unwrap().unwrap();
    let rating = store
        .get_rating(alice.principal_id, rynvek_security::League::Human)
        .await
        .unwrap();
    assert_eq!(rating.matches_played, 1);
    store
        .settle_ranked(finished, host.fencing_token.unwrap(), parts)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_rating(alice.principal_id, rynvek_security::League::Human)
            .await
            .unwrap(),
        rating
    );
    assert_eq!(context(standby, &alice).await["status"], "idle");
    let replay = client::get(
        &format!("{}/api/v1/matches/{mid}/replay", standby.base_url()),
        Some(&alice.token),
    )
    .await
    .unwrap();
    assert_eq!(replay.status, 200);
    let mut keys = rynvek_security::Keyring::new();
    keys.insert(server_key().key_id(), server_key().public_key()).unwrap();
    let opened = rynvek_server::replay::open(&replay.body, Some(&keys)).unwrap();
    assert_eq!(opened.body.final_tick, 4, "必须覆盖接管前后三帧加终局，而非截断旧WAL");
    drop(wa);
    drop(wb);
    a.shutdown().await;
    b.shutdown().await;
}

fn request(user: &User, action: QueueAction) -> QueueCommand {
    QueueCommand {
        action,
        ticket: QueueTicket {
            league: rynvek_security::League::Human,
            ruleset: rynvek_server::app::arena_ruleset_hash(),
            entry: rynvek_server::matchmaking::QueueEntry {
                principal_id: user.principal_id,
                rating: 1500.0,
                deviation: 100.0,
                enqueued_at_s: rynvek_server::ids::unix_now_s(),
            },
        },
    }
}
#[tokio::test]
async fn ranked_atomic_reservation_failure_and_committed_terminal_without_actor_reconcile_once() {
    use rynvek_rts::{ArenaRuntime, ArenaWorld, FactionId, MatchConfig, commands::RtsCommand};
    use rynvek_server::transactions::{TickCommitter, TickInput, TickRecord};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let server = start(store.clone(), dir.path()).await;
    let a = register(&server.base_url(), "reserve-a", "human", 154).await;
    let b = register(&server.base_url(), "reserve-b", "human", 155).await;
    assert!(matches!(
        store.ranked_queue(request(&a, QueueAction::Join), None).await.unwrap(),
        QueueDecision::Queued { .. }
    ));
    assert!(matches!(
        store.ranked_queue(request(&b, QueueAction::Join), None).await.unwrap(),
        QueueDecision::Prepare
    ));
    let mut bad = server.state.prepare_ranked_genesis().unwrap();
    bad.world.world_id = rynvek_protocol::WireUuid([0; 16]); // 制造真实FK/组织配额以外的已存在ID冲突。
    // 在同一CAS快照下先登记这个ID的世界，让配对事务必然失败；不得丢双方队列或半建对局。
    bad.world.org_id = a.principal_id;
    bad.world.owner_principal_id = a.principal_id;
    bad.commit.world_id = bad.world.world_id;
    store.insert_world(bad.world.clone(), bad.commit.clone()).await.unwrap();
    assert!(
        store
            .ranked_queue(request(&b, QueueAction::Poll), Some(bad))
            .await
            .is_err()
    );
    assert!(store.matches_of(a.principal_id).await.unwrap().is_empty());
    assert_eq!(context(&server, &a).await["status"], "queued");
    assert_eq!(context(&server, &b).await["status"], "queued");
    let g = server.state.prepare_ranked_genesis().unwrap();
    let world = g.world.world_id;
    let seed = g.row.map_seed;
    let QueueDecision::Matched(id) = store
        .ranked_queue(request(&b, QueueAction::Poll), Some(g))
        .await
        .unwrap()
    else {
        panic!("真实配对")
    };
    let discovered = context(&server, &a).await;
    assert_eq!(discovered["active"]["host"]["state"], "recoverable");
    assert!(server.state.registry().by_world(&world).is_none());
    let w = store.get_world(world).await.unwrap().unwrap();
    let lease = store
        .claim_ranked_host(HostClaim {
            world,
            revision: 0,
            fence: w.fencing_token,
            node: "已退出的测试宿主".into(),
            origin: server.base_url(),
        })
        .await
        .unwrap();
    let rt = ArenaRuntime::new(world.0, ArenaWorld::new(MatchConfig::duel(seed)));
    let mut next = rt.candidate_tick().unwrap();
    next.command(FactionId(0), &RtsCommand::Surrender).unwrap();
    next.finish_tick().unwrap();
    let record = TickRecord {
        format: "rynvek-tick/2".into(),
        base_revision: 0,
        base_root: rt.state_root(),
        target_tick: 1,
        tick: next.tick,
        revision: next.revision(),
        state_root: next.state_root(),
        inputs: vec![TickInput::Command {
            faction: 0,
            command: RtsCommand::Surrender,
            principal_id: a.principal_id,
            idempotency_key: "terminal-before-exit".into(),
        }],
    };
    let committer = TickCommitter {
        store: store.clone(),
        snapshots: server.state.inner.world_snapshots.clone(),
        world_id: world,
        fencing_token: lease.fence,
    };
    committer.persist(&next, &record).await.unwrap();
    // 模拟终局已提交、尚未发送完成事件；释放只是受控drain，不冒充断电。
    store.release_ranked_hosts("已退出的测试宿主".into()).await.unwrap();
    connect(&server, &a, &id.to_string()).await;
    completed(&store, id).await;
    assert_eq!(
        store
            .get_rating(a.principal_id, rynvek_security::League::Human)
            .await
            .unwrap()
            .matches_played,
        1
    );
    assert_eq!(context(&server, &a).await["status"], "idle");
    server.shutdown().await;
}
