// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 真实 TCP、认证、Actor、提交存储及 WAL 恢复；不是独立生态模拟器。
mod common;
use common::*;
use rynvek_protocol::{WireChannelId, WireUuid};
use rynvek_rts::world::ecology::{EcologyContract, RoleEnergyContract};
use rynvek_rts::{ArenaRuntime, MatchConfig};
use rynvek_security::{League, PrincipalKindCode};
use rynvek_server::host::{MatchSpec, SeatSpec};
use rynvek_server::store::{MatchRow, MatchState, ParticipantRow};
use rynvek_server::{RunningServer, client};
use serde_json::json;

async fn restored(server: &RunningServer, world_id: WireUuid) -> ArenaRuntime {
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    rynvek_server::transactions::restore_committed(
        server.state.store(),
        server.state.inner.world_snapshots.as_ref(),
        &row,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn automatic_local_uptake_uses_real_actor_permissions_private_state_and_wal() {
    run_energy_contract(
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
        false,
    )
    .await;
}

#[tokio::test]
async fn explicit_basal_survival_contract_really_kills_fed_foragers_in_actor_and_wal() {
    // 实验数值特意高于单Tick真实摄食量，不冒充v1冻结平衡。
    run_energy_contract(
        RoleEnergyContract {
            basal_cost_micro_per_tick: 2_000_000,
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
        true,
    )
    .await;
}

async fn run_energy_contract(params: RoleEnergyContract, expect_death: bool) {
    let ecology = EcologyContract::single(rynvek_rts::UnitKind::Forager, params);
    let server = start_server(false).await;
    let base = server.base_url();
    let owner = register(&base, "energy-owner", "human", 101).await;
    let url = format!("{base}/api/v1/worlds");
    let forbidden = client::send_json(
        "POST",
        &url,
        Some(&owner.token),
        &json!({"title":"禁止排位换规则","template":"arena_duel","ecology":&ecology}),
    )
    .await
    .unwrap();
    assert_eq!(forbidden.status, 400);
    let created = client::send_json(
        "POST",
        &url,
        Some(&owner.token),
        &json!({"title":"真实能量交换","template":"arena_pve","map_seed":57,"visibility":"Public","ecology":&ecology}),
    )
    .await
    .unwrap();
    assert_eq!(created.status, 201, "{:?}", created.json());
    let world_id: WireUuid = created.json().unwrap()["world_id"].as_str().unwrap().parse().unwrap();
    let match_id = rynvek_server::ids::new_uuid_v7();
    let hash = rynvek_server::app::arena_ruleset_hash();
    server
        .state
        .store()
        .insert_match(
            MatchRow {
                match_id,
                world_id,
                game_mode: "PvE_Survival".into(),
                ruleset_blake3: hash,
                map_seed: 57,
                match_state: MatchState::Created,
                winner_faction_id: None,
                win_reason: None,
                total_ticks: 0,
                replay_cid_blake3: None,
                started_at_unix_s: None,
                completed_at_unix_s: None,
            },
            vec![ParticipantRow {
                match_id,
                faction_id: 0,
                principal_id: owner.principal_id,
                controller_league: League::Human,
                final_score: 0,
                rating_delta: None,
            }],
        )
        .await
        .unwrap();
    let mut config = MatchConfig::pve(57, 1);
    config.ecology = Some(ecology.clone());
    let host = server
        .state
        .attach_world_host(MatchSpec {
            custom: false,
            match_id,
            world_id,
            league: League::Human,
            ruleset_hash: hash,
            config: config.clone(),
            seats: vec![SeatSpec {
                faction: 0,
                principal_id: owner.principal_id,
                kind: PrincipalKindCode::Human,
            }],
        })
        .await
        .unwrap();
    let mut socket = StreamClient::new(
        rynvek_network::websocket::connect(&format!(
            "{}/api/v1/worlds/{world_id}/stream",
            base.replacen("http://", "ws://", 1)
        ))
        .await
        .unwrap(),
        owner.principal_id,
    );
    socket.hello(owner.token_bytes(), world_id, 0).await;
    socket.recv_on(WireChannelId::Control, |_| {}).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        host.wait_for(|s| s.state == MatchState::Running),
    )
    .await
    .unwrap();
    let mut original = restored(&server, world_id).await;
    assert_eq!(original.config, config);
    // 真实地图出生区为贫营养基岩，驻守并不等于进食；以受控战略命令走向实际营养区。
    let start = original.units.values().next().unwrap().tile();
    let mut food = Vec::new();
    for y in 3..original.map.height - 3 {
        for x in 3..original.map.width - 3 {
            let nourished = (-3..=3).all(|dy| {
                (-3..=3).all(|dx| {
                    original
                        .fields
                        .get(rynvek_rts::FieldId::Nutrient, i64::from(x) + dx, i64::from(y) + dy)
                        > 0
                })
            });
            if nourished && original.nav.passable(i64::from(x), i64::from(y)) {
                let dx = i64::from(x) - i64::from(start.0);
                let dy = i64::from(y) - i64::from(start.1);
                food.push((dx * dx + dy * dy, x, y));
            }
        }
    }
    food.sort();
    let (_, x, y) = food[0];
    let command = rynvek_rts::commands::RtsCommand::IssueOrder {
        units: rynvek_rts::commands::UnitSelector::Archetype(None),
        order: rynvek_rts::OrderKind::Move,
        target: rynvek_rts::Vec2::from_int(x as i32, y as i32),
    };
    let lease = socket
        .command(0, json!({"type":"acquire_lease","seq":1,"level":"Sovereign"}))
        .await;
    let token = lease["lease"]["fencing_token"].as_u64().unwrap();
    let mut first = original.candidate_tick().unwrap();
    first.command(rynvek_rts::FactionId(0), &command).unwrap();
    first.finish_tick().unwrap();
    let (ack, advanced) = tokio::join!(
        socket.command(
            0,
            json!({"type":"command","seq":2,"fencing_token":token,"command":command})
        ),
        async {
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                host.wait_for(|s| s.pending_inputs == 1),
            )
            .await
            .unwrap();
            host.advance(1).await
        }
    );
    assert_eq!(ack["type"], "ack", "{ack}");
    advanced.unwrap();
    original = first;
    assert_eq!(original.state_root(), host.status().state_root);
    for _ in 0..9 {
        for _ in 0..20 {
            original.step().unwrap();
        }
        host.advance(20).await.unwrap();
        let replayed = restored(&server, world_id).await;
        assert_eq!(replayed.state_root(), original.state_root());
        assert_eq!(replayed.state_root(), host.status().state_root);
        assert_eq!(replayed.ecology, original.ecology);
    }
    let eco = original.ecology.as_ref().unwrap();
    if expect_death {
        assert!(
            eco.remains.values().any(|b| b.account.received_q > 0),
            "真实Actor必须产生已进食后的代谢死亡，不能只保存参数"
        );
        for (id, body) in eco.remains.iter().filter(|(_, b)| b.account.received_q > 0) {
            assert!(!original.units.contains_key(id));
            assert_eq!(body.account.dissipated_q, body.account.received_q);
            assert_eq!(body.account.stored_q, 0);
            assert_eq!(body.account.returned_q, 0);
            assert!(body.account.is_conserved());
        }
    } else {
        assert!(eco.bodies.values().any(|b| b.account.received_q > 0));
    }
    assert!(eco.bodies.values().all(|b| b.account.is_conserved()));
    let viewer = register(&base, "energy-viewer", "human", 102).await;
    let public = client::get(&url, Some(&viewer.token)).await.unwrap();
    assert_eq!(public.status, 200);
    assert!(
        public.json().unwrap()["worlds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["world_id"] == world_id.to_string()),
        "必须检查真实可见的世界元数据，不能用不存在接口的404代替隐私验证"
    );
    assert!(
        !String::from_utf8_lossy(&public.body).contains("stored_q"),
        "公开世界元数据不可带私有个体能量"
    );
    assert_eq!(
        server
            .state
            .store()
            .get_rating(owner.principal_id, League::Human)
            .await
            .unwrap()
            .matches_played,
        0
    );
    if !expect_death {
        // 同一真实认证Actor继续建槽、生产、付款出生和WAL恢复；不直接插入单位或身体账户。
        let origin = (0..original.map.height)
            .flat_map(|y| (0..original.map.width).map(move |x| (x, y)))
            .find(|&p| {
                original
                    .validate_placement(rynvek_rts::FactionId(0), rynvek_rts::BuildingKind::SpawningVat, p)
                    .is_ok()
            })
            .unwrap();
        let mut vat = None;
        for phase in 0..2 {
            let command = if phase == 0 {
                rynvek_rts::commands::RtsCommand::PlaceBuilding {
                    kind: rynvek_rts::BuildingKind::SpawningVat,
                    origin,
                }
            } else {
                rynvek_rts::commands::RtsCommand::QueueUnit {
                    vat: vat.unwrap(),
                    unit: rynvek_rts::UnitKind::Forager,
                    genome_slug: "seed.forager.v1".into(),
                    count: 1,
                }
            };
            let old_ids: Vec<_> = original.units.keys().copied().collect();
            let mut next = original.candidate_tick().unwrap();
            let expected = next.apply_command(rynvek_rts::FactionId(0), &command).unwrap();
            if let rynvek_rts::commands::CommandAck::BuildingPlaced { id } = expected {
                vat = Some(id);
            }
            next.finish_tick().unwrap();
            let (ack, advanced) = tokio::join!(
                socket.command(
                    original.tick,
                    json!({"type":"command","seq":3+phase,"fencing_token":token,"command":command})
                ),
                async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        host.wait_for(|s| s.pending_inputs == 1),
                    )
                    .await
                    .unwrap();
                    host.advance(1).await
                }
            );
            assert_eq!(ack["type"], "ack", "{ack}");
            advanced.unwrap();
            original = next;
            let ticks = if phase == 0 {
                rynvek_rts::buildings::building_spec(rynvek_rts::BuildingKind::SpawningVat).build_ticks
            } else {
                rynvek_rts::units::unit_spec(rynvek_rts::UnitKind::Forager).hatch_ticks - 1
            };
            for n in (0..ticks).step_by(20).map(|i| (ticks - i).min(20)) {
                for _ in 0..n {
                    original.step().unwrap();
                }
                host.advance(n).await.unwrap();
                let replayed = restored(&server, world_id).await;
                assert_eq!(replayed.state_root(), original.state_root());
                assert_eq!(replayed.ecology, original.ecology);
                assert_eq!(host.status().state_root, original.state_root());
            }
            if phase == 1 {
                let born = *original
                    .units
                    .keys()
                    .find(|id| !old_ids.contains(id) && !original.ecology.as_ref().unwrap().births.contains_key(id))
                    .expect("真实Actor孵化必须出生，不能拿自然子代冒充生产完成");
                let a = &original.ecology.as_ref().unwrap().bodies[&born].account;
                assert!(
                    a.received_q >= u64::from(params.birth_energy_milli) * 4000,
                    "真实Actor不能在空身体池上冒充已付款出生"
                );
                assert!(a.is_conserved());
                assert_eq!(a.unfunded_q, 0);
                assert_eq!(original.units[&born].order.kind, rynvek_rts::OrderKind::HoldPosition);
            }
        }
    }
    if !expect_death {
        let eco = original.ecology.as_ref().unwrap();
        assert!(
            !eco.births.is_empty(),
            "真实认证Actor须由进食亲本自然出生，不能只保存配置或跑孤立模拟"
        );
        assert!(eco.bodies.values().all(|b| b.account.is_conserved()));
        let replayed = restored(&server, world_id).await;
        assert_eq!(
            replayed.ecology, original.ecology,
            "亲本转出/子代收入/谱系必须随真实WAL恢复"
        );
        assert_eq!(replayed.state_root(), host.status().state_root);
        let observation = serde_json::to_string(&original.observe(rynvek_rts::FactionId(0)).unwrap()).unwrap();
        for field in ["transferred_q", "parent_genome", "child_genome", "allocated_q"] {
            assert!(!observation.contains(field), "观察输出不得泄露私有谱系/能量");
        }
    }
    server.shutdown().await;
}

