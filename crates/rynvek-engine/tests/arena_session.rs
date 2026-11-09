// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `ArenaSession` 与 `ArenaToolHost`：六个受控工具在真实 `ArenaWorld` 上的语义、错误码与租约接管流程。

use rynvek_agent::{ManualClock, PrincipalKind};
use rynvek_engine::arena::{ARENA_PROTOCOL, ArenaClientMessage, ArenaServerMessage, ArenaSession, Envelope};
use rynvek_rts::commands::RtsCommand;
use rynvek_rts::{BuildingKind, MatchConfig};
use serde_json::{Value, json};

fn hello(s: &mut ArenaSession<ManualClock>, id: &str, kind: PrincipalKind, faction: u32) -> Vec<Envelope> {
    s.handle(
        id,
        ArenaClientMessage::Hello {
            protocol: ARENA_PROTOCOL.into(),
            principal_id: id.into(),
            kind,
            faction,
        },
    )
}

fn lease(s: &mut ArenaSession<ManualClock>, id: &str, level: &str) -> u64 {
    let msg = serde_json::from_value(json!({"type": "acquire_lease", "seq": 1, "level": level})).unwrap();
    match &s.handle(id, msg)[0].message {
        ArenaServerMessage::Lease { lease: Some(l), .. } => l.fencing_token,
        other => panic!("{other:?}"),
    }
}

fn tool(s: &mut ArenaSession<ManualClock>, id: &str, token: u64, name: &str, args: Value) -> Value {
    let out = s.handle(
        id,
        ArenaClientMessage::ToolCall {
            seq: 9,
            fencing_token: token,
            tool: name.into(),
            arguments: args,
        },
    );
    match &out[0].message {
        ArenaServerMessage::ToolResult { response, .. } => serde_json::to_value(response).unwrap(),
        other => panic!("{other:?}"),
    }
}

fn advance(s: &mut ArenaSession<ManualClock>, id: &str, ticks: u32) {
    s.handle(id, ArenaClientMessage::Advance { ticks });
}

fn setup() -> (ArenaSession<ManualClock>, u64) {
    let mut s = ArenaSession::new(MatchConfig::duel(42), ManualClock::default());
    let w = hello(&mut s, "agent:x", PrincipalKind::Agent, 0);
    assert!(matches!(&w[0].message, ArenaServerMessage::Welcome { tools, .. } if tools.len() == 6));
    let t = lease(&mut s, "agent:x", "Produce");
    (s, t)
}

#[test]
fn hello_validates_protocol_identity_and_faction() {
    let mut s = ArenaSession::new(MatchConfig::duel(1), ManualClock::default());
    let bad = s.handle(
        "a",
        ArenaClientMessage::Hello {
            protocol: "rynvek-arena/0".into(),
            principal_id: "a".into(),
            kind: PrincipalKind::Agent,
            faction: 0,
        },
    );
    assert!(matches!(&bad[0].message, ArenaServerMessage::Error { error } if error.code == 0x5001));
    let spoof = hello(&mut s, "a", PrincipalKind::Agent, 0);
    assert!(matches!(spoof[0].message, ArenaServerMessage::Welcome { .. }));
    let other = s.handle(
        "b",
        ArenaClientMessage::Hello {
            protocol: ARENA_PROTOCOL.into(),
            principal_id: "a".into(),
            kind: PrincipalKind::Human,
            faction: 0,
        },
    );
    assert!(matches!(&other[0].message, ArenaServerMessage::Error { error } if error.code == 0x8001));
    assert!(
        matches!(&hello(&mut s, "c", PrincipalKind::Agent, 7)[0].message, ArenaServerMessage::Error { error } if error.code == 0x5001)
    );
    let unseated = s.handle("z", ArenaClientMessage::Advance { ticks: 1 });
    assert!(matches!(&unseated[0].message, ArenaServerMessage::Error { error } if error.code == 0x8001));
}

