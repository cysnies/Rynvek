// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 真实 TCP、认证、Actor、提交存储及 WAL 恢复；不是独立生态模拟器。
mod common;
use common::*;
use rynvek_protocol::{WireChannelId, WireUuid};
use rynvek_rts::world::ecology::EcologyVersion;
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
async fn explicit_pve_energy_version_is_authoritative_private_and_replayable() {
    let server = start_server(false).await;
    let base = server.base_url();
    let owner = register(&base, "energy-owner", "human", 101).await;
    let url = format!("{base}/api/v1/worlds");
    let forbidden = client::send_json(
        "POST",
        &url,
        Some(&owner.token),
        &json!({"title":"禁止排位换规则","template":"arena_duel","ecology":"energy_exchange_v1"}),
    )
    .await
    .unwrap();
    assert_eq!(forbidden.status, 400);
    let created = client::send_json("POST", &url,Some(&owner.token), &json!({"title":"真实能量交换","template":"arena_pve","map_seed":57,"visibility":"Public","ecology":"energy_exchange_v1"})).await.unwrap();
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
    config.ecology = Some(EcologyVersion::EnergyExchangeV1);
    let host = server
        .state
        .attach_world_host(MatchSpec {
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
        order: rynvek_rts::OrderKind::Harvest,
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
    assert!(eco.bodies.values().any(|b| b.account.received_q > 0));
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
    server.shutdown().await;
}
