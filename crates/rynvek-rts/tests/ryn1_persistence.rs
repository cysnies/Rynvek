// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_core::Blake3Hash;
use rynvek_rts::{ArenaWorld, MatchConfig, TerrainTileKind};

#[test]
fn commit_and_snapshot_bind_the_same_canonical_world() {
    let world = ArenaWorld::new(MatchConfig::duel(42));
    let parent = [7; 32];
    let expected = world.canonical_state_with_parent(Blake3Hash(parent)).state_root().0;
    let commit = world.to_commit_manifest([1; 16], 3, parent);
    assert_eq!(commit.state_root_hash, expected, "WAL 必须绑定完整规范世界状态");
    let (_, snapshot) = world.encode_snapshot([1; 16], 3, parent).unwrap();
    assert_eq!(snapshot.state_root, expected, "快照与 WAL 必须采用相同根定义");
}

#[test]
fn snapshot_restores_edited_terrain_and_continues_identically() {
    let mut world = ArenaWorld::new(MatchConfig::duel(42));
    let index = world.map.index(20, 20);
    world.map.tiles[index] = if world.map.tiles[index] == TerrainTileKind::ImperviousWall {
        TerrainTileKind::NutrientMarsh
    } else {
        TerrainTileKind::ImperviousWall
    };
    // 模拟地形补丁的真实导航更新；保留原世界，不用两份恢复副本互相证明。
    world.nav = rynvek_rts::nav::NavGrid::from_terrain(world.map.width, world.map.height, &world.map.tiles);
    for building in world.buildings.values() {
        let rect = building.rect();
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                world.nav.set_occupied(x, y, true);
            }
        }
    }
    world.hpa = rynvek_rts::nav::HpaGraph::build(&world.nav);
    let (bytes, _) = world.encode_snapshot([1; 16], 1, [0; 32]).unwrap();
    let mut restored = ArenaWorld::decode_snapshot(&bytes).expect("修改后的地形必须能够恢复");
    assert_eq!(restored.map.tiles, world.map.tiles);
    assert_eq!(restored.state_hash(), world.state_hash());
    for _ in 0..12 {
        world.step();
        restored.step();
        assert_eq!(restored.state_hash(), world.state_hash());
    }
}

#[test]
fn snapshot_rejects_a_tampered_envelope_root() {
    let world = ArenaWorld::new(MatchConfig::duel(42));
    let (mut bytes, _) = world.encode_snapshot([1; 16], 1, [0; 32]).unwrap();
    bytes[64] ^= 1;
    assert!(
        ArenaWorld::decode_snapshot(&bytes).is_err(),
        "ROOT 分块必须与文件头状态根一致"
    );
}

#[test]
fn snapshot_binds_header_identity_tick_revision_and_version() {
    let world = ArenaWorld::new(MatchConfig::duel(42));
    let (bytes, _) = world.encode_snapshot([2; 16], 7, [3; 32]).unwrap();
    // 版本、世界身份、时钟与修订都必须和 CMIT 一致。
    for offset in [4, 16, 32, 40] {
        let mut tampered = bytes.clone();
        tampered[offset] ^= 1;
        assert!(ArenaWorld::decode_snapshot(&tampered).is_err(), "文件头偏移 {offset}");
    }
}
