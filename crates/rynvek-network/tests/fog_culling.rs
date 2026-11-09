// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端物理迷雾裁剪（GAP-12）：在一局脚本对局里逐 Tick 检查
//!
//! 1. 客户端副本恰好等于服务端为该阵营计算的 "可知集合"；
//! 2. 副本里每个敌方实体都能被权威迷雾独立证明为可见；
//! 3. 不可见敌方状态的任意变化不改变下发字节（非干扰性：零字节泄露）；
//! 4. 离开视野只下发一次 `EntityVisibilityLost` 静态幽灵；关键帧按间隔出现。

use std::collections::{BTreeMap, BTreeSet};

use rynvek_network::replication::{ReplicaState, Replicator, StatePacket, visible_records};
use rynvek_network::visibility::VisibilityGrid;
use rynvek_rts::commands::{RtsCommand, UnitSelector};
use rynvek_rts::ids::{FactionId, OrderKind};
use rynvek_rts::math::Vec2;
use rynvek_rts::world::{ArenaWorld, MatchConfig};

const P0: FactionId = FactionId(0);
const P1: FactionId = FactionId(1);
const KEYFRAME_INTERVAL: u64 = 300;

fn order(w: &mut ArenaWorld, f: FactionId, order: OrderKind, target: Vec2) {
    w.apply_command(
        f,
        &RtsCommand::IssueOrder {
            units: UnitSelector::Archetype(None),
            order,
            target,
        },
    )
    .unwrap();
}

fn home(w: &ArenaWorld, f: FactionId) -> Vec2 {
    let (x, y) = w.buildings[&w.factions[f.0 as usize].nexus.unwrap()].center_tile();
    Vec2::from_int(x as i32, y as i32)
}

/// 脚本：双方开局全军压向中心（约 Tick 800 相遇交战），Tick 1000 双方撤回老家（制造 "离开视野"）
fn script(w: &mut ArenaWorld) {
    let centre = Vec2::from_int(w.map.width as i32 / 2, w.map.height as i32 / 2);
    match w.tick {
        10 => {
            order(w, P0, OrderKind::Move, centre);
            order(w, P1, OrderKind::Move, centre);
        }
        1000 => {
            let back = home(w, P1);
            order(w, P1, OrderKind::Move, back);
            let back = home(w, P0);
            order(w, P0, OrderKind::Move, back);
        }
        _ => {}
    }
}

fn grid_of(w: &ArenaWorld, f: FactionId) -> VisibilityGrid {
    VisibilityGrid::from_fog(&w.factions[f.0 as usize].fog)
}

/// 把所有对 `viewer` 不可见的敌方单位 / 建筑 / 私有账本随意篡改
fn perturb_hidden_enemy_state(w: &mut ArenaWorld, viewer: FactionId) -> usize {
    let fog = w.factions[viewer.0 as usize].fog.clone();
    let mut touched = 0;
    for u in w.units.values_mut() {
        if u.faction == viewer {
            continue;
        }
        let (x, y) = u.tile();
        if fog.is_visible(x, y) {
            continue;
        }
        u.defense.hp_milli -= 1;
        u.defense.shield_milli += 7;
        u.weapon_cooldown ^= 3;
        // 挪到同样不可见的相邻 Tile（若有）
        for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i64 + dx as i64, y as i64 + dy as i64);
            if nx >= 0
                && ny >= 0
                && nx < fog.width as i64
                && ny < fog.height as i64
                && !fog.is_visible(nx as u32, ny as u32)
            {
                u.position = Vec2::from_int(nx as i32, ny as i32);
                break;
            }
        }
        touched += 1;
    }
    for b in w.buildings.values_mut() {
        let f = b.footprint();
        if b.faction != viewer && !fog.any_visible(b.origin.0, b.origin.1, f, f) {
            b.defense.hp_milli -= 11;
            b.build_progress = b.build_progress.wrapping_add(5);
            touched += 1;
        }
    }
    for f in w.factions.iter_mut().filter(|f| f.id != viewer) {
        f.ledger.balance.biomass += 123_456;
        f.ledger.biomass_harvested_total += 99;
    }
    touched
}