#[test]
fn six_tools_drive_the_authoritative_world() {
    let (mut s, t) = setup();
    advance(&mut s, "agent:x", 1);
    let r = tool(
        &mut s,
        "agent:x",
        t,
        "query_visible_clusters",
        json!({"include_enemy": true, "region_chunk": [0, 0]}),
    );
    assert_eq!(r["result"]["clusters"].as_array().unwrap().len(), 2);
    assert_eq!(r["result"]["nexus"].as_array().unwrap().len(), 1, "敌方主脑不在视野内");

    advance(&mut s, "agent:x", 30);
    let r = tool(
        &mut s,
        "agent:x",
        t,
        "sample_regional_fields",
        json!({"chunk_x": 0, "chunk_y": 0}),
    );
    assert_eq!(r["result"]["channels"].as_array().unwrap().len(), 5);
    advance(&mut s, "agent:x", 30);
    let r = tool(
        &mut s,
        "agent:x",
        t,
        "sample_regional_fields",
        json!({"chunk_x": 3, "chunk_y": 0}),
    );
    assert_eq!(r["error"]["code"], 0x5001, "Chunk 越界");

    // 建造请求：bld.* 原型 + "x,y" 落点；落点非法给出 0x8006
    advance(&mut s, "agent:x", 30);
    let bad = json!({"vat_entity_id": "250,250", "unit_archetype": "bld.signal_beacon", "genome_slug": "", "count": 1});
    assert_eq!(
        tool(&mut s, "agent:x", t, "queue_vat_production", bad)["error"]["code"],
        0x8006
    );
    advance(&mut s, "agent:x", 30);
    let ok = json!({"vat_entity_id": "36,36", "unit_archetype": "bld.signal_beacon", "genome_slug": "", "count": 1});
    let r = tool(&mut s, "agent:x", t, "queue_vat_production", ok);
    assert_eq!(r["result"]["ack"]["type"], "building_placed", "{r}");
    let beacon = r["result"]["ack"]["id"].as_u64().unwrap();

    // 信标调质：浮点参数量化为 milli 写入权威命令
    advance(&mut s, "agent:x", 30);
    let cfg = json!({"beacon_entity_id": beacon.to_string(), "signal_channel": "field.signal_b", "signal_strength": -12.5, "neuromodulators": [0.5, 0, -0.25, 1]});
    let r = tool(&mut s, "agent:x", t, "configure_signal_beacon", cfg);
    assert_eq!(
        r["result"]["command"],
        json!({"type": "configure_beacon", "beacon": beacon, "channel": "field.signal_b", "strength_milli": -12500, "modulators_milli": [500, 0, -250, 1000]})
    );

    // 战略意图
    advance(&mut s, "agent:x", 30);
    let intent = json!({"unit_archetype": "all", "order_kind": "Move", "target_world_x": 40.0, "target_world_y": 40.0});
    assert_eq!(
        tool(&mut s, "agent:x", t, "issue_strategic_intent", intent)["result"]["ack"]["units"],
        4
    );
    advance(&mut s, "agent:x", 30);
    let off =
        json!({"unit_archetype": "unit.forager", "order_kind": "Move", "target_world_x": -1.0, "target_world_y": 40.0});
    assert_eq!(
        tool(&mut s, "agent:x", t, "issue_strategic_intent", off)["error"]["code"],
        0x5001
    );

    // 实体种类错误 0x2002、坩埚不存在 0x2002
    advance(&mut s, "agent:x", 30);
    let wrong = json!({"vat_entity_id": beacon.to_string(), "unit_archetype": "unit.forager", "genome_slug": "seed.forager.v1", "count": 1});
    assert_eq!(
        tool(&mut s, "agent:x", t, "queue_vat_production", wrong)["error"]["code"],
        0x2002
    );
    advance(&mut s, "agent:x", 30);
    let crucible = json!({"crucible_entity_id": "1", "action": "research_tech", "tech_or_archetype_id": "tech.hebbian_plasticity"});
    assert_eq!(
        tool(&mut s, "agent:x", t, "trigger_crucible_breeding", crucible)["error"]["code"],
        0x2002
    );
}

