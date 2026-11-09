// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 180° 中心对称地图生成器的自动验收：对称性、出生安全区、确定性、连通性与跨语言 Golden 哈希。
//!
//! Golden 夹具 `tests/golden_vectors/arena_maps_v1.json` 同时被 `@rynvek/arena` 的 TypeScript 端口校验；
//! 生成器改动时以 `RYNVEK_UPDATE_GOLDEN=1 cargo test -p rynvek-rts --test map_generation` 重写夹具。

use std::path::PathBuf;

use rynvek_rts::{ArenaMap, ArenaMapSize, TerrainTileKind, generate_symmetric_arena_map};
use serde_json::{Value, json};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden_vectors/arena_maps_v1.json")
}

/// Golden 用例：(宽, 高, 种子)
const GOLDEN_CASES: &[(u32, u32, u64)] = &[
    (256, 256, 0),
    (256, 256, 1),
    (256, 256, 42),
    (256, 256, 0xDEAD_BEEF),
    (256, 256, u64::MAX),
    (128, 128, 7),
    (512, 512, 2026),
];

fn golden_entry(w: u32, h: u32, seed: u64) -> Value {
    let m = ArenaMap::generate_custom(w, h, seed);
    let probes: Vec<Value> = [(0u32, 0u32), (32, 32), (w / 2, h / 2 - 1), (w - 1, h - 1), (17, 99), (w - 5, 3)]
        .iter()
        .map(|&(x, y)| json!([x, y, m.tile(x, y) as u8]))
        .collect();
    json!({
        "width": w,
        "height": h,
        // u64 种子以十进制字符串存放，避免 JavaScript number 精度损失
        "seed": seed.to_string(),
        "blake3": m.tiles_hash().to_hex(),
        "histogram": m.terrain_histogram(),
        "probes": probes,
    })
}

#[test]
fn golden_map_hashes_are_stable() {
    let actual = json!({
        "schema": "rynvek.golden.arena-maps.v1",
        "generator": "rynvek_rts::generate_symmetric_arena_map",
        "cases": GOLDEN_CASES.iter().map(|&(w, h, s)| golden_entry(w, h, s)).collect::<Vec<_>>(),
    });
    let path = golden_path();
    if std::env::var_os("RYNVEK_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("缺少 Golden 夹具")).unwrap();
    assert_eq!(actual, expected, "地图生成器输出与 Golden 夹具不一致");
}

#[test]
fn all_standard_sizes_are_point_symmetric() {
    for size in ArenaMapSize::ALL {
        let m = ArenaMap::generate(size, 0x5EED_0000 + size.side_tiles() as u64);
        assert_eq!(m.width, size.side_tiles());
        assert!(m.is_point_symmetric(), "{} 不对称", size.as_str());
        // 出生锚点互为中心反演
        let [a, b] = m.spawn_anchors();
        assert_eq!(m.mirror_point(a), b);
        let [ta, tb] = m.spawn_tiles();
        assert_eq!(m.mirror_tile(ta.0, ta.1), tb);
    }
}

#[test]
fn spawn_safe_zone_is_conductive_basalt() {
    let m = ArenaMap::generate(ArenaMapSize::Duel1v1, 99);
    for y in 0..m.height {
        for x in 0..m.width {
            let (dx, dy) = (x as i64 - 32, y as i64 - 32);
            if dx * dx + dy * dy <= 400 {
                assert_eq!(m.tile(x, y), TerrainTileKind::ConductiveBasalt, "({x}, {y})");
                let (mx, my) = m.mirror_tile(x, y);
                assert_eq!(m.tile(mx, my), TerrainTileKind::ConductiveBasalt);
            }
        }
    }
}

#[test]
fn generator_is_deterministic_and_seed_sensitive() {
    let a = generate_symmetric_arena_map(256, 256, 12345);
    let b = generate_symmetric_arena_map(256, 256, 12345);
    let c = generate_symmetric_arena_map(256, 256, 12346);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
#[should_panic(expected = "Arena dimensions must be even")]
fn odd_dimensions_are_rejected() {
    let _ = generate_symmetric_arena_map(255, 256, 0);
}

#[test]
fn terrain_mix_follows_frozen_thresholds() {
    // 远离出生区的 Tile 按 r % 1000 的阈值分布：墙 8.5%、沼泽 7.5%、裂隙 1.5%、玄武岩 12.5%、基质 70%
    let m = ArenaMap::generate(ArenaMapSize::Skirmish2v2, 31);
    let mut counts = [0u64; 5];
    let mut total = 0u64;
    for y in 0..m.height {
        for x in 0..m.width {
            let near_a = (x as i64 - 32).pow(2) + (y as i64 - 32).pow(2) <= 900;
            let (mx, my) = m.mirror_tile(x, y);
            let near_b = (mx as i64 - 32).pow(2) + (my as i64 - 32).pow(2) <= 900;
            if !near_a && !near_b {
                counts[m.tile(x, y) as usize] += 1;
                total += 1;
            }
        }
    }
    let expected_permille = [700u64, 125, 75, 15, 85];
    for (i, &e) in expected_permille.iter().enumerate() {
        let got = counts[i] * 1000 / total;
        assert!(got.abs_diff(e) <= 5, "地形 {i}: {got}‰ vs {e}‰");
    }
}

#[test]
fn spawns_are_connected_by_two_six_wide_corridors() {
    for seed in [0u64, 1, 2, 3, 42, 777, 0xDEAD_BEEF] {
        let m = ArenaMap::generate(ArenaMapSize::Duel1v1, seed);
        let report = m.validate_connectivity();
        assert!(report.ok, "种子 {seed}：{report:?}");
        assert_eq!(report.required, 12);
    }
}

#[test]
fn edge_rift_anchors_are_open_ground() {
    let m = ArenaMap::generate(ArenaMapSize::Duel1v1, 5);
    for (x, y) in m.edge_rift_anchors() {
        let t = m.tile(x, y);
        assert!(t != TerrainTileKind::ImperviousWall && t != TerrainTileKind::AcidicFissure);
        assert!(x <= 2 || y <= 2 || x >= m.width - 3 || y >= m.height - 3, "锚点应贴近地图外围：({x}, {y})");
    }
}
