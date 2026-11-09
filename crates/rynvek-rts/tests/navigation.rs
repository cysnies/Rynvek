// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三级导航在真实生成地图上的确定性与质量断言：
//! - HPA* 路径代价 ≤ 1.25 × 全图精确 A* 最优；
//! - 两次独立构建的抽象图、路径与流场逐位一致；
//! - 沿流场下降必达目标。

use rynvek_rts::arena_map::{ArenaMap, ArenaMapSize};
use rynvek_rts::nav::flow::UNREACHED;
use rynvek_rts::nav::{FlowField, HpaGraph, NavGrid, astar};

/// 简单 64 位 LCG（测试用确定性取样）
fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

fn random_passable(grid: &NavGrid, rng: &mut u64) -> (u32, u32) {
    loop {
        let x = (lcg(rng) % grid.width as u64) as u32;
        let y = (lcg(rng) % grid.height as u64) as u32;
        if grid.passable(x as i64, y as i64) {
            return (x, y);
        }
    }
}

#[test]
fn hpa_is_within_25_percent_of_optimal_on_generated_maps() {
    for seed in [0u64, 42, 2026] {
        let map = ArenaMap::generate(ArenaMapSize::Duel1v1, seed);
        let grid = NavGrid::from_terrain(map.width, map.height, &map.tiles);
        let hpa = HpaGraph::build(&grid);
        let mut rng = seed ^ 0x5151;
        let mut checked = 0;
        let mut worst = 0u64;
        for _ in 0..40 {
            let (a, b) = (random_passable(&grid, &mut rng), random_passable(&grid, &mut rng));
            let exact = astar(&grid, a, b, grid.full_rect());
            let approx = hpa.find_path(&grid, a, b);
            match (exact, approx) {
                (Some((opt, _)), Some(p)) => {
                    assert!(p.cost >= opt);
                    assert!(
                        p.cost * 100 <= opt * 125,
                        "seed {seed} {a:?}→{b:?}: {} vs {opt}",
                        p.cost
                    );
                    worst = worst.max(if opt == 0 { 1000 } else { p.cost * 1000 / opt });
                    for w in p.tiles.windows(2) {
                        let (dx, dy) = (w[1].0 as i32 - w[0].0 as i32, w[1].1 as i32 - w[0].1 as i32);
                        assert!(grid.step_cost(w[0].0, w[0].1, dx, dy).is_some(), "路径包含非法步");
                    }
                    checked += 1;
                }
                (None, None) => {}
                (e, h) => panic!("可达性不一致：exact={:?} hpa={:?}", e.map(|x| x.0), h.map(|p| p.cost)),
            }
        }
        assert!(checked >= 30, "seed {seed} 可达样本过少：{checked}");
        assert!(worst <= 1250, "seed {seed} 最差比 {worst}‰");
    }
}

#[test]
fn navigation_is_bitwise_deterministic() {
    let map = ArenaMap::generate(ArenaMapSize::Duel1v1, 7);
    let build = || {
        let grid = NavGrid::from_terrain(map.width, map.height, &map.tiles);
        let hpa = HpaGraph::build(&grid);
        let [s0, s1] = map.spawn_tiles();
        let path = hpa.find_path(&grid, s0, s1).expect("双出生点必须连通");
        let corridor = path.sectors(grid.width);
        let flow = FlowField::solve(&grid, s1, Some(&corridor));
        (hpa, path, flow)
    };
    let (h1, p1, f1) = build();
    let (h2, p2, f2) = build();
    assert_eq!(h1, h2);
    assert_eq!(p1, p2);
    assert_eq!(f1, f2);
}

#[test]
fn descending_the_flow_field_reaches_the_goal() {
    let map = ArenaMap::generate(ArenaMapSize::Duel1v1, 42);
    let grid = NavGrid::from_terrain(map.width, map.height, &map.tiles);
    let [s0, s1] = map.spawn_tiles();
    let flow = FlowField::solve(&grid, s1, None);
    assert_ne!(flow.arrival_at(s0.0, s0.1), UNREACHED);
    // 按「到达时间最小的可走八邻格」贪心下降，步数有界
    let mut cur = s0;
    for _ in 0..4 * map.width {
        if cur == s1 {
            break;
        }
        let mut best = (flow.arrival_at(cur.0, cur.1), cur);
        for (dx, dy) in rynvek_rts::nav::grid::NEIGHBORS_8 {
            if grid.step_cost(cur.0, cur.1, dx, dy).is_none() {
                continue;
            }
            let n = ((cur.0 as i32 + dx) as u32, (cur.1 as i32 + dy) as u32);
            let v = flow.arrival_at(n.0, n.1);
            if v < best.0 {
                best = (v, n);
            }
        }
        assert_ne!(best.1, cur, "流场出现局部极小");
        cur = best.1;
    }
    assert_eq!(cur, s1);
}
