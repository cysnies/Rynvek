// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 真实HTTP权限、既有Store原子提交、历史CAS/检查点和TickRecord恢复。
mod common;
use common::*;
use rynvek_core::Blake3Hash;
use rynvek_protocol::WireUuid;
use rynvek_rts::world::ecology::EcologyVersion;
use rynvek_rts::{ArenaRuntime, ArenaWorld, MatchConfig};
use rynvek_security::Visibility;
use rynvek_server::store::{CommitRow, WorldRow};
use rynvek_server::transactions::{TickCommitter, TickRecord, restore_committed};
use rynvek_server::{RunningServer, client};
use serde_json::json;

async fn seed_legacy(s: &RunningServer, u: &User, v2: bool, mislabel: bool) -> (WorldRow, MatchConfig) {
    let text = if v2 {
        include_str!("../../../tests/arena/forager-budget-v2.json")
    } else {
        include_str!("../../../tests/arena/energy-exchange-v1.json")
    };
    let f: serde_json::Value = serde_json::from_str(text).unwrap();
    let bytes: Vec<u8> = f["snapshot_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|x| u8::from_str_radix(std::str::from_utf8(x).unwrap(), 16).unwrap())
        .collect();
    let mut expected = MatchConfig::pve(57, 1);
    expected.ecology = Some(if v2 {
        EcologyVersion::ForagerBudgetV2
    } else {
        EcologyVersion::EnergyExchangeV1
    });
    let mut world = ArenaWorld::decode_snapshot_with_config(&bytes, &expected).unwrap();
    let original_root = world.state_hash();
    if mislabel {
        world.config.ecology = Some(if v2 {
            EcologyVersion::EnergyExchangeV1
        } else {
            EcologyVersion::ForagerBudgetV2
        });
        assert_eq!(world.state_hash(), original_root, "明确复现legacy模式根碰撞");
    }
    let id = rynvek_server::ids::new_uuid_v7();
    let runtime = ArenaRuntime::new(id.0, world);
    let cp = runtime.checkpoint().unwrap();
    let snap = s.state.inner.world_snapshots.put(&cp.bytes).unwrap();
    let genesis = s
        .state
        .inner
        .world_snapshots
        .put(&serde_json::to_vec(&json!({"config":expected})).unwrap())
        .unwrap();
    let row = WorldRow {
        world_id: id,
        org_id: u.principal_id,
        owner_principal_id: u.principal_id,
        title: "冻结旧生态世界".into(),
        world_profile: "RTS_Standard".into(),
        game_mode: "PvE_Survival".into(),
        execution_profile: "D0_Strict".into(),
        visibility: Visibility::Private,
        parent_world_id: None,
        forked_at_revision: None,
        latest_tick: cp.tick,
        latest_revision: 0,
        latest_state_root: cp.state_root,
        lease_holder_node: None,
        fencing_token: 1,
    };
    s.state
        .store()
        .insert_world(
            row.clone(),
            CommitRow {
                world_id: id,
                world_revision: 0,
                tick: cp.tick,
                parent_revision: None,
                state_root_blake3: cp.state_root,
                journal_segment_cid: genesis.0,
                snapshot_cid: Some(snap.0),
            },
        )
        .await
        .unwrap();
    (row, expected)
}

async fn restored(s: &RunningServer, row: &WorldRow) -> ArenaRuntime {
    restore_committed(s.state.store(), s.state.inner.world_snapshots.as_ref(), row)
        .await
        .unwrap()
}
fn next_record(runtime: &ArenaRuntime) -> (ArenaRuntime, TickRecord) {
    let mut c = runtime.candidate_tick().unwrap();
    c.finish_tick().unwrap();
    let r = TickRecord {
        format: "rynvek-tick/1".into(),
        base_revision: runtime.revision(),
        base_root: runtime.state_root(),
        target_tick: runtime.tick + 1,
        tick: c.tick,
        revision: c.revision(),
        state_root: c.state_root(),
        inputs: vec![],
    };
    (c, r)
}

#[tokio::test]
async fn migration_requires_permissions_and_publishes_only_atomic_auditable_private_fork() {
    let s = start_server(false).await;
    let base = s.base_url();
    let owner = register(&base, "identity-owner", "human", 111).await;
    let other = register(&base, "identity-other", "human", 112).await;
    let (source, config) = seed_legacy(&s, &owner, false, false).await;
    let source_cp = restored(&s, &source).await.checkpoint().unwrap();
    let url = format!("{base}/api/v1/worlds/{}/migrate-ecology-identity", source.world_id);
    let req =
        json!({"base_revision":source.latest_revision,"base_root":source.latest_state_root,"expected_config":config});
    let denied = client::send_json("POST", &url, Some(&other.token), &req).await.unwrap();
    assert_eq!(denied.status, 404, "复用既有私有世界防探测策略，不泄漏资源存在性");
    let mut stale = req.clone();
    stale["base_root"][0] = json!(source.latest_state_root[0] ^ 1);
    let rejected = client::send_json("POST", &url, Some(&owner.token), &stale)
        .await
        .unwrap();
    assert_eq!(rejected.status, 409);
    let mut wrong = req.clone();
    wrong["expected_config"]["ecology"] = json!("forager_budget_v2");
    let rejected = client::send_json("POST", &url, Some(&owner.token), &wrong)
        .await
        .unwrap();
    assert_eq!(rejected.status, 409);
    assert_eq!(
        s.state
            .store()
            .list_worlds(Some(owner.principal_id), 100, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.state.store().get_world(source.world_id).await.unwrap().unwrap(),
        source
    );
    let made = client::send_json("POST", &url, Some(&owner.token), &req).await.unwrap();
    assert_eq!(made.status, 201, "{:?}", made.json());
    let id: WireUuid = made.json().unwrap()["world_id"].as_str().unwrap().parse().unwrap();
    let row = s.state.store().get_world(id).await.unwrap().unwrap();
    assert_eq!(row.visibility, Visibility::Private);
    assert_eq!(row.parent_world_id, Some(source.world_id));
    assert_eq!(row.forked_at_revision, Some(source.latest_revision));
    let mut migrated = restored(&s, &row).await;
    let (_, legacy_record) = next_record(&restored(&s, &source).await);
    assert!(
        legacy_record.replay(&migrated).is_err(),
        "旧来源日志不能混入迁移后的新根链"
    );
    assert!(migrated.config.ecology_identity.is_some());
    assert_eq!(migrated.config.ecology, config.ecology);
    assert_eq!(migrated.tick, source.latest_tick);
    assert_ne!(row.latest_state_root, source.latest_state_root);
    let commit = s.state.store().list_commits(id).await.unwrap().pop().unwrap();
    let audit: serde_json::Value = serde_json::from_slice(
        &s.state
            .inner
            .world_snapshots
            .get(&Blake3Hash(commit.journal_segment_cid))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(audit["format"], "rynvek-ecology-identity-migration/1");
    assert_eq!(audit["source"]["config"], serde_json::to_value(&config).unwrap());
    assert_eq!(
        audit["source"]["state_root"],
        rynvek_server::ids::hex(&source.latest_state_root)
    );
    // 新分支继续走原真实WAL提交和恢复，不能只产出一次性导出文件。
    let token = s
        .state
        .store()
        .claim_world(id, 0, row.fencing_token, "migration-test".into())
        .await
        .unwrap();
    let committer = TickCommitter {
        store: s.state.store().clone(),
        snapshots: s.state.inner.world_snapshots.clone(),
        world_id: id,
        fencing_token: token,
    };
    for _ in 0..3 {
        let (c, r) = next_record(&migrated);
        committer.persist(&c, &r).await.unwrap();
        migrated = c;
    }
    let latest = s.state.store().get_world(id).await.unwrap().unwrap();
    assert_eq!(restored(&s, &latest).await.state_root(), migrated.state_root());
    // 填满既有真实组织配额，再触发insert_world事务拒绝；不添加测试专用故障状态。
    let quota = s
        .state
        .store()
        .get_org(owner.principal_id)
        .await
        .unwrap()
        .unwrap()
        .quota_max_worlds as usize;
    while s
        .state
        .store()
        .list_worlds(Some(owner.principal_id), 100, 0)
        .await
        .unwrap()
        .len()
        < quota
    {
        let r = client::send_json("POST", &url, Some(&owner.token), &req).await.unwrap();
        assert_eq!(r.status, 201);
    }
    let failed = client::send_json("POST", &url, Some(&owner.token), &req).await.unwrap();
    assert!(failed.status >= 400);
    assert_eq!(failed.json().unwrap()["code"], 0x2004);
    assert_eq!(
        s.state
            .store()
            .list_worlds(Some(owner.principal_id), 100, 0)
            .await
            .unwrap()
            .len(),
        quota
    );
    assert_eq!(
        s.state.store().get_world(source.world_id).await.unwrap().unwrap(),
        source
    );
    assert_eq!(s.state.store().list_commits(source.world_id).await.unwrap().len(), 1);
    assert_eq!(restored(&s, &source).await.checkpoint().unwrap().bytes, source_cp.bytes);
    s.shutdown().await;
}

#[tokio::test]
async fn replay_requires_legacy_context_and_rejects_cross_bound_mode() {
    let s = start_server(false).await;
    let base = s.base_url();
    let u = register(&base, "identity-replay", "human", 113).await;
    for v2 in [false, true] {
        let (source, config) = seed_legacy(&s, &u, v2, false).await;
        let old = restored(&s, &source).await;
        let (candidate, record) = next_record(&old);
        assert!(record.replay(&old).is_err());
        assert_eq!(
            record.replay_with_config(&old, &config).unwrap().state_root(),
            candidate.state_root()
        );
        let mut wrong = config.clone();
        wrong.ecology = Some(if v2 {
            EcologyVersion::EnergyExchangeV1
        } else {
            EcologyVersion::ForagerBudgetV2
        });
        assert!(record.replay_with_config(&old, &wrong).is_err());
        let token = s
            .state
            .store()
            .claim_world(source.world_id, 0, 1, "legacy-host".into())
            .await
            .unwrap();
        TickCommitter {
            store: s.state.store().clone(),
            snapshots: s.state.inner.world_snapshots.clone(),
            world_id: source.world_id,
            fencing_token: token,
        }
        .persist(&candidate, &record)
        .await
        .unwrap();
        let latest = s.state.store().get_world(source.world_id).await.unwrap().unwrap();
        assert_eq!(restored(&s, &latest).await.state_root(), candidate.state_root());
        let url = format!("{base}/api/v1/worlds/{}/migrate-ecology-identity", source.world_id);
        let r=client::send_json("POST",&url,Some(&u.token),&json!({"base_revision":latest.latest_revision,"base_root":latest.latest_state_root,"expected_config":config})).await.unwrap();
        assert_eq!(r.status, 409, "不得旁路活动宿主");
        let a = ArenaRuntime::new([117; 16], ArenaWorld::new(config));
        let b = ArenaRuntime::new([117; 16], ArenaWorld::new(wrong));
        let (_, r) = next_record(&a);
        assert!(r.replay(&b).is_err(), "新记录跨模式即使输入为空也必须拒绝");
    }
    s.shutdown().await;
}

#[tokio::test]
async fn trusted_genesis_pin_rejects_legacy_mode_collision_without_reinterpreting_snapshot() {
    let s = start_server(false).await;
    let u = register(&s.base_url(), "identity-collision", "human", 114).await;
    for v2 in [false, true] {
        let (row, _) = seed_legacy(&s, &u, v2, true).await;
        assert!(
            restore_committed(s.state.store(), s.state.inner.world_snapshots.as_ref(), &row)
                .await
                .is_err()
        );
    }
    s.shutdown().await;
}
