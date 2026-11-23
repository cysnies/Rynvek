// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 真实回环HTTP/Ed25519身份、原Store+私有文件CAS；不是PG实库/多机断电证据。
mod common;
use common::*;
use rynvek_core::Blake3Hash;
use rynvek_protocol::{CapabilityScope, CommandEnvelope, IdempotencyKey, WireUuid};
use rynvek_server::{
    AppConfig, RunningServer, ServerConfig, client,
    general_host::{GeneralInput, GeneralTickRecord},
    ids::new_uuid_v7,
    store::Store,
};
use rynvek_simulation::physics::PhysicsCommand;
use rynvek_world::general::GeneralWorldConfig;
use serde_json::{Value, json};

fn rules() -> GeneralWorldConfig {
    serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap()
}
fn physical() -> GeneralWorldConfig {
    let mut c = rules();
    c.life = None;
    c.components = None;
    c
}
async fn start(store: Store, path: &std::path::Path) -> RunningServer {
    rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: AppConfig {
            public_base_url: String::new(),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            lobby_timeout: None,
            blob_dir: Some(path.to_owned()),
            ..AppConfig::default()
        },
        store,
        signer: server_key(),
    })
    .await
    .unwrap()
}
async fn create(s: &RunningServer, u: &User, c: &GeneralWorldConfig) -> WireUuid {
    let bytes = c.build_package("world.test.general-authority", "0.1.0").unwrap();
    let r = client::request(
        "POST",
        &format!("{}/api/v1/worlds/general", s.base_url()),
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
async fn action(s: &RunningServer, u: &User, id: WireUuid, v: Value) -> client::HttpResponse {
    client::send_json(
        "POST",
        &format!("{}/api/v1/worlds/{id}/general", s.base_url()),
        Some(&u.token),
        &v,
    )
    .await
    .unwrap()
}
async fn ok_action(s: &RunningServer, u: &User, id: WireUuid, v: Value) -> Value {
    let r = action(s, u, id, v).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
async fn connect(s: &RunningServer, u: &User, id: WireUuid) -> u64 {
    ok_action(s, u, id, json!({"action":"connect"})).await["host_fence"]
        .as_u64()
        .unwrap()
}
async fn acquire(s: &RunningServer, u: &User, id: WireUuid, h: u64, level: &str) -> u64 {
    ok_action(s, u, id, json!({"action":"acquire","host_fence":h,"level":level})).await["lease"]["fencing_token"]
        .as_u64()
        .unwrap()
}
fn input(u: &User, h: u64, l: u64, tick: u64, commands: Vec<PhysicsCommand>) -> GeneralInput {
    GeneralInput {
        install: None,
        recovery: None,
        host_fence: h,
        control_fence: l,
        envelope: CommandEnvelope {
            idempotency_key: IdempotencyKey::new(new_uuid_v7()).unwrap(),
            principal_id: u.principal_id,
            issued_at_tick: tick,
            target_tick: tick + 1,
            payload: commands,
        },
    }
}
async fn step(s: &RunningServer, u: &User, id: WireUuid, i: &GeneralInput) -> client::HttpResponse {
    action(s, u, id, json!({"action":"tick","input":i})).await
}
async fn view(s: &RunningServer, u: &User, id: WireUuid) -> Value {
    let r = client::get(&format!("{}/api/v1/worlds/{id}/general", s.base_url()), Some(&u.token))
        .await
        .unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
#[tokio::test]
async fn signed_identity_paid_components_run_through_original_store_and_private_wal() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "general-owner", "human", 81).await;
    let config = rules();
    let id = create(&s, &u, &config).await;
    let initial = view(&s, &u, id).await;
    assert_eq!(initial["life"]["accounts"][0]["stored_q"], 0);
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    let i = input(&u, h, l, 0, vec![]);
    let r = step(&s, &u, id, &i).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    let after = view(&s, &u, id).await;
    assert_eq!(after["commit"]["tick"], 1);
    assert!(after["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_ne!(
        after["bodies"][0]["body"]["position_q32"],
        initial["bodies"][0]["body"]["position_q32"]
    );
    assert_eq!(after["life"]["genesis_energy_q"], after["life"]["universe_energy_q"]);
    let mut engine = config.instantiate(id.0).unwrap();
    engine.tick().unwrap();
    assert_eq!(
        after["commit"]["state_root"],
        rynvek_server::ids::hex(&engine.parent_commit_hash)
    );
    let commits = s.state.store().list_commits(id).await.unwrap();
    assert_eq!(commits.len(), 2);
    assert!(commits[1].snapshot_cid.is_none());
    let raw = s
        .state
        .inner
        .world_snapshots
        .get(&Blake3Hash(commits[1].journal_segment_cid))
        .unwrap();
    let record: GeneralTickRecord = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        record
            .replay(&config.instantiate(id.0).unwrap())
            .unwrap()
            .parent_commit_hash,
        engine.parent_commit_hash
    );
    let cid = Blake3Hash(commits[1].journal_segment_cid).to_hex();
    // 私有日志没有被写入原公共Hub CAS。
    assert!(
        s.state
            .inner
            .blobs
            .get(&Blake3Hash(commits[1].journal_segment_cid))
            .is_err(),
        "private {cid}"
    );
    let dup = step(&s, &u, id, &i).await;
    assert_eq!(dup.status, 200);
    assert_eq!(dup.json().unwrap()["duplicate"], true);
    assert_eq!(s.state.store().get_world(id).await.unwrap().unwrap().latest_tick, 1);
    s.shutdown().await;
}
#[tokio::test]
async fn private_admission_scopes_identity_control_and_revocation_are_real() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "general-alice", "human", 82).await;
    let other = register(&s.base_url(), "general-bob", "human", 83).await;
    let id = create(&s, &u, &rules()).await;
    let url = format!("{}/api/v1/worlds/{id}/general", s.base_url());
    assert_eq!(client::get(&url, None).await.unwrap().status, 401);
    assert_eq!(client::get(&url, Some(&other.token)).await.unwrap().status, 404);
    assert_eq!(action(&s, &other, id, json!({"action":"connect"})).await.status, 404);
    let scope = s.state.inner.tokens.write().unwrap().issue(
        u.principal_id,
        rynvek_protocol::CapabilityScopeSet::default().with(CapabilityScope::WorldRead),
        rynvek_server::ids::unix_now_s(),
        60,
    );
    let mut limited = User {
        principal_id: u.principal_id,
        token: rynvek_server::app::encode_token_text(&scope),
        key: server_key(),
    };
    assert_eq!(action(&s, &limited, id, json!({"action":"connect"})).await.status, 403);
    let h = connect(&s, &u, id).await;
    let low = acquire(&s, &u, id, h, "Produce").await;
    assert_eq!(step(&s, &u, id, &input(&u, h, low, 0, vec![])).await.status, 403);
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    assert_ne!(step(&s, &u, id, &input(&u, h, low, 0, vec![])).await.status, 200);
    let mut forged = input(&u, h, l, 0, vec![]);
    forged.envelope.principal_id = other.principal_id;
    assert_eq!(step(&s, &u, id, &forged).await.status, 403);
    limited.token = u.token.clone();
    s.state.store().tombstone_principal(u.principal_id).await.unwrap();
    assert_ne!(step(&s, &limited, id, &input(&u, h, l, 0, vec![])).await.status, 200);
    assert_ne!(client::get(&url, Some(&u.token)).await.unwrap().status, 200);
    assert_eq!(s.state.store().get_world(id).await.unwrap().unwrap().latest_revision, 0);
    s.shutdown().await;
}
#[tokio::test]
async fn life_cannot_get_free_spawn_or_thrust_and_failed_tick_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "general-atomic", "human", 84).await;
    let id = create(&s, &u, &rules()).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    let before = view(&s, &u, id).await;
    for cmd in [
        PhysicsCommand::Spawn {
            body: rules().bodies[0],
        },
        PhysicsCommand::SetAcceleration {
            serial: 1,
            acceleration_q32: [1, 0],
        },
    ] {
        assert_eq!(step(&s, &u, id, &input(&u, h, l, 0, vec![cmd])).await.status, 403);
    }
    assert_ne!(
        step(
            &s,
            &u,
            id,
            &input(&u, h, l, 0, vec![PhysicsCommand::Despawn { serial: 999 }])
        )
        .await
        .status,
        200
    );
    assert_eq!(view(&s, &u, id).await, before);
    assert_eq!(s.state.store().list_commits(id).await.unwrap().len(), 1);
    let mut cfg = rules();
    cfg.components.as_mut().unwrap().rule_cost_micro_per_gas = Some(1_000_000);
    let expensive = create(&s, &u, &cfg).await;
    let eh = connect(&s, &u, expensive).await;
    let el = acquire(&s, &u, expensive, eh, "Sovereign").await;
    let base = view(&s, &u, expensive).await;
    assert_ne!(step(&s, &u, expensive, &input(&u, eh, el, 0, vec![])).await.status, 200);
    assert_eq!(view(&s, &u, expensive).await, base);
    assert_eq!(
        s.state.store().get_world(expensive).await.unwrap().unwrap().latest_tick,
        0
    );
    s.shutdown().await;
}
#[tokio::test]
async fn stale_host_cannot_publish_or_observe_actor_and_healthy_peer_cannot_steal() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let peer = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "general-fence", "human", 85).await;
    let id = create(&s, &u, &rules()).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    assert_eq!(action(&peer, &u, id, json!({"action":"connect"})).await.status, 409);
    store
        .release_ranked_hosts(s.state.inner.ranked_node.clone())
        .await
        .unwrap();
    let h2 = connect(&peer, &u, id).await;
    assert!(h2 > h);
    let l2 = acquire(&peer, &u, id, h2, "Sovereign").await;
    assert_eq!(action(&s, &u, id, json!({"action":"connect"})).await.status, 409);
    assert_ne!(step(&peer, &u, id, &input(&u, h, l, 0, vec![])).await.status, 200);
    assert_eq!(store.get_world(id).await.unwrap().unwrap().latest_revision, 0);
    assert_eq!(step(&peer, &u, id, &input(&u, h2, l2, 0, vec![])).await.status, 200);
    s.shutdown().await;
    assert_eq!(view(&peer, &u, id).await["commit"]["tick"], 1);
    peer.shutdown().await;
}
#[tokio::test]
async fn fresh_app_file_cas_restores_wal_and_exactly_once_receipt_without_resetting_host_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "general-restore", "human", 86).await;
    let id = create(&s, &u, &rules()).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    let mut last = input(&u, h, l, 0, vec![]);
    for tick in 0..3 {
        last = input(&u, h, l, tick, vec![]);
        let r = step(&s, &u, id, &last).await;
        assert_eq!(r.status, 200, "{:?}", r.json());
    }
    let expected = view(&s, &u, id).await;
    s.shutdown().await;
    let s = start(store.clone(), dir.path()).await;
    assert_eq!(view(&s, &u, id).await, expected);
    let h2 = connect(&s, &u, id).await;
    assert!(h2 > h);
    let l2 = acquire(&s, &u, id, h2, "Sovereign").await;
    assert_ne!(step(&s, &u, id, &last).await.status, 200);
    last.host_fence = h2;
    last.control_fence = l2;
    let duplicate = step(&s, &u, id, &last).await;
    assert_eq!(duplicate.status, 200);
    assert_eq!(duplicate.json().unwrap()["duplicate"], true);
    last.envelope.payload.push(PhysicsCommand::Despawn { serial: 1 });
    assert_eq!(step(&s, &u, id, &last).await.status, 409);
    assert_eq!(view(&s, &u, id).await, expected);
    s.shutdown().await;
}
#[tokio::test]
async fn periodic_snapshot_plus_suffix_replays_real_external_edit_not_fake_arena() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "general-anchor", "human", 87).await;
    let config = physical();
    let id = create(&s, &u, &config).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    let mut local = config.instantiate(id.0).unwrap();
    for tick in 0..301 {
        let commands = if tick == 300 {
            vec![PhysicsCommand::SetAcceleration {
                serial: 1,
                acceleration_q32: [1 << 30, 0],
            }]
        } else {
            vec![]
        };
        local.tick_with_commands(&commands).unwrap();
        let r = step(&s, &u, id, &input(&u, h, l, tick, commands)).await;
        assert_eq!(r.status, 200, "Tick {tick} {:?}", r.json());
    }
    let c = store.list_commits(id).await.unwrap();
    assert!(c[300].snapshot_cid.is_some());
    assert!(c[301].snapshot_cid.is_none());
    let expected = view(&s, &u, id).await;
    assert_eq!(
        expected["commit"]["state_root"],
        rynvek_server::ids::hex(&local.parent_commit_hash)
    );
    assert!(store.matches_of(u.principal_id).await.unwrap().is_empty());
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(view(&s, &u, id).await, expected);
    s.shutdown().await;
}
#[tokio::test]
async fn agent_cannot_acquire_sovereign_and_close_does_not_reuse_control_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let mut a = register(&s.base_url(), "general-agent", "agent", 88).await;
    // Agent默认令牌不含world.create；仅测试受控签发完整scope仍不能绕过原Sovereign主体种类限制。
    let token = s.state.inner.tokens.write().unwrap().issue(
        a.principal_id,
        CapabilityScope::ALL.into_iter().collect(),
        rynvek_server::ids::unix_now_s(),
        60,
    );
    a.token = rynvek_server::app::encode_token_text(&token);
    let id = create(&s, &a, &physical()).await;
    let h = connect(&s, &a, id).await;
    assert_eq!(
        action(
            &s,
            &a,
            id,
            json!({"action":"acquire","host_fence":h,"level":"Sovereign"})
        )
        .await
        .status,
        403
    );
    let u = register(&s.base_url(), "general-close", "human", 89).await;
    let id = create(&s, &u, &physical()).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    ok_action(&s, &u, id, json!({"action":"close","host_fence":h,"control_fence":l})).await;
    assert_eq!(action(&s, &u, id, json!({"action":"connect"})).await.status, 409);
    assert_ne!(step(&s, &u, id, &input(&u, h, l, 0, vec![])).await.status, 200);
    s.shutdown().await;
}
#[tokio::test]
async fn forged_private_wal_cannot_replay_even_with_a_valid_cas_digest() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let u = register(&s.base_url(), "general-corrupt", "human", 90).await;
    let cfg = physical();
    let id = create(&s, &u, &cfg).await;
    let mut world = cfg.instantiate(id.0).unwrap();
    let root = world.parent_commit_hash;
    world.tick().unwrap();
    let mut i = input(&u, 1, 1, 0, vec![]);
    i.envelope.payload.push(PhysicsCommand::Despawn { serial: 999 });
    let record = GeneralTickRecord {
        installation: None,
        format: "rynvek-general-tick/1".into(),
        base_revision: 0,
        base_root: root,
        input: i,
        commit_bytes: world.authoritative_commit(root).encode(),
    };
    // 直接构造损坏的存储夹具，不经过正式Actor；摘要合法不能替代真实逐Tick验算。
    let cid = s
        .state
        .inner
        .world_snapshots
        .put(&serde_json::to_vec(&record).unwrap())
        .unwrap();
    store
        .append_commit(
            rynvek_server::store::CommitRow {
                world_id: id,
                world_revision: 1,
                tick: 1,
                parent_revision: Some(0),
                state_root_blake3: world.parent_commit_hash,
                journal_segment_cid: cid.0,
                snapshot_cid: None,
            },
            1,
        )
        .await
        .unwrap();
    let r = client::get(&format!("{}/api/v1/worlds/{id}/general", s.base_url()), Some(&u.token))
        .await
        .unwrap();
    assert_ne!(r.status, 200);
    assert_ne!(action(&s, &u, id, json!({"action":"connect"})).await.status, 200);
    assert!(store.ranked_host(id).await.unwrap().is_none());
    s.shutdown().await;
}
#[tokio::test]
async fn release_duplicate_collision_bad_time_and_legacy_routes_never_publish() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "general-reject", "human", 91).await;
    let id = create(&s, &u, &physical()).await;
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    ok_action(&s, &u, id, json!({"action":"release","host_fence":h,"control_fence":l})).await;
    assert_ne!(step(&s, &u, id, &input(&u, h, l, 0, vec![])).await.status, 200);
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    let mut future = input(&u, h, l, 0, vec![]);
    future.envelope.target_tick = 2;
    assert_ne!(step(&s, &u, id, &future).await.status, 200);
    let excessive = input(&u, h, l, 0, vec![PhysicsCommand::Despawn { serial: 999 }; 33]);
    assert_ne!(step(&s, &u, id, &excessive).await.status, 200);
    let base = format!("{}/api/v1/worlds/{id}", s.base_url());
    assert_eq!(
        client::send_json("POST", &format!("{base}/fork"), Some(&u.token), &json!({}))
            .await
            .unwrap()
            .status,
        400
    );
    assert_eq!(
        client::send_json("POST", &format!("{base}/patches"), Some(&u.token), &json!({}))
            .await
            .unwrap()
            .status,
        400
    );
    assert_eq!(s.state.store().get_world(id).await.unwrap().unwrap().latest_revision, 0);
    let i = input(&u, h, l, 0, vec![]);
    assert_eq!(step(&s, &u, id, &i).await.status, 200);
    let mut changed = i;
    changed.envelope.payload.push(PhysicsCommand::Despawn { serial: 1 });
    assert_eq!(step(&s, &u, id, &changed).await.status, 409);
    assert_eq!(s.state.store().get_world(id).await.unwrap().unwrap().latest_revision, 1);
    s.shutdown().await;
}

