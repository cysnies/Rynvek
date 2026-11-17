// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 真实 TCP HTTP 与活动 Actor：排队、提交后确认、冲突、私有检查点、恢复和有序回放。
mod common;
use common::*;
use rynvek_core::Blake3Hash;
use rynvek_protocol::{WireChannelId, WireUuid};
use rynvek_rts::{ArenaRuntime, MatchConfig};
use rynvek_security::{Keyring, League, PrincipalKindCode};
use rynvek_server::host::{MatchHandle, MatchSpec, SeatSpec};
use rynvek_server::store::{MatchRow, MatchState, ParticipantRow};
use rynvek_server::transactions::TickCommitter;
use rynvek_server::{RunningServer, client};
use rynvek_world::{WorldPatch, WorldPatchOp};
use serde_json::json;
use std::time::Duration;

struct Fixture {
    server: RunningServer,
    owner: User,
    other: User,
    host: MatchHandle,
    a: StreamClient,
    _b: StreamClient,
}

impl Fixture {
    async fn new() -> Self {
        let server = start_server(false).await;
        let base = server.base_url();
        let owner = register(&base, "editor-owner", "human", 71).await;
        let other = register(&base, "editor-other", "human", 72).await;
        let created = client::send_json(
            "POST",
            &format!("{base}/api/v1/worlds"),
            Some(&owner.token),
            &json!({"title":"活动事务回归", "template":"arena_duel", "map_seed":42,"visibility":"Public"}),
        )
        .await
        .unwrap();
        assert_eq!(created.status, 201);
        let world_id: WireUuid = created.json().unwrap()["world_id"].as_str().unwrap().parse().unwrap();
        let match_id = rynvek_server::ids::new_uuid_v7();
        server
            .state
            .store()
            .insert_match(
                MatchRow {
                    match_id,
                    world_id,
                    game_mode: "PvP_Custom".into(),
                    ruleset_blake3: rynvek_server::app::arena_ruleset_hash(),
                    map_seed: 42,
                    match_state: MatchState::Created,
                    winner_faction_id: None,
                    win_reason: None,
                    total_ticks: 0,
                    replay_cid_blake3: None,
                    started_at_unix_s: None,
                    completed_at_unix_s: None,
                },
                [&owner, &other]
                    .iter()
                    .enumerate()
                    .map(|(i, u)| ParticipantRow {
                        match_id,
                        faction_id: i as i16,
                        principal_id: u.principal_id,
                        controller_league: League::Human,
                        final_score: 0,
                        rating_delta: None,
                    })
                    .collect(),
            )
            .await
            .unwrap();
        let spec = MatchSpec {
            custom: false,
            match_id,
            world_id,
            league: League::Human,
            ruleset_hash: rynvek_server::app::arena_ruleset_hash(),
            config: MatchConfig::duel(42),
            seats: [&owner, &other]
                .iter()
                .enumerate()
                .map(|(i, u)| SeatSpec {
                    faction: i as u8,
                    principal_id: u.principal_id,
                    kind: PrincipalKindCode::Human,
                })
                .collect(),
        };
        let host = server.state.attach_world_host(spec).await.unwrap();
        let url = format!(
            "{}/api/v1/worlds/{world_id}/stream",
            base.replacen("http://", "ws://", 1)
        );
        let mut a = StreamClient::new(
            rynvek_network::websocket::connect(&url).await.unwrap(),
            owner.principal_id,
        );
        let mut b = StreamClient::new(
            rynvek_network::websocket::connect(&url).await.unwrap(),
            other.principal_id,
        );
        a.hello(owner.token_bytes(), world_id, 0).await;
        b.hello(other.token_bytes(), world_id, 1).await;
        let _ = a.recv_on(WireChannelId::Control, |_| {}).await;
        let _ = b.recv_on(WireChannelId::Control, |_| {}).await;
        tokio::time::timeout(
            Duration::from_secs(10),
            host.wait_for(|s| s.state == MatchState::Running),
        )
        .await
        .unwrap();
        Self {
            server,
            owner,
            other,
            host,
            a,
            _b: b,
        }
    }