#[test]
fn replica_matches_visible_set_and_never_leaks_hidden_state() {
    let mut w = ArenaWorld::new(MatchConfig::duel(7));
    let mut rep = Replicator::new(P0, KEYFRAME_INTERVAL);
    let mut replica = ReplicaState::new();
    let mut keyframes = Vec::new();
    let mut enemy_sightings = 0usize;
    let mut ghost_events: BTreeMap<u32, usize> = BTreeMap::new();
    let mut absent_since_lost: BTreeSet<u32> = BTreeSet::new();
    let mut interference_checks = 0usize;
    let mut perturbed_entities = 0usize;

    while w.tick < 1500 {
        script(&mut w);
        w.step();

        // (3) 非干扰性：每 25 Tick 复制世界、篡改不可见敌方状态，比较两份下发字节
        if w.tick % 25 == 0 {
            let mut shadow = w.clone();
            let touched = perturb_hidden_enemy_state(&mut shadow, P0);
            if touched > 0 {
                let mut rep_a = rep.clone();
                let mut rep_b = rep.clone();
                let a = rep_a.next_packet(&w).encode();
                let b = rep_b.next_packet(&shadow).encode();
                assert_eq!(a, b, "Tick {}：不可见敌方状态影响了下发字节", w.tick);
                interference_checks += 1;
                perturbed_entities += touched;
            }
        }

        let packet = rep.next_packet(&w);
        let bytes = packet.encode();
        assert_eq!(bytes.len() % 16, 0, "状态包须 16 字节对齐");
        let decoded = StatePacket::decode(&bytes).unwrap();
        assert_eq!(decoded, packet);
        if decoded.keyframe {
            keyframes.push(decoded.tick);
        }
        for g in &decoded.lost {
            if !decoded.keyframe {
                assert!(absent_since_lost.insert(g.id), "实体 {} 的幽灵重复下发", g.id);
                *ghost_events.entry(g.id).or_default() += 1;
            }
        }
        for r in &decoded.upserts {
            absent_since_lost.remove(&r.id);
        }
        replica.apply(&decoded).unwrap();

        // (1) 副本 == 服务端可知集合
        let grid = grid_of(&w, P0);
        let truth = visible_records(&w, P0, &grid);
        assert_eq!(replica.entities, truth, "Tick {}：副本与可知集合不一致", w.tick);

        // (2) 每个敌方实体都由权威迷雾独立证明可见
        let fog = &w.factions[0].fog;
        for r in replica.entities.values().filter(|r| r.faction != P0.0) {
            enemy_sightings += 1;
            let visible = if r.is_building() {
                let b = w.buildings.values().find(|b| b.id.0 == r.id).unwrap();
                let f = b.footprint();
                fog.any_visible(b.origin.0, b.origin.1, f, f)
            } else {
                let u = w.units.values().find(|u| u.id.0 == r.id).unwrap();
                let (x, y) = u.tile();
                fog.is_visible(x, y)
            };
            assert!(visible, "Tick {}：敌方实体 {} 不可见却被下发", w.tick, r.id);
        }
        // 幽灵与实体不重叠，且幽灵位置当前不可见
        for g in replica.ghosts.values() {
            assert!(!replica.entities.contains_key(&g.id));
            assert!(!fog.is_visible((g.x >> 32) as u32, (g.y >> 32) as u32) || g.kind >= 16);
            assert_ne!(g.faction, P0.0, "己方实体不产生幽灵");
        }
        // 私有状态只属于己方
        let private = replica.private.unwrap();
        assert_eq!(private.balance_milli[0], w.factions[0].ledger.balance.biomass);
    }

    assert!(enemy_sightings > 0, "脚本未让双方相遇，测试无效");
    assert!(!ghost_events.is_empty(), "脚本未产生离开视野事件，测试无效");
    assert!(interference_checks >= 20 && perturbed_entities > 0);
    assert_eq!(
        keyframes,
        [1, 301, 601, 901, 1201],
        "关键帧按 {KEYFRAME_INTERVAL} Tick 间隔"
    );
}

#[test]
fn keyframe_request_resyncs_a_fresh_replica() {
    let mut w = ArenaWorld::new(MatchConfig::duel(3));
    let mut rep = Replicator::new(P1, KEYFRAME_INTERVAL);
    let mut replica = ReplicaState::new();
    for _ in 0..50 {
        script(&mut w);
        w.step();
        replica.apply(&rep.next_packet(&w)).unwrap();
    }
    // 新客户端（断线重连）从 Delta 无法起步
    let mut fresh = ReplicaState::new();
    script(&mut w);
    w.step();
    let delta = rep.next_packet(&w);
    assert!(!delta.keyframe);
    assert!(fresh.apply(&delta).is_err());
    replica.apply(&delta).unwrap();
    rep.request_keyframe();
    w.step();
    let key = rep.next_packet(&w);
    assert!(key.keyframe && key.visibility.is_some());
    fresh.apply(&key).unwrap();
    replica.apply(&key).unwrap();
    assert_eq!(fresh.entities, replica.entities);
    assert_eq!(fresh.entities, visible_records(&w, P1, &grid_of(&w, P1)));
}
