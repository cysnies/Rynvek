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
        admission_grant_id: None,
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

// 目的隔离规则驱动：共用上述真实HTTP/签名身份/有源世界辅助，不创建第二种世界。
async fn driver_change(
    s: &RunningServer,
    o: &User,
    id: WireUuid,
    d: &User,
    expected: Option<WireUuid>,
    active: bool,
) -> WireUuid {
    let r=client::send_json("POST",&format!("{}/api/v1/worlds/{id}/general/drivers",s.base_url()),Some(&o.token),&json!({"principal_id":d.principal_id.to_string(),"expected_grant_id":expected,"active":active,"consent":"general-installed-rule-drive/1"})).await.unwrap();
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()["grant_id"].as_str().unwrap().parse().unwrap()
}
async fn driver_request(
    s: &RunningServer,
    d: &User,
    id: WireUuid,
    g: WireUuid,
    command: Option<Value>,
) -> client::HttpResponse {
    let body = command.map_or_else(
        || json!({"grant_id":g,"frame":true}),
        |c| json!({"grant_id":g,"command":c}),
    );
    client::send_json(
        "POST",
        &format!("{}/api/v1/worlds/{id}/general/drive", s.base_url()),
        Some(&d.token),
        &body,
    )
    .await
    .unwrap()
}
async fn driver_ok(s: &RunningServer, d: &User, id: WireUuid, g: WireUuid, c: Value) -> Value {
    let r = driver_request(s, d, id, g, Some(c)).await;
    assert_eq!(r.status, 200, "{:?}", r.json());
    r.json().unwrap()
}
async fn driver_control(s: &RunningServer, d: &User, id: WireUuid, g: WireUuid) -> (u64, u64) {
    let c = driver_ok(s, d, id, g, json!({"action":"connect"})).await;
    assert!(c.get("observation").is_none());
    let h = c["host_fence"].as_u64().unwrap();
    let a = driver_ok(
        s,
        d,
        id,
        g,
        json!({"action":"acquire","host_fence":h,"level":"Sovereign"}),
    )
    .await;
    (h, a["lease"]["fencing_token"].as_u64().unwrap())
}
#[tokio::test]
async fn general_driver_consent_and_studio_purpose_never_upgrade_or_overwrite_each_other() {
    use rynvek_server::store::{AdmissionPurpose, EditorPermit};
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let o = register(&s.base_url(), "driver-purpose-owner", "human", 141).await;
    let d = register(&s.base_url(), "driver-purpose-guest", "human", 142).await;
    let id = create(&s, &o, &rules()).await;
    let base = format!("{}/api/v1/worlds/{id}", s.base_url());
    let bad = json!({"principal_id":d.principal_id.to_string(),"expected_grant_id":null,"active":true,"consent":"studio-source-edit/1"});
    assert_eq!(
        client::send_json("POST", &format!("{base}/general/drivers"), Some(&o.token), &bad)
            .await
            .unwrap()
            .status,
        400
    );
    let old = client::send_json("POST", &format!("{base}/authoring/editors"), Some(&o.token), &bad)
        .await
        .unwrap();
    assert_eq!(old.status, 200);
    let studio = old.json().unwrap()["grant_id"]
        .as_str()
        .unwrap()
        .parse::<WireUuid>()
        .unwrap();
    assert_eq!(driver_request(&s, &d, id, studio, None).await.status, 404);
    let general = driver_change(&s, &o, id, &d, None, true).await;
    assert!(
        s.state
            .store()
            .check_editor(
                id,
                EditorPermit {
                    principal_id: d.principal_id,
                    grant_id: Some(general),
                    purpose: AdmissionPurpose::StudioSourceEdit
                }
            )
            .await
            .is_err()
    );
    assert!(
        s.state
            .store()
            .check_editor(
                id,
                EditorPermit {
                    principal_id: d.principal_id,
                    grant_id: Some(studio),
                    purpose: AdmissionPurpose::GeneralRuleDrive
                }
            )
            .await
            .is_err()
    );
    assert_eq!(s.state.store().editor_grants(id).await.unwrap().len(), 2);
    for suffix in ["", "/general", "/general/frame", "/commits"] {
        assert_eq!(
            client::get(&format!("{base}{suffix}"), Some(&d.token))
                .await
                .unwrap()
                .status,
            404
        );
    }
    assert_eq!(action(&s, &d, id, json!({"action":"connect"})).await.status, 404);
    let a = client::get(&format!("{base}/authoring/editors"), Some(&o.token))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(a["grants"].as_array().unwrap().len(), 1);
    assert_eq!(a["grants"][0]["grant_id"], studio.to_string());
    let b = client::get(&format!("{base}/general/drivers"), Some(&o.token))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(b["grants"].as_array().unwrap().len(), 1);
    assert_eq!(b["grants"][0]["grant_id"], general.to_string());
    let revoke = json!({"principal_id":d.principal_id.to_string(),"expected_grant_id":studio,"active":false,"consent":"studio-source-edit/1"});
    assert_eq!(
        client::send_json("POST", &format!("{base}/authoring/editors"), Some(&o.token), &revoke)
            .await
            .unwrap()
            .status,
        200
    );
    assert_eq!(driver_request(&s, &d, id, general, None).await.status, 200);
    s.shutdown().await;
}
#[tokio::test]
async fn general_driver_only_minimal_frames_and_empty_installed_rule_ticks_are_paid() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let o = register(&s.base_url(), "driver-paid-owner", "human", 143).await;
    let d = register(&s.base_url(), "driver-paid-guest", "human", 144).await;
    let id = create(&s, &o, &rules()).await;
    let g = driver_change(&s, &o, id, &d, None, true).await;
    let before = view(&s, &o, id).await;
    let r = driver_request(&s, &d, id, g, None).await;
    assert_eq!(r.status, 200);
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("cache-control") && v.contains("no-store"))
    );
    let f = r.json().unwrap()["frame"].clone();
    let mut keys = f.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, ["bodies", "format", "revision", "stateRoot", "tick", "worldId"]);
    assert!(s.state.store().ranked_host(id).await.unwrap().is_none());
    let (h, l) = driver_control(&s, &d, id, g).await;
    for action in [
        json!({"action":"close","host_fence":h,"control_fence":l}),
        json!({"action":"install","input":input(&d,h,l,0,vec![])}),
        json!({"action":"recover","input":input(&d,h,l,0,vec![])}),
        json!({"action":"tick","input":input(&d,h,l,0,vec![PhysicsCommand::Despawn{serial:1}])}),
    ] {
        assert_eq!(driver_request(&s, &d, id, g, Some(action)).await.status, 403);
    }
    let mut forged = input(&d, h, l, 0, vec![]);
    forged.envelope.principal_id = o.principal_id;
    assert_eq!(
        driver_request(&s, &d, id, g, Some(json!({"action":"tick","input":forged})))
            .await
            .status,
        403
    );
    assert_eq!(view(&s, &o, id).await, before);
    let i = input(&d, h, l, 0, vec![]);
    let r = driver_ok(&s, &d, id, g, json!({"action":"tick","input":i})).await;
    assert!(r.get("observation").is_none());
    assert_eq!(r["frame"]["tick"], 1);
    assert_ne!(r["frame"]["bodies"], f["bodies"]);
    let after = view(&s, &o, id).await;
    assert!(after["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_eq!(after["life"]["universe_energy_q"], before["life"]["universe_energy_q"]);
    let dup = driver_ok(&s, &d, id, g, json!({"action":"tick","input":i})).await;
    assert_eq!(dup["duplicate"], true);
    assert!(dup.get("frame").is_none());
    assert_eq!(view(&s, &o, id).await, after);
    let commits = s.state.store().list_commits(id).await.unwrap();
    let row = commits.last().unwrap();
    let raw = s
        .state
        .inner
        .world_snapshots
        .get(&Blake3Hash(row.journal_segment_cid))
        .unwrap();
    let record: GeneralTickRecord = serde_json::from_slice(&raw).unwrap();
    assert_eq!(record.admission_grant_id, Some(g));
    assert_eq!(record.format, "rynvek-general-tick/4");
    assert_eq!(record.input.envelope.principal_id, d.principal_id);
    let genesis = rules().instantiate(id.0).unwrap();
    assert!(record.replay(&genesis).is_ok());
    let mut wrong = record.clone();
    wrong.admission_grant_id = None;
    assert!(wrong.replay(&genesis).is_err(), "/4不可抹去目的版本");
    let mut wrong = record.clone();
    wrong.format = "rynvek-general-tick/1".into();
    assert!(wrong.replay(&genesis).is_err(), "旧格式不可偷带协作目的");
    let mut wrong = record.clone();
    wrong.input.envelope.payload.push(PhysicsCommand::Despawn { serial: 1 });
    assert!(wrong.replay(&genesis).is_err(), "目的版本不授结构编辑");
    assert!(s.state.inner.blobs.get(&Blake3Hash(row.journal_segment_cid)).is_err());
    s.shutdown().await;
}
#[tokio::test]
async fn general_driver_regrant_cannot_reuse_old_control_fence_or_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let o = register(&s.base_url(), "driver-fence-owner", "human", 145).await;
    let d = register(&s.base_url(), "driver-fence-guest", "human", 146).await;
    let id = create(&s, &o, &rules()).await;
    let g = driver_change(&s, &o, id, &d, None, true).await;
    let (h, l) = driver_control(&s, &d, id, g).await;
    let old = input(&d, h, l, 0, vec![]);
    driver_ok(&s, &d, id, g, json!({"action":"tick","input":old})).await;
    assert_eq!(
        action(
            &s,
            &o,
            id,
            json!({"action":"acquire","host_fence":h,"level":"Sovereign"})
        )
        .await
        .status,
        403,
        "不能偷抢有效人类租约"
    );
    let revoked = driver_change(&s, &o, id, &d, Some(g), false).await;
    assert_eq!(driver_request(&s, &d, id, g, None).await.status, 404);
    let fresh = driver_change(&s, &o, id, &d, Some(revoked), true).await;
    assert_ne!(
        driver_request(
            &s,
            &d,
            id,
            fresh,
            Some(json!({"action":"tick","input":input(&d,h,l,1,vec![])}))
        )
        .await
        .status,
        200
    );
    let l2 = driver_ok(
        &s,
        &d,
        id,
        fresh,
        json!({"action":"acquire","host_fence":h,"level":"Sovereign"}),
    )
    .await["lease"]["fencing_token"]
        .as_u64()
        .unwrap();
    assert!(l2 > l);
    let mut retry = old.clone();
    retry.control_fence = l2;
    assert_eq!(
        driver_request(&s, &d, id, fresh, Some(json!({"action":"tick","input":retry})))
            .await
            .status,
        409
    );
    assert_eq!(view(&s, &o, id).await["commit"]["tick"], 1);
    driver_ok(
        &s,
        &d,
        id,
        fresh,
        json!({"action":"tick","input":input(&d,h,l2,1,vec![])}),
    )
    .await;
    assert_eq!(view(&s, &o, id).await["commit"]["tick"], 2);
    s.shutdown().await;
}
#[tokio::test]
async fn general_driver_revocation_is_rechecked_in_original_store_commit_lock() {
    use rynvek_server::store::{AdmissionPurpose, CommitRow, EditorPermit};
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let o = register(&s.base_url(), "driver-lock-owner", "human", 147).await;
    let d = register(&s.base_url(), "driver-lock-guest", "human", 148).await;
    let id = create(&s, &o, &rules()).await;
    let g = driver_change(&s, &o, id, &d, None, true).await;
    let (h, l) = driver_control(&s, &d, id, g).await;
    let before = view(&s, &o, id).await;
    let mut candidate = rules().instantiate(id.0).unwrap();
    let root = candidate.parent_commit_hash;
    candidate.tick_with_commands(&[]).unwrap();
    let manifest = candidate.authoritative_commit(root);
    let record = GeneralTickRecord {
        format: "rynvek-general-tick/4".into(),
        base_revision: 0,
        base_root: root,
        input: input(&d, h, l, 0, vec![]),
        commit_bytes: manifest.encode(),
        installation: None,
        admission_grant_id: Some(g),
    };
    let cid = s
        .state
        .inner
        .world_snapshots
        .put(&serde_json::to_vec(&record).unwrap())
        .unwrap();
    driver_change(&s, &o, id, &d, Some(g), false).await;
    assert!(
        s.state
            .store()
            .append_commit_guarded(
                CommitRow {
                    world_id: id,
                    world_revision: 1,
                    tick: 1,
                    parent_revision: Some(0),
                    state_root_blake3: candidate.parent_commit_hash,
                    journal_segment_cid: cid.0,
                    snapshot_cid: None
                },
                h,
                vec![EditorPermit {
                    principal_id: d.principal_id,
                    grant_id: Some(g),
                    purpose: AdmissionPurpose::GeneralRuleDrive
                }],
                vec![]
            )
            .await
            .is_err()
    );
    assert_eq!(view(&s, &o, id).await, before);
    assert_eq!(s.state.store().list_commits(id).await.unwrap().len(), 1);
    let own = acquire(&s, &o, id, h, "Sovereign").await;
    assert!(own > l, "撤销后所有者能经原租约管理器收回，不重置fence");
    s.shutdown().await;
}
#[tokio::test]
async fn general_driver_requires_scopes_active_human_and_exact_world_binding() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let o = register(&s.base_url(), "driver-scope-owner", "human", 149).await;
    let d = register(&s.base_url(), "driver-scope-guest", "human", 150).await;
    let agent = register(&s.base_url(), "driver-scope-agent", "agent", 151).await;
    let id = create(&s, &o, &rules()).await;
    let other = create(&s, &o, &rules()).await;
    let g = driver_change(&s, &o, id, &d, None, true).await;
    let token = s.state.inner.tokens.write().unwrap().issue(
        d.principal_id,
        rynvek_protocol::CapabilityScopeSet::default()
            .with(CapabilityScope::WorldRead)
            .with(CapabilityScope::WorldConnect),
        rynvek_server::ids::unix_now_s(),
        60,
    );
    let limited = User {
        principal_id: d.principal_id,
        token: rynvek_server::app::encode_token_text(&token),
        key: server_key(),
    };
    assert_eq!(driver_request(&s, &limited, id, g, None).await.status, 200);
    assert_eq!(
        driver_request(&s, &limited, id, g, Some(json!({"action":"connect"})))
            .await
            .status,
        403
    );
    assert_eq!(driver_request(&s, &d, other, g, None).await.status, 404);
    let b = json!({"principal_id":agent.principal_id.to_string(),"expected_grant_id":null,"active":true,"consent":"general-installed-rule-drive/1"});
    assert_eq!(
        client::send_json(
            "POST",
            &format!("{}/api/v1/worlds/{id}/general/drivers", s.base_url()),
            Some(&o.token),
            &b
        )
        .await
        .unwrap()
        .status,
        403
    );
    let bare = create(&s, &o, &physical()).await;
    let bg = driver_change(&s, &o, bare, &d, None, true).await;
    let (bh, bl) = driver_control(&s, &d, bare, bg).await;
    assert_eq!(
        driver_request(
            &s,
            &d,
            bare,
            bg,
            Some(json!({"action":"tick","input":input(&d,bh,bl,0,vec![])}))
        )
        .await
        .status,
        403
    );
    assert_eq!(
        view(&s, &o, bare).await["commit"]["tick"],
        0,
        "规则目的不偷授无规则物理推进"
    );
    s.state.store().tombstone_principal(d.principal_id).await.unwrap();
    assert_ne!(driver_request(&s, &d, id, g, None).await.status, 200);
    assert_eq!(view(&s, &o, id).await["commit"]["tick"], 0);
    s.shutdown().await;
}
#[tokio::test]
async fn general_driver_same_grant_private_recovery_keeps_receipt_and_double_fences() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let o = register(&s.base_url(), "driver-restore-owner", "human", 152).await;
    let d = register(&s.base_url(), "driver-restore-guest", "human", 153).await;
    let id = create(&s, &o, &rules()).await;
    let g = driver_change(&s, &o, id, &d, None, true).await;
    let (h, l) = driver_control(&s, &d, id, g).await;
    let mut i = input(&d, h, l, 0, vec![]);
    driver_ok(&s, &d, id, g, json!({"action":"tick","input":i})).await;
    let expected = view(&s, &o, id).await;
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(view(&s, &o, id).await, expected);
    let (h2, l2) = driver_control(&s, &d, id, g).await;
    assert!(h2 > h);
    assert_ne!(
        driver_request(&s, &d, id, g, Some(json!({"action":"tick","input":i})))
            .await
            .status,
        200
    );
    i.host_fence = h2;
    i.control_fence = l2;
    assert_eq!(
        driver_ok(&s, &d, id, g, json!({"action":"tick","input":i})).await["duplicate"],
        true
    );
    assert_eq!(view(&s, &o, id).await, expected);
    s.shutdown().await;
}