    fn post(&self, patch: &WorldPatch) -> tokio::task::JoinHandle<client::HttpResponse> {
        let url = format!(
            "{}/api/v1/worlds/{}/patches",
            self.server.base_url(),
            self.host.world_id
        );
        let token = self.owner.token.clone();
        let body = serde_json::to_value(patch).unwrap();
        tokio::spawn(async move { client::send_json("POST", &url, Some(&token), &body).await.unwrap() })
    }

    async fn queued(&self, count: usize) {
        tokio::time::timeout(
            Duration::from_secs(10),
            self.host.wait_for(|s| s.pending_inputs >= count),
        )
        .await
        .unwrap();
    }

    async fn runtime(&self) -> ArenaRuntime {
        let row = self
            .server
            .state
            .store()
            .get_world(self.host.world_id)
            .await
            .unwrap()
            .unwrap();
        rynvek_server::transactions::restore_committed(
            self.server.state.store(),
            self.server.state.inner.world_snapshots.as_ref(),
            &row,
        )
        .await
        .unwrap()
    }
}

fn patch(id: u8, base: u64, x: i32) -> WorldPatch {
    serde_json::from_value(json!({"patch_id":vec![id;16],"base_authoring_revision":base,
        "hlc":{"wall_time_ms":1,"logical_counter":0,"author_node_id":1},"safety_class":"TickBoundarySafe",
        "operations":[{"PaintTerrainRect":{"x0":x,"y0":100,"width":1,"height":1,"tile_kind":4}}]}))
    .unwrap()
}