#[test]
fn automatic_first_feeding_survives_serialized_tick_wal() {
    use rynvek_rts::{ArenaMap, ArenaWorld, TerrainTileKind, UnitKind};
    use rynvek_server::transactions::TickRecord;
    let mut cfg = MatchConfig::pve(57, 1);
    cfg.ecology = Some(EcologyContract::single(
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
    let mut world = ArenaWorld::with_map(
        cfg,
        ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::OpenSubstrate; 96 * 96]),
    );
    let id = *world.units.iter().find(|(_, u)| u.kind == UnitKind::Forager).unwrap().0;
    let u = world.units.get_mut(&id).unwrap();
    let mut g = u.genome.clone();
    g.model = rynvek_rts::genome::seed_genome_spec(UnitKind::Striker).model;
    g.w_in.fill(256);
    let n = g.n_hid as usize;
    let harvest = rynvek_rts::genome::action::HARVEST;
    g.w_out[harvest * n..(harvest + 1) * n].fill(512);
    u.brain = rynvek_rts::brain::UnitBrain::from_genome(&g).unwrap();
    u.genome = g;
    let before = world.units[&id].brain.state_root();
    let mut original = ArenaRuntime::new([76; 16], world);
    let cp = original.checkpoint().unwrap();
    let mut replay = ArenaRuntime::restore([76; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
    for step in 0..13 {
        let base_revision = original.revision();
        let base_root = original.state_root();
        let target_tick = original.tick + 1;
        let mut next = original.candidate_tick().unwrap();
        next.finish_tick().unwrap();
        let record = TickRecord {
            format: "rynvek-tick/2".into(),
            base_revision,
            base_root,
            target_tick,
            tick: next.tick,
            revision: next.revision(),
            state_root: next.state_root(),
            inputs: vec![],
        };
        let bytes = serde_json::to_vec(&record).unwrap();
        let saved: TickRecord = serde_json::from_slice(&bytes).unwrap();
        replay = saved.replay(&replay).unwrap();
        assert_eq!(replay.state_root(), next.state_root());
        if step == 0 {
            assert_ne!(next.units[&id].brain.state_root(), before);
            assert!(next.ecology.as_ref().unwrap().bodies[&id].account.dissipated_q > 4000);
        }
        original = next;
    }
    let a = &original.ecology.as_ref().unwrap().bodies[&id].account;
    assert!(a.received_q > 0);
    assert!(a.dissipated_q > 0);
    assert_eq!(a.unfunded_q, 0);
    assert!(a.is_conserved());
    assert_ne!(original.units[&id].brain.state_root(), before);
}

#[tokio::test]
async fn survival_contract_cannot_sneak_into_legacy_or_invalid_worlds() {
    let server = start_server(false).await;
    let base = server.base_url();
    let owner = register(&base, "survival-validation", "human", 105).await;
    let url = format!("{base}/api/v1/worlds");
    let params = json!({"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}});
    for ecology in [serde_json::Value::Null, json!("energy_exchange_v1")] {
        let res = client::send_json(
            "POST",
            &url,
            Some(&owner.token),
            &json!({"title":"拒绝隐式升级","template":"arena_pve","ecology":ecology,"forager_survival":params}),
        )
        .await
        .unwrap();
        assert_eq!(res.status, 400);
    }
    for bad in [
        json!({"roles":{"unit.forager":{"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}),
        json!({"roles":{"unit.forager":{"basal_cost_micro_per_tick":0,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}),
        json!({"roles":{"unit.forager":{"basal_cost_micro_per_tick":u32::MAX,"reserve_ticks":u32::MAX,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}),
        json!({"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}),
        json!({"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":0,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}),
    ] {
        let res = client::send_json(
            "POST",
            &url,
            Some(&owner.token),
            &json!({"title":"拒绝非法代谢参数","template":"arena_pve","ecology":bad}),
        )
        .await
        .unwrap();
        assert_eq!(res.status, 400);
    }
    server.shutdown().await;
}