#[tokio::test]
async fn current_identity_is_original_authenticated_active_self_only_and_private_no_store() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "general-web-identity", "human", 114).await;
    let url = format!("{}/api/v1/auth/session", s.base_url());
    assert_eq!(client::get(&url, None).await.unwrap().status, 401);
    assert_eq!(
        client::get(&url, Some("invalid-test-capability")).await.unwrap().status,
        401
    );
    let r = client::get(&url, Some(&u.token)).await.unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.header("cache-control"), Some("private, no-store"));
    assert_eq!(r.header("vary"), Some("Authorization"));
    let value = r.json().unwrap();
    assert_eq!(value["principal_id"], u.principal_id.to_string());
    assert_eq!(value["kind"], "Human");
    assert_eq!(value["ed25519_pubkey"], rynvek_server::ids::hex(&u.key.public_key()));
    assert!(value.get("token").is_none());
    let list = client::get(&format!("{}/api/v1/worlds", s.base_url()), Some(&u.token))
        .await
        .unwrap();
    assert_eq!(list.header("cache-control"), Some("private, no-store"));
    s.state.store().tombstone_principal(u.principal_id).await.unwrap();
    assert_eq!(client::get(&url, Some(&u.token)).await.unwrap().status, 403);
    s.shutdown().await;
}