#[tokio::test]
async fn active_http_queue_commit_conflict_restore_and_replay() {
    let f = Fixture::new().await;
    let mut engine = f.runtime().await;
    let initial = engine.checkpoint().unwrap();
    let input = patch(1, 0, 100);
    let url = format!("{}/api/v1/worlds/{}/patches", f.server.base_url(), f.host.world_id);
    let forbidden = client::send_json(
        "POST",
        &url,
        Some(&f.other.token),
        &serde_json::to_value(&input).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(forbidden.status, 403);
    assert_eq!(f.host.status().tick, 0);
    let request = f.post(&input);
    f.queued(1).await;
    assert!(!request.is_finished(), "排队不能伪报 applied");
    assert_eq!(f.runtime().await.revision(), 0);
    f.host.advance(1).await.unwrap();
    let response = request.await.unwrap();
    assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
    assert_eq!(response.json().unwrap()["effective_tick"], 1);
    engine
        .transact_authoring_at(1, |w| input.apply_at_boundary(w, 1))
        .unwrap();
    assert_eq!(
        engine.state_root(),
        f.host.status().state_root,
        "HTTP 创作路径和活动宿主同 Tick 同根"
    );
    let duplicate = f.post(&input).await.unwrap();
    assert_eq!(duplicate.status, 200);
    assert_eq!(duplicate.json().unwrap(), response.json().unwrap());
    assert_eq!(f.host.status().revision, 1);
    let mut collision = input.clone();
    collision.operations = patch(1, 0, 101).operations;
    assert_eq!(f.post(&collision).await.unwrap().status, 409);
    let one = f.post(&patch(2, 1, 101));
    let two = f.post(&patch(3, 1, 102));
    f.queued(2).await;
    f.host.advance(1).await.unwrap();
    let mut statuses = vec![one.await.unwrap().status, two.await.unwrap().status];
    statuses.sort_unstable();
    assert_eq!(statuses, vec![200, 409]);
    let mut clean = f.runtime().await;
    clean.step().unwrap();
    let mut invalid = patch(4, 2, 103);
    invalid.operations.push(WorldPatchOp::SetWorldParam {
        key_path: "field.diffusion".into(),
        new_value_json: "1".into(),
    });
    let request = f.post(&invalid);
    f.queued(1).await;
    f.host.advance(1).await.unwrap();
    assert!(request.await.unwrap().status >= 400);
    assert_eq!(
        f.host.status().state_root,
        clean.state_root(),
        "失败补丁没有留下任何领域修改，仅正常推进 Tick"
    );
    let restored = f.runtime().await;
    let mut continuation = restored;
    continuation.step().unwrap();
    f.host.advance(1).await.unwrap();
    assert_eq!(continuation.state_root(), f.host.status().state_root);
    let row = f
        .server
        .state
        .store()
        .get_world(f.host.world_id)
        .await
        .unwrap()
        .unwrap();
    let committer = TickCommitter {
        store: f.server.state.store().clone(),
        snapshots: f.server.state.inner.world_snapshots.clone(),
        world_id: f.host.world_id,
        fencing_token: row.fencing_token,
    };
    let records = committer.records().await.unwrap();
    assert_eq!(records.len(), 4);
    let mut replayed = ArenaRuntime::restore(f.host.world_id.0, 0, initial.state_root, &initial.bytes).unwrap();
    for record in &records {
        replayed = record.replay(&replayed).unwrap();
    }
    assert_eq!(replayed.state_root(), continuation.state_root());
    let base = ArenaRuntime::restore(f.host.world_id.0, 0, initial.state_root, &initial.bytes).unwrap();
    let mut corrupt = records[0].clone();
    corrupt.state_root[0] ^= 1;
    assert!(corrupt.replay(&base).is_err());
    assert_eq!(base.state_root(), initial.state_root);
    let receipt = committer.patch_receipt(&input).await.unwrap().unwrap();
    assert_eq!(receipt.revision, 1, "幂等回执来自持久化日志，而不是临时队列");
    let commits = f.server.state.store().list_commits(f.host.world_id).await.unwrap();
    assert!(commits[1].snapshot_cid.is_none(), "普通 Tick 不应制造全量快照风暴");
    let cid = Blake3Hash(commits.iter().rev().find_map(|c| c.snapshot_cid).unwrap()).to_hex();
    assert_eq!(
        client::get(&format!("{}/api/v1/hub/blobs/{cid}", f.server.base_url()), None)
            .await
            .unwrap()
            .status,
        404
    );
    // v2 将补丁和命令一起按序重放，不能退回只录命令的旧归档。
    let body = rynvek_server::replay::record_runtime(
        &replayed,
        rynvek_server::replay::RuntimeReplay {
            world_id: f.host.world_id.0,
            initial_revision: 0,
            initial_root: initial.state_root,
            initial_snapshot: initial.bytes,
            ticks: records,
        },
        f.host.match_id.to_string(),
        f.host.world_id.to_string(),
        ruleset_hex(),
        Vec::new(),
    )
    .unwrap();
    let bytes = rynvek_server::replay::encode(&body, f.host.world_id.0, &server_key()).unwrap();
    let mut keys = Keyring::new();
    keys.insert(SERVER_KEY_ID, server_key().public_key()).unwrap();
    assert_eq!(
        rynvek_server::replay::verify(&bytes, &keys).unwrap().final_state_root,
        cid_root(&replayed)
    );
    f.host.abort("用例结束").await;
    f.server.shutdown().await;
}

fn cid_root(runtime: &ArenaRuntime) -> String {
    rynvek_server::ids::hex(&runtime.state_root())
}

#[tokio::test]
async fn failed_fencing_commit_publishes_neither_patch_nor_tick() {
    let f = Fixture::new().await;
    let before = f.host.status();
    let request = f.post(&patch(9, 0, 100));
    f.queued(1).await;
    let row = f
        .server
        .state
        .store()
        .get_world(f.host.world_id)
        .await
        .unwrap()
        .unwrap();
    f.server
        .state
        .store()
        .claim_world(
            row.world_id,
            row.latest_revision,
            row.fencing_token,
            "另一个宿主".into(),
        )
        .await
        .unwrap();
    assert!(f.host.advance(1).await.is_err());
    assert_eq!(request.await.unwrap().status, 409);
    assert_eq!(f.host.status().state, MatchState::Aborted);
    assert_eq!(f.host.status().state_root, before.state_root);
    assert_eq!(f.runtime().await.revision(), 0);
    assert_eq!(
        f.server.state.store().list_commits(row.world_id).await.unwrap().len(),
        1
    );
    f.server.shutdown().await;
}

#[tokio::test]
async fn commands_and_patches_share_ordered_tick_and_signed_completion() {
    let mut f = Fixture::new().await;
    let mut engine = f.runtime().await.candidate_tick().unwrap();
    let lease =
        f.a.command(0, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"}))
            .await;
    let token = lease["lease"]["fencing_token"].as_u64().unwrap();
    let key = rynvek_protocol::IdempotencyKey::new(rynvek_server::ids::new_uuid_v7()).unwrap();
    let command = rynvek_rts::commands::RtsCommand::IssueOrder {
        units: rynvek_rts::commands::UnitSelector::Archetype(None),
        order: rynvek_rts::OrderKind::HoldPosition,
        target: rynvek_rts::Vec2::from_int(20, 20),
    };
    let env = rynvek_protocol::CommandEnvelope {
        idempotency_key: key,
        principal_id: f.owner.principal_id,
        issued_at_tick: 0,
        target_tick: 1,
        payload: json!({"type":"command","seq":2,"fencing_token":token,"command":command.clone()}),
    };
    engine.command(rynvek_rts::FactionId(0), &command).unwrap();
    let paint = patch(11, 0, 100);
    engine
        .transact_authoring_at(0, |world| paint.apply_at_boundary(world, 1))
        .unwrap();
    engine.finish_tick().unwrap();
    let bytes = serde_json::to_vec(&env).unwrap();
    f.a.send(WireChannelId::Command, &bytes).await;
    f.queued(1).await;
    let pending = f.post(&patch(11, 0, 100));
    f.queued(2).await;
    assert_eq!(f.runtime().await.revision(), 0);
    f.host.advance(1).await.unwrap();
    let frame = f.a.recv_on(WireChannelId::Command, |_| {}).await;
    let reply: serde_json::Value = serde_json::from_slice(&frame.decoded_payload().unwrap()).unwrap();
    assert_eq!(reply["idempotency_key"], key.to_string());
    assert_eq!(reply["message"]["type"], "ack");
    assert!(reply["message"].get("error").is_none(), "{reply}");
    assert_eq!(reply["message"]["tick"], 1);
    assert_eq!(pending.await.unwrap().status, 200);
    assert_eq!(
        engine.state_root(),
        f.host.status().state_root,
        "引擎同序命令与补丁必须产生相同规范根"
    );
    let row = f
        .server
        .state
        .store()
        .get_world(f.host.world_id)
        .await
        .unwrap()
        .unwrap();
    let committer = TickCommitter {
        store: f.server.state.store().clone(),
        snapshots: f.server.state.inner.world_snapshots.clone(),
        world_id: row.world_id,
        fencing_token: row.fencing_token,
    };
    let records = committer.records().await.unwrap();
    assert_eq!(records.len(), 1);
    assert!(
        matches!(&records[0].inputs[..], [rynvek_server::transactions::TickInput::Command { idempotency_key, .. },
        rynvek_server::transactions::TickInput::Patch { .. }] if idempotency_key == &key.to_string())
    );
    f.a.send(WireChannelId::Command, &bytes).await;
    let frame = f.a.recv_on(WireChannelId::Command, |_| {}).await;
    let reply: serde_json::Value = serde_json::from_slice(&frame.decoded_payload().unwrap()).unwrap();
    assert_eq!(reply["message"]["type"], "error");
    assert_eq!(f.host.status().pending_inputs, 0);
    assert_eq!(f.host.status().revision, 1);
    let handle = f.host.clone();
    let (ack, _) = tokio::join!(
        f.a.command(
            1,
            json!({"type":"command","seq":3,"fencing_token":token,"command":{"type":"surrender"}})
        ),
        async {
            tokio::time::timeout(Duration::from_secs(10), handle.wait_for(|s| s.pending_inputs > 0))
                .await
                .unwrap();
            handle.advance(1).await.unwrap();
        }
    );
    assert_eq!(ack["type"], "ack");
    assert!(ack.get("error").is_none());
    assert_eq!(f.host.status().state, MatchState::Completed);
    let base = f.server.base_url();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let detail = client::get(
                &format!("{base}/api/v1/matches/{}", f.host.match_id),
                Some(&f.owner.token),
            )
            .await
            .unwrap()
            .json()
            .unwrap();
            if detail["match_state"] == "Completed" && !detail["replay_cid_blake3"].is_null() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let archive = client::get(
        &format!("{base}/api/v1/matches/{}/replay", f.host.match_id),
        Some(&f.owner.token),
    )
    .await
    .unwrap();
    assert_eq!(archive.status, 200);
    let mut keys = Keyring::new();
    keys.insert(SERVER_KEY_ID, server_key().public_key()).unwrap();
    let checked = rynvek_server::replay::verify(&archive.body, &keys).unwrap();
    assert_eq!(checked.commands, 2);
    assert_eq!(
        checked.final_state_root,
        rynvek_server::ids::hex(&f.host.status().state_root)
    );
    let final_world = f
        .server
        .state
        .store()
        .get_world(f.host.world_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        final_world.latest_revision,
        f.host.status().revision,
        "终局持久化不能再追加一个无检查点的假权威修订"
    );
    assert_eq!(f.runtime().await.state_root(), f.host.status().state_root);
    assert_eq!(
        f.server
            .state
            .store()
            .get_rating(f.owner.principal_id, League::Human)
            .await
            .unwrap()
            .matches_played,
        0,
        "可创作编辑的自定义世界不能写排位天梯"
    );
    assert_eq!(
        rynvek_server::replay::open(&archive.body, Some(&keys))
            .unwrap()
            .body
            .format,
        "rynvek-replay/2"
    );
    f.server.shutdown().await;
}

#[tokio::test]
async fn periodic_checkpoint_and_wal_suffix_restore_the_same_runtime() {
    let f = Fixture::new().await;
    f.host.advance(301).await.unwrap();
    let commits = f.server.state.store().list_commits(f.host.world_id).await.unwrap();
    assert_eq!(commits.len(), 302);
    assert!(commits[300].snapshot_cid.is_some());
    assert!(commits[301].snapshot_cid.is_none());
    let mut restored = f.runtime().await;
    assert_eq!(restored.state_root(), f.host.status().state_root);
    restored.step().unwrap();
    f.host.advance(1).await.unwrap();
    assert_eq!(restored.state_root(), f.host.status().state_root);
    f.host.abort("周期锚点用例结束").await;
    f.server.shutdown().await;
}

#[tokio::test]
async fn studio_action_rule_enters_real_actor_epoch_and_committed_restore() {
    let f = Fixture::new().await;
    let source=json!({"format":"rynvek.rule-graph.v1","name":"活动动作","writes":[{"component":"unit.actions.move_x","ty":"Q8_8"}],"nodes":[{"op":"ConstQ8_8","imm":"128"},{"op":"EmitComponentWrite","component":"unit.actions.move_x","args":[0]}]}).to_string();
    let module = rynvek_rts::world::authoring::compile_action_rule(&source).unwrap();
    let digest = rynvek_core::Blake3Hash::of(&rynvek_rules::encode_rir(&module)).0;
    let mut input = patch(29, 0, 100);
    input.safety_class = rynvek_world::LiveEditSafetyClass::EpochBoundarySafe;
    input.operations = vec![WorldPatchOp::UpdateRuleModule {
        module_id: "control".into(),
        rule_graph_json: source,
        compiled_rir_blake3: digest,
    }];
    let pending = f.post(&input);
    f.queued(1).await;
    assert!(!pending.is_finished());
    f.host.advance(4).await.unwrap();
    let response = pending.await.unwrap();
    assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
    assert_eq!(response.json().unwrap()["effective_tick"], 4);
    let restored = f.runtime().await;
    assert!(restored.world().authoring.rules.contains_key("control"));
    assert!(restored.world().units.values().all(|u| u.actions[0] == 128));
    let mut deletion = patch(30, f.host.status().revision, 101);
    deletion.safety_class = rynvek_world::LiveEditSafetyClass::EpochBoundarySafe;
    deletion.operations = vec![WorldPatchOp::RemoveRuleModule {
        module_id: "control".into(),
    }];
    let pending = f.post(&deletion);
    f.queued(1).await;
    f.host.advance(3).await.unwrap();
    assert!(!pending.is_finished(), "Epoch之前不能出现假删除回执");
    f.host.advance(1).await.unwrap();
    let response = pending.await.unwrap();
    assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
    let ack = response.json().unwrap();
    assert_eq!(ack["effective_tick"], 8);
    assert_eq!(ack["report"]["rules_removed"], 1);
    let deleted = f.runtime().await; // 从原始检查点 + Tick WAL恢复，非本地镜像
    assert!(deleted.world().authoring.rules.is_empty());
    assert_eq!(deleted.state_root(), f.host.status().state_root);
    // 删除不倒写上一Tick动作；下一次原神经计算不再被模块覆盖。
    f.host.advance(1).await.unwrap();
    assert!(f.runtime().await.world().units.values().any(|u| u.actions[0] != 128));
    let replayed_ack = f.post(&deletion).await.unwrap();
    assert_eq!(replayed_ack.status, 200); // 活动Actor保留原patch_id幂等回执
    assert_eq!(replayed_ack.json().unwrap(), ack);
    let mut stale = deletion.clone();
    stale.patch_id = [32; 16];
    assert_eq!(f.post(&stale).await.unwrap().status, 409);
    let mut restoration = input.clone();
    restoration.patch_id = [31; 16];
    restoration.base_authoring_revision = f.host.status().revision;
    let pending = f.post(&restoration);
    f.queued(1).await;
    f.host.advance(3).await.unwrap();
    assert_eq!(pending.await.unwrap().status, 200);
    let restored_again = f.runtime().await;
    assert_eq!(restored_again.state_root(), f.host.status().state_root);
    assert_eq!(restored_again.world().authoring.rules, restored.world().authoring.rules);
    assert!(restored_again.world().units.values().all(|u| u.actions[0] == 128));
    f.host.abort("短定向用例完成").await;
    f.server.shutdown().await;
}

#[test]
fn ecology_authoring_change_replays_same_wal_without_relaxing_normal_config_freeze() {
    use rynvek_rts::world::ecology::{EcologyContract, RoleEnergyContract};
    use rynvek_server::transactions::{TickInput, TickRecord};
    let mut config = MatchConfig::pve(42, 1);
    let role = RoleEnergyContract {
        basal_cost_micro_per_tick: 100,
        reserve_ticks: 4,
        birth_energy_milli: 1000,
        movement_cost_micro_per_tile: 100,
        synapse_maintenance_micro_per_tick: 1,
        natural_reproduction: true,
        attack_cost_micro_per_shot: 0,
        repair_cost_micro_per_milli: 0,
        secretion_cost_micro_per_field_raw: 0,
        birth_synthesis_milli: 0,
        activity_reserve_milli: 0,
        replication_mutation_rate_permille: 50,
    };
    config.ecology = Some(EcologyContract::single(rynvek_rts::UnitKind::Forager, role));
    let mut before = ArenaRuntime::new([95; 16], rynvek_rts::ArenaWorld::new(config));
    for _ in 0..3 {
        before.step().unwrap();
    }
    let mut input = patch(31, before.revision(), 100);
    input.safety_class = rynvek_world::LiveEditSafetyClass::EpochBoundarySafe;
    let mut next = role;
    next.birth_energy_milli = 1200;
    next.replication_mutation_rate_permille = 0;
    input.operations = vec![WorldPatchOp::SetWorldParam {
        key_path: "ecology.roles.unit.forager".into(),
        new_value_json: serde_json::to_string(&next).unwrap(),
    }];
    let mut ordinary = before.clone();
    assert!(
        ordinary
            .transact_at(3, |w| {
                w.config
                    .ecology
                    .as_mut()
                    .unwrap()
                    .roles
                    .insert(rynvek_rts::UnitKind::Forager, next);
                Ok(())
            })
            .is_err()
    );
    assert_eq!(ordinary.state_root(), before.state_root());
    let mut candidate = before.candidate_tick().unwrap();
    let report = candidate
        .transact_authoring_at(3, |w| input.apply_at_boundary(w, 4))
        .unwrap();
    candidate.finish_tick().unwrap();
    let mut record = TickRecord {
        format: "rynvek-tick/2".into(),
        base_revision: before.revision(),
        base_root: before.state_root(),
        target_tick: 4,
        tick: candidate.tick,
        revision: candidate.revision(),
        state_root: candidate.state_root(),
        inputs: vec![TickInput::Patch { patch: input, report }],
    };
    assert_eq!(record.replay(&before).unwrap().state_root(), candidate.state_root());
    record.format = "rynvek-tick/1".into();
    assert!(record.replay(&before).is_err(), "新回执结构不默默读取旧WAL版本");
}
