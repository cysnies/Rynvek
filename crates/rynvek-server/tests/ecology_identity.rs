// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 真实HTTP权限、既有Store原子提交、历史CAS/检查点和TickRecord恢复。
mod common;
use common::*;
use rynvek_protocol::WireUuid;
use rynvek_rts::world::ecology::{EcologyContract, RoleEnergyContract};
use rynvek_rts::{ArenaRuntime, ArenaWorld, MatchConfig};
use rynvek_security::Visibility;
use rynvek_server::store::{CommitRow, WorldRow};
use rynvek_server::transactions::{TickCommitter, TickRecord, restore_committed};
use rynvek_server::{RunningServer, client};
use serde_json::json;

async fn seed_current(s: &RunningServer, u: &User) -> (WorldRow, MatchConfig) {
    let mut expected = MatchConfig::pve(57, 1);
    expected.ecology = Some(EcologyContract::single(
        rynvek_rts::UnitKind::Forager,
        RoleEnergyContract {
            basal_cost_micro_per_tick: 1000,
            reserve_ticks: 4,
            birth_energy_milli: 1000,
            movement_cost_micro_per_tile: 0,
            synapse_maintenance_micro_per_tick: 0,
            natural_reproduction: true,
            attack_cost_micro_per_shot: 0,
            repair_cost_micro_per_milli: 0,
            secretion_cost_micro_per_field_raw: 0,
            birth_synthesis_milli: 0,
            activity_reserve_milli: 0,
            replication_mutation_rate_permille: 50,
        },
    ));
    let world = ArenaWorld::new(expected.clone());
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
        title: "当前统一契约世界".into(),
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
        format: "rynvek-tick/2".into(),
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
async fn current_fork_preserves_permissions_atomicity_private_state_and_wal() {
    let s = start_server(false).await;
    let base = s.base_url();
    let owner = register(&base, "identity-owner", "human", 111).await;
    let other = register(&base, "identity-other", "human", 112).await;
    let (source, config) = seed_current(&s, &owner).await;
    let source_cp = restored(&s, &source).await.checkpoint().unwrap();
    let url = format!("{base}/api/v1/worlds/{}/fork", source.world_id);
    let req = json!({"world_revision": source.latest_revision});
    let denied = client::send_json("POST", &url, Some(&other.token), &req).await.unwrap();
    assert_eq!(denied.status, 404, "私有分叉仍执行真实权限防探测");
    let wrong = json!({"world_revision":source.latest_revision, "expected_config":config});
    let rejected = client::send_json("POST", &url, Some(&owner.token), &wrong)
        .await
        .unwrap();
    assert_eq!(rejected.status, 400, "普通分叉不接受覆盖契约或历史迁移参数");
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
        "来源日志不能混入分叉后的新根链"
    );
    assert_eq!(migrated.config.ecology, config.ecology);
    assert_eq!(migrated.tick, source.latest_tick);
    assert_ne!(row.latest_state_root, source.latest_state_root);
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
async fn current_wal_rejects_other_contracts_and_replays_committed_suffix() {
    let s = start_server(false).await;
    let u = register(&s.base_url(), "current-replay", "human", 113).await;
    let (source, config) = seed_current(&s, &u).await;
    let old = restored(&s, &source).await;
    let (candidate, record) = next_record(&old);
    assert_eq!(record.replay(&old).unwrap().state_root(), candidate.state_root());
    for variant in 0..4 {
        let mut wrong = config.clone();
        match variant {
            0 => wrong.ecology = None,
            1 => {
                wrong
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .get_mut(&rynvek_rts::UnitKind::Forager)
                    .unwrap()
                    .basal_cost_micro_per_tick += 1
            }
            2 => {
                wrong
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .get_mut(&rynvek_rts::UnitKind::Forager)
                    .unwrap()
                    .reserve_ticks += 1
            }
            _ => {
                wrong
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .get_mut(&rynvek_rts::UnitKind::Forager)
                    .unwrap()
                    .birth_energy_milli += 1
            }
        }
        let other = ArenaRuntime::new(source.world_id.0, ArenaWorld::new(wrong));
        assert!(record.replay(&other).is_err(), "空输入也不能跨契约混入WAL");
    }
    let token = s
        .state
        .store()
        .claim_world(source.world_id, 0, 1, "current-host".into())
        .await
        .unwrap();
    let committer = TickCommitter {
        store: s.state.store().clone(),
        snapshots: s.state.inner.world_snapshots.clone(),
        world_id: source.world_id,
        fencing_token: token,
    };
    committer.persist(&candidate, &record).await.unwrap();
    let latest = s.state.store().get_world(source.world_id).await.unwrap().unwrap();
    assert_eq!(restored(&s, &latest).await.state_root(), candidate.state_root());
    assert!(
        committer.persist(&candidate, &record).await.is_err(),
        "重复提交不能多扣费或增加第二修订"
    );
    assert_eq!(
        s.state.store().get_world(source.world_id).await.unwrap().unwrap(),
        latest
    );
    s.shutdown().await;
}