#[tokio::test]
async fn minimal_private_render_frame_is_authoritative_read_only_and_never_contains_source_or_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let u = register(&s.base_url(), "general-render-owner", "human", 115).await;
    let other = register(&s.base_url(), "general-render-other", "human", 116).await;
    let id = create(&s, &u, &rules()).await;
    let url = format!("{}/api/v1/worlds/{id}/general/frame", s.base_url());
    assert_eq!(client::get(&url, None).await.unwrap().status, 401);
    assert_eq!(client::get(&url, Some(&other.token)).await.unwrap().status, 404);
    let response = client::get(&url, Some(&u.token)).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.header("cache-control"), Some("private, no-store"));
    assert!(
        s.state.store().ranked_host(id).await.unwrap().is_none(),
        "只读帧不得抢占宿主"
    );
    let initial: rynvek_engine::general::GeneralRenderFrame = serde_json::from_slice(&response.body).unwrap();
    assert!(initial.validate());
    let h = connect(&s, &u, id).await;
    let l = acquire(&s, &u, id, h, "Sovereign").await;
    assert_eq!(step(&s, &u, id, &input(&u, h, l, 0, vec![])).await.status, 200);
    let f: rynvek_engine::general::GeneralRenderFrame =
        serde_json::from_slice(&client::get(&url, Some(&u.token)).await.unwrap().body).unwrap();
    let observation = view(&s, &u, id).await;
    assert_eq!(f.tick, 1);
    assert_eq!(f.state_root, observation["commit"]["state_root"]);
    assert_ne!(f.bodies, initial.bodies);
    let raw = serde_json::to_value(&f).unwrap();
    assert_eq!(raw.as_object().unwrap().len(), 6);
    assert_eq!(raw["bodies"][0].as_object().unwrap().len(), 3);
    assert!(raw.get("components").is_none());
    assert!(raw.get("life").is_none());
    assert!(raw.get("snapshot").is_none());
    let mut engine = rynvek_engine::Engine::new(rynvek_engine::EngineConfig {
        world_id: id.0,
        general_remote: true,
        ..Default::default()
    });
    engine
        .handle_command(
            rynvek_engine::EngineCommand::from_json(&json!({"type":"GENERAL_FRAME","frame":f}).to_string()).unwrap(),
        )
        .unwrap();
    assert_eq!(
        engine.frame_scene().instance_count(),
        observation["bodies"].as_array().unwrap().len()
    );
    assert_eq!(engine.simulation().clock.tick, 0);
    s.state.store().tombstone_principal(u.principal_id).await.unwrap();
    assert_ne!(client::get(&url, Some(&u.token)).await.unwrap().status, 200);
    s.shutdown().await;
}