async fn actual_driver_sdk(payload: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("node")
            .arg("packages/agent-sdk/test/general-driver-e2e.mjs")
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
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    })
    .await
    .unwrap()
}
#[tokio::test]
#[ignore = "真实跨Node SDK/原Engine帧校验：先构建agent-sdk和engine，再显式--include-ignored运行"]
async fn actual_ts_driver_sdk_paid_rules_private_restore_revoke_regrant_and_original_engine_frame() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let o = register(&s.base_url(), "driver-sdk-owner", "human", 156).await;
    let d = register(&s.base_url(), "driver-sdk-guest", "human", 157).await;
    let id = create(&s, &o, &rules()).await;
    let previous=actual_driver_sdk(json!({"phase":"start","base":s.base_url(),"world":id.to_string(),"driverId":d.principal_id.to_string(),"ownerToken":o.token,"driverToken":d.token})).await;
    let expected = view(&s, &o, id).await;
    assert_eq!(expected["commit"]["tick"], 3);
    assert_eq!(expected["commit"]["state_root"], previous["root"]);
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(view(&s, &o, id).await, expected);
    let end=actual_driver_sdk(json!({"phase":"restore","base":s.base_url(),"world":id.to_string(),"driverId":d.principal_id.to_string(),"ownerToken":o.token,"driverToken":d.token,"previous":previous})).await;
    let result = view(&s, &o, id).await;
    assert_eq!(result["commit"]["tick"], 4);
    assert_eq!(result["commit"]["state_root"], end["root"]);
    assert_eq!(
        result["life"]["universe_energy_q"],
        expected["life"]["universe_energy_q"]
    );
    s.shutdown().await;
}

#[tokio::test]
#[ignore = "真实协作Web控制器HTTP：先tsc构建web-platform，显式--include-ignored；非浏览器/PG进程恢复"]
async fn actual_general_collaboration_web_purpose_minimal_frame_paid_ticks_revoke_and_explicit_regrant() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Store::memory(), dir.path()).await;
    let o = register(&s.base_url(), "driver-web-owner", "human", 158).await;
    let d = register(&s.base_url(), "driver-web-guest", "human", 159).await;
    let id = create(&s, &o, &rules()).await;
    let payload = json!({"base":s.base_url(),"world":id.to_string(),"driverId":d.principal_id.to_string(),"ownerToken":o.token,"driverToken":d.token});
    let result: Value = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("node")
            .arg("apps/web-platform/test/general-driver-http-e2e.mjs")
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
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    })
    .await
    .unwrap();
    let after = view(&s, &o, id).await;
    assert_eq!(after["commit"]["tick"], 4);
    assert_eq!(after["commit"]["state_root"], result["root"]);
    assert!(result["paid"].as_u64().unwrap() > 0);
    assert_eq!(after["life"]["universe_energy_q"], after["life"]["genesis_energy_q"]);
    s.shutdown().await;
}
