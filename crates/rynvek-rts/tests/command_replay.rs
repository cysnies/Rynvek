// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 命令录制与确定性重放：`.rreplay`（M7）的基础——同一配置 + 同一命令日志 ⇒ 逐 Tick 相同的状态根。

use rynvek_rts::commands::{RtsCommand, UnitSelector};
use rynvek_rts::ids::{BuildingKind, FactionId, OrderKind, UnitKind};
use rynvek_rts::math::Vec2;
use rynvek_rts::world::{ArenaWorld, MatchConfig};

const P0: FactionId = FactionId(0);
const P1: FactionId = FactionId(1);

fn spot(w: &ArenaWorld, f: FactionId, kind: BuildingKind) -> (u32, u32) {
    let (cx, cy) = w.buildings[&w.factions[f.0 as usize].nexus.unwrap()].center_tile();
    (4..20i64)
        .flat_map(|r| (-r..=r).flat_map(move |dy| (-r..=r).map(move |dx| (cx as i64 + dx, cy as i64 + dy))))
        .filter(|&(x, y)| x >= 0 && y >= 0)
        .map(|(x, y)| (x as u32, y as u32))
        .find(|&o| w.validate_placement(f, kind, o).is_ok())
        .expect("落点")
}

/// 双方各建孵化槽、排产、下达移动 / 进攻；返回录制了命令的世界与每 Tick 状态根
fn scripted(ticks: u64) -> (ArenaWorld, Vec<[u8; 32]>) {
    let mut w = ArenaWorld::new(MatchConfig::duel(7));
    w.enable_command_log();
    let mut roots = Vec::new();
    while w.tick < ticks {
        roots.push(*w.state_hash().as_bytes());
        match w.tick {
            5 => {
                for f in [P0, P1] {
                    let origin = spot(&w, f, BuildingKind::SpawningVat);
                    w.apply_command(
                        f,
                        &RtsCommand::PlaceBuilding {
                            kind: BuildingKind::SpawningVat,
                            origin,
                        },
                    )
                    .unwrap();
                }
                // 失败的命令不得进入日志
                assert!(
                    w.apply_command(
                        P0,
                        &RtsCommand::PlaceBuilding {
                            kind: BuildingKind::NexusCore,
                            origin: (1, 1)
                        }
                    )
                    .is_err()
                );
            }
            40 => {
                let target = Vec2::from_int(128, 128);
                w.apply_command(
                    P0,
                    &RtsCommand::IssueOrder {
                        units: UnitSelector::Archetype(Some(UnitKind::Striker)),
                        order: OrderKind::AttackMove,
                        target,
                    },
                )
                .unwrap();
                w.apply_command(
                    P1,
                    &RtsCommand::IssueOrder {
                        units: UnitSelector::Archetype(None),
                        order: OrderKind::Move,
                        target,
                    },
                )
                .unwrap();
            }
            _ => {}
        }
        w.step();
    }
    roots.push(*w.state_hash().as_bytes());
    (w, roots)
}

#[test]
fn only_accepted_commands_are_recorded_with_their_tick() {
    let (w, _) = scripted(60);
    let log = w.command_log();
    assert_eq!(log.len(), 4);
    assert_eq!(
        log.iter().map(|r| (r.tick, r.faction)).collect::<Vec<_>>(),
        [(5, P0), (5, P1), (40, P0), (40, P1)]
    );
    let json = serde_json::to_string(log).unwrap();
    let back: Vec<rynvek_rts::RecordedCommand> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, log);
}

#[test]
fn replay_reproduces_every_tick_state_root() {
    let (w, roots) = scripted(600);
    let mut replayed_roots = Vec::new();
    let replayed = ArenaWorld::replay(w.config.clone(), w.command_log(), 600, |rw| {
        replayed_roots.push(*rw.state_hash().as_bytes())
    })
    .unwrap();
    assert_eq!(replayed_roots, roots);
    assert_eq!(replayed.state_hash(), w.state_hash());
    assert_eq!(replayed.command_log(), w.command_log());
}

#[test]
fn tampered_log_diverges_or_is_rejected() {
    let (w, _) = scripted(300);
    let mut log = w.command_log().to_vec();
    log[2].tick += 1;
    let replayed = ArenaWorld::replay(w.config.clone(), &log, 300, |_| {}).unwrap();
    assert_ne!(
        replayed.state_hash(),
        w.state_hash(),
        "命令错位一个 Tick 必然改变状态根"
    );

    let mut log = w.command_log().to_vec();
    log.swap(0, 1);
    log[0].faction = P0; // 让 P0 在已占用的落点再建一次
    assert!(ArenaWorld::replay(w.config.clone(), &log, 300, |_| {}).is_err());
}