#[test]
fn commands_require_lease_level_and_count_toward_apm() {
    let mut s = ArenaSession::new(MatchConfig::duel(42), ManualClock::default());
    hello(&mut s, "human:h", PrincipalKind::Human, 0);
    let surrender = |token| ArenaClientMessage::Command {
        seq: 3,
        fencing_token: token,
        command: RtsCommand::Surrender,
    };
    let out = s.handle("human:h", surrender(0));
    assert!(matches!(&out[0].message, ArenaServerMessage::Ack { error: Some(e), .. } if e.code == 0x8001));
    let t = lease(&mut s, "human:h", "Produce");
    let out = s.handle("human:h", surrender(t));
    assert!(matches!(&out[0].message, ArenaServerMessage::Ack { error: Some(e), .. } if e.code == 0x5006));
    let place = ArenaClientMessage::Command {
        seq: 4,
        fencing_token: t,
        command: RtsCommand::PlaceBuilding {
            kind: BuildingKind::SignalBeacon,
            origin: (36, 36),
        },
    };
    assert!(matches!(
        &s.handle("human:h", place)[0].message,
        ArenaServerMessage::Ack { ack: Some(_), .. }
    ));
    assert_eq!(s.broker().leases().current(0).unwrap().holder.id, "human:h");
}

#[test]
fn human_takeover_and_release_restore_the_agent() {
    let (mut s, agent_token) = setup();
    hello(&mut s, "human:h", PrincipalKind::Human, 0);
    let msg = serde_json::from_value(json!({"type": "acquire_lease", "seq": 2, "level": "Sovereign"})).unwrap();
    let out = s.handle("human:h", msg);
    assert_eq!(out.len(), 2);
    assert!(
        matches!(&out[1], Envelope { to, message: ArenaServerMessage::LeaseRevoked { by, .. } } if to == "agent:x" && by == "human:h")
    );
    let human_token = match &out[0].message {
        ArenaServerMessage::Lease { lease: Some(l), .. } => l.fencing_token,
        other => panic!("{other:?}"),
    };
    let r = tool(
        &mut s,
        "agent:x",
        agent_token,
        "query_visible_clusters",
        json!({"include_enemy": false}),
    );
    assert_eq!(r["error"]["code"], 0x8001);
    // Agent 不能抢回
    let msg = serde_json::from_value(json!({"type": "acquire_lease", "seq": 3, "level": "Produce"})).unwrap();
    assert!(
        matches!(&s.handle("agent:x", msg)[0].message, ArenaServerMessage::Lease { error: Some(e), .. } if e.code == 0x8001)
    );
    // 只有持有者能交还
    let out = s.handle(
        "agent:x",
        ArenaClientMessage::ReleaseLease {
            seq: 4,
            fencing_token: human_token,
        },
    );
    assert!(matches!(&out[0].message, ArenaServerMessage::Lease { error: Some(e), .. } if e.code == 0x8001));
    let out = s.handle(
        "human:h",
        ArenaClientMessage::ReleaseLease {
            seq: 5,
            fencing_token: human_token,
        },
    );
    assert_eq!(out.len(), 2);
    let restored = match &out[1] {
        Envelope {
            to,
            message:
                ArenaServerMessage::Lease {
                    seq: None,
                    lease: Some(l),
                    ..
                },
        } if to == "agent:x" => l.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(format!("{:?}", restored.level), "Produce");
    advance(&mut s, "agent:x", 30);
    let r = tool(
        &mut s,
        "agent:x",
        restored.fencing_token,
        "query_visible_clusters",
        json!({"include_enemy": false}),
    );
    assert!(r.get("error").is_none(), "{r}");
}

#[test]
fn observations_are_fog_filtered_per_seat_and_events_do_not_leak() {
    let mut s = ArenaSession::new(MatchConfig::duel(42), ManualClock::default());
    hello(&mut s, "p0", PrincipalKind::Agent, 0);
    hello(&mut s, "p1", PrincipalKind::Agent, 1);
    let out = s.handle("p0", ArenaClientMessage::Advance { ticks: 1 });
    assert_eq!(out.len(), 2);
    for e in &out {
        let ArenaServerMessage::Observation { observation, events } = &e.message else {
            panic!()
        };
        let mine = if e.to == "p0" { 0 } else { 1 };
        assert_eq!(observation.faction.0 as u32, mine);
        assert!(observation.units.iter().all(|u| u.faction.0 as u32 == mine));
        assert_eq!(events.len(), 5, "只收到本阵营 Starter Kit 的 5 条事件");
    }
    let too_many = s.handle("p0", ArenaClientMessage::Advance { ticks: 901 });
    assert!(matches!(&too_many[0].message, ArenaServerMessage::Error { error } if error.code == 0x5001));
}
