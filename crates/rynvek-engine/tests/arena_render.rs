// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 同一裁剪观察穿过 Worker JSON 协议与 WorldRenderer 的可运行回归；不冒充浏览器验收。
mod common;
use rynvek_engine::{Engine, EngineCommand, EngineConfig, EngineEvent, GpuMode};
use rynvek_render::{Camera2D, OffscreenTarget, RenderLayer, RenderLayerMask, WorldPalette};
use rynvek_rts::{ArenaWorld, MatchConfig, ids::FactionId, observation::FactionObservation};

fn engine() -> Engine {
    Engine::new(EngineConfig {
        arena_faction: Some(0),
        gpu_mode: GpuMode::Disabled,
        viewport: [256, 256],
        ..Default::default()
    })
}
fn world() -> ArenaWorld {
    ArenaWorld::new(MatchConfig::duel(42))
}
fn frame(obs: FactionObservation) -> EngineCommand {
    // 与服务器/TS 同样的线上字段，经过真实 serde 而非旁路 setter。
    EngineCommand::from_json(
        &serde_json::json!({"type":"ARENA_OBSERVATION",
        "worldId":"00000000000000000000000000000000", "observation":obs})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn observation_to_scene_uses_real_entities_not_demo_and_never_ticks() {
    let mut engine = engine();
    assert_eq!(engine.frame_scene().instance_count(), 0);
    let mut world = world();
    let obs = world.observe(FactionId(0)).unwrap();
    engine.handle_command(frame(obs.clone())).unwrap();
    let before = engine.frame_scene();
    assert_eq!(before.layer_instances(RenderLayer::Organisms).len(), obs.units.len());
    assert_eq!(before.tick, world.tick);
    let first = &obs.units[0];
    assert_eq!(
        before.layer_instances(RenderLayer::Organisms)[0].position,
        [
            first.position.x.0 as f64 / 4294967296.0,
            first.position.y.0 as f64 / 4294967296.0
        ]
        .map(|v| v as f32)
    );
    let command = rynvek_rts::commands::RtsCommand::IssueOrder {
        units: rynvek_rts::commands::UnitSelector::Ids(vec![first.id]),
        order: rynvek_rts::ids::OrderKind::Move,
        target: rynvek_rts::math::Vec2::new(
            rynvek_core::fixed::Q32_32(first.position.x.0 + (5_i64 << 32)),
            first.position.y,
        ),
    };
    world.apply_command(FactionId(0), &command).unwrap();
    for _ in 0..30 {
        world.step();
    }
    let moved = world.observe(FactionId(0)).unwrap();
    assert_ne!(moved.units[0].position, first.position);
    engine.handle_command(frame(moved)).unwrap();
    assert_eq!(engine.frame_scene().tick, world.tick);
    assert_ne!(
        engine.frame_scene().layer_instances(RenderLayer::Organisms)[0].position,
        before.layer_instances(RenderLayer::Organisms)[0].position
    );
    assert!(engine.handle_command(EngineCommand::Start { tick_rate: 30 }).is_err());
    assert!(engine.step().is_err());
    assert_eq!(engine.simulation().clock.tick, 0);
    assert!(
        !engine
            .drain_events()
            .iter()
            .any(|e| matches!(e, EngineEvent::TickCommitted { .. }))
    );
}

#[test]
fn hidden_entities_never_enter_gpu_instances_even_if_fog_layer_disabled() {
    let world = world();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let enemy = world.observe(FactionId(1)).unwrap();
    let count = obs.units.len();
    obs.units.extend(enemy.units);
    obs.buildings.extend(enemy.buildings);
    obs.visibility.fill(0);
    let mut engine = engine();
    engine.handle_command(frame(obs)).unwrap();
    engine
        .handle_command(EngineCommand::SetLayerMask { mask: !(1 << 6) })
        .unwrap();
    let scene = engine.frame_scene();
    assert_eq!(scene.layer_instances(RenderLayer::Organisms).len(), count);
    assert_eq!(scene.layer_instances(RenderLayer::Structures).len(), 1);
    assert!(scene.ordered_instances().iter().all(|i| i.pick_id == 0));
    assert!(!scene.layers.contains(RenderLayer::FieldHeatmap));
    assert!(!scene.layers.contains(RenderLayer::Terrain));
}

#[test]
fn wrong_binding_stale_and_malformed_frames_roll_back_atomically() {
    let mut engine = engine();
    let mut obs = world().observe(FactionId(0)).unwrap();
    obs.tick = 10;
    engine.handle_command(frame(obs.clone())).unwrap();
    let before = engine.frame_scene();
    for case in 0..4 {
        let mut bad = obs.clone();
        match case {
            0 => bad.tick = 9,
            1 => bad.faction = FactionId(1),
            2 => bad.visibility_width = 0,
            _ => bad.visibility[0] = 3,
        }
        assert!(engine.handle_command(frame(bad)).is_err());
        assert_eq!(engine.frame_scene(), before);
        assert_eq!(engine.last_tick(), 10);
    }
    let mut wrong_world = frame(obs.clone());
    if let EngineCommand::ArenaObservation { world_id, .. } = &mut wrong_world {
        *world_id = "f".repeat(32);
    }
    assert!(engine.handle_command(wrong_world).is_err());
    assert_eq!(engine.frame_scene(), before);
    // 终局可以与上个 Tick 相同，不把服务器最后一次观察当作过期包。
    obs.outcome = Some(rynvek_rts::world::MatchOutcome::Draw { tick: 10 });
    engine.handle_command(frame(obs)).unwrap();
    engine.handle_command(EngineCommand::Shutdown).unwrap();
    assert!(
        engine
            .handle_command(frame(world().observe(FactionId(0)).unwrap()))
            .is_err()
    );
}

#[test]
fn enemy_disappears_from_next_frame_and_simulation_mode_rejects_observations() {
    let world = world();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let mut enemy = world.observe(FactionId(1)).unwrap().units.remove(0);
    enemy.position = obs.units[0].position;
    obs.units.push(enemy);
    let count = obs.units.len();
    let mut engine = engine();
    engine.handle_command(frame(obs.clone())).unwrap();
    assert_eq!(
        engine.frame_scene().layer_instances(RenderLayer::Organisms).len(),
        count
    );
    obs.tick += 1;
    obs.visibility.fill(2);
    engine.handle_command(frame(obs.clone())).unwrap();
    assert_eq!(
        engine.frame_scene().layer_instances(RenderLayer::Organisms).len(),
        count - 1
    );
    assert!(Engine::new(EngineConfig::default()).handle_command(frame(obs)).is_err());
}

#[test]
fn real_wgpu_renders_observed_unit_and_never_uploads_hidden_enemy() {
    let Some(owner) = common::gpu() else {
        return;
    };
    let mut engine = engine();
    let world = world();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let p = obs.units[0].position;
    let center = [p.x.0 as f64 / 4294967296.0, p.y.0 as f64 / 4294967296.0].map(|v| v as f32);
    let mut enemy = world.observe(FactionId(1)).unwrap().units.remove(0);
    enemy.position = rynvek_rts::math::Vec2::new(rynvek_core::fixed::Q32_32(p.x.0 + (8_i64 << 32)), p.y);
    obs.units.push(enemy);
    obs.visibility.fill(0);
    engine.attach_gpu(owner).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    engine.enable_rendering(format).unwrap();
    engine.camera = Camera2D::new(center, 8.0, [256, 256]);
    engine.handle_command(frame(obs)).unwrap();
    let target = OffscreenTarget::new(engine.gpu_owner().unwrap().device(), [256, 256], format);
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let pixels = target.read_color_blocking(owner.device(), owner.queue());
    let at = |x: usize, y: usize| &pixels[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
    assert_eq!(at(128, 128), WorldPalette::faction(0).0);
    assert_eq!(at(192, 128), WorldPalette::NEUTRAL_DARK.background.0);
    assert!(owner.take_uncaptured_errors().is_empty());
    assert_eq!(engine.simulation().clock.tick, 0);
    assert_eq!(engine.frame_scene().layers.0 & RenderLayerMask::ALL.0, 12);
}

#[test]
fn fog_index_never_rounds_a_hidden_unit_into_the_adjacent_visible_cell() {
    let world = world();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let count = obs.units.len();
    let mut enemy = world.observe(FactionId(1)).unwrap().units.remove(0);
    enemy.position = rynvek_rts::math::Vec2::new(
        rynvek_core::fixed::Q32_32((4_i64 << 32) - 1),
        rynvek_core::fixed::Q32_32::from_int(2),
    );
    obs.units.push(enemy);
    obs.visibility.fill(0);
    obs.visibility[1] = 1;
    let mut engine = engine();
    engine.handle_command(frame(obs)).unwrap();
    assert_eq!(
        engine.frame_scene().layer_instances(RenderLayer::Organisms).len(),
        count
    );
}

fn map_command(world: &ArenaWorld) -> serde_json::Value {
    serde_json::json!({"type":"ARENA_MAP", "worldId":"0".repeat(32), "map": {
        "width":world.map.width, "height":world.map.height,
        "tiles":world.map.tiles.iter().map(|t| *t as u8).collect::<Vec<_>>(),
        "tilesHash":world.map.tiles_hash().to_hex()
    }})
}
fn command(value: serde_json::Value) -> EngineCommand {
    EngineCommand::from_json(&value.to_string()).unwrap()
}
#[test]
fn verified_server_map_and_own_selection_reach_existing_frame_scene() {
    let world = world();
    let mut engine = engine();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let own = obs.units[0].id.0;
    let mut enemy = world.observe(FactionId(1)).unwrap().units.remove(0);
    enemy.position = obs.units[0].position;
    let enemy_id = enemy.id.0;
    obs.units.push(enemy);
    engine.handle_command(command(map_command(&world))).unwrap();
    engine.handle_command(frame(obs.clone())).unwrap();
    engine.handle_command(command(serde_json::json!({"type":"ARENA_SELECTION", "worldId":"0".repeat(32), "units":[own, enemy_id], "building":obs.buildings[0].id.0}))).unwrap();
    let scene = engine.frame_scene();
    assert!(!scene.layer_instances(RenderLayer::Terrain).is_empty());
    assert_eq!(scene.layer_instances(RenderLayer::Overlay).len(), 2);
    assert!(scene.ordered_instances().iter().all(|i| i.pick_id == 0));
    assert_eq!(engine.simulation().clock.tick, 0);
    // 恶意保留已经不在观察中的选择，也不得残留高亮。
    obs.units.clear();
    obs.buildings.clear();
    obs.tick += 1;
    engine.handle_command(frame(obs)).unwrap();
    assert_eq!(engine.frame_scene().layer_instances(RenderLayer::Overlay).len(), 0);
}
#[test]
fn map_hash_dimensions_world_and_immutability_are_checked_atomically() {
    let world = world();
    let mut engine = engine();
    engine
        .handle_command(frame(world.observe(FactionId(0)).unwrap()))
        .unwrap();
    let before = engine.frame_scene();
    for case in 0..5 {
        let mut bad = map_command(&world);
        match case {
            0 => bad["map"]["tilesHash"] = serde_json::json!("f".repeat(64)),
            1 => bad["map"]["tiles"][0] = serde_json::json!(5),
            2 => bad["map"]["width"] = serde_json::json!(0),
            3 => bad["map"]["height"] = serde_json::json!(world.map.height - 4),
            _ => bad["worldId"] = serde_json::json!("f".repeat(32)),
        }
        assert!(engine.handle_command(command(bad)).is_err());
        assert_eq!(engine.frame_scene(), before);
    }
    engine.handle_command(command(map_command(&world))).unwrap();
    let valid = engine.frame_scene();
    let mut changed = world.map.clone();
    changed.tiles[0] = rynvek_rts::map_gen::TerrainTileKind::NutrientMarsh;
    let mut replacement = map_command(&world);
    replacement["map"]["tiles"] = serde_json::json!(changed.tiles.iter().map(|t| *t as u8).collect::<Vec<_>>());
    replacement["map"]["tilesHash"] = serde_json::json!(changed.tiles_hash().to_hex());
    assert!(engine.handle_command(command(replacement)).is_err());
    assert_eq!(engine.frame_scene(), valid);
    engine.handle_command(EngineCommand::Shutdown).unwrap();
    assert!(engine.handle_command(command(map_command(&world))).is_err());
}

#[test]
fn software_gpu_pixels_cover_terrain_fog_selection_and_y_down_mapping() {
    let Some(owner) = common::gpu() else {
        return;
    };
    let world = world();
    let mut engine = engine();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let mut own = obs.units.remove(0);
    own.position = rynvek_rts::math::Vec2::new(
        rynvek_core::fixed::Q32_32(3_i64 << 31),
        rynvek_core::fixed::Q32_32(3_i64 << 31),
    );
    let mut enemy = world.observe(FactionId(1)).unwrap().units.remove(0);
    enemy.position = rynvek_rts::math::Vec2::new(rynvek_core::fixed::Q32_32(11_i64 << 31), own.position.y);
    let ids = vec![own.id.0, enemy.id.0];
    obs.units = vec![own, enemy];
    obs.buildings.clear();
    obs.visibility_width = 2;
    obs.visibility = vec![1, 2, 0, 1];
    // 非对称、已知色值的地图夹具检查真实管线，不用 CPU 位图替代 GPU 输出。
    let tiles: Vec<u8> = (0..64).map(|i| (i % 8 % 5) as u8).collect();
    engine
        .handle_command(command(
            serde_json::json!({"type":"ARENA_MAP", "worldId":"0".repeat(32), "map": {
                "width":8,"height":8,"tilesHash":rynvek_core::hash::Blake3Hash::of(&tiles).to_hex(),"tiles":tiles
            }}),
        ))
        .unwrap();
    engine.handle_command(frame(obs)).unwrap();
    engine
        .handle_command(command(
            serde_json::json!({"type":"ARENA_SELECTION", "worldId":"0".repeat(32), "units":ids,"building":null}),
        ))
        .unwrap();
    engine
        .handle_command(EngineCommand::SetLayerMask {
            mask: RenderLayerMask::ALL.without(RenderLayer::FogOfWar).0,
        })
        .unwrap();
    assert_eq!(engine.frame_scene().layer_instances(RenderLayer::Overlay).len(), 1);
    engine.attach_gpu(owner).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    engine.enable_rendering(format).unwrap();
    engine.camera = Camera2D::new([4.0, 4.0], 32.0, [256, 256]);
    let target = OffscreenTarget::new(engine.gpu_owner().unwrap().device(), [256, 256], format);
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let pixels = target.read_color_blocking(owner.device(), owner.queue());
    let at = |x: usize, y: usize| &pixels[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
    for (x, color) in [
        (16, [39, 54, 43, 255]),
        (48, [59, 69, 71, 255]),
        (80, [83, 108, 50, 255]),
        (112, [110, 59, 40, 255]),
        (144, [7, 9, 8, 255]),
        (176, [12, 17, 13, 255]),
    ] {
        assert_eq!(at(x, 16), color);
    }
    assert_eq!(at(16, 144), WorldPalette::NEUTRAL_DARK.background.0);
    assert_eq!(at(48, 48), WorldPalette::faction(0).0); // 与浏览器 yDown=true 相同，非竖直镜像。
    assert_eq!(at(66, 48), [228, 255, 158, 255]);
    eprintln!(
        "[原生软件GL像素，非浏览器] 地形={:?}，暗格={:?}，未知格={:?}，己方={:?}，选择环={:?}，隐藏敌人落点={:?}",
        at(16, 16),
        at(176, 16),
        at(16, 144),
        at(48, 48),
        at(66, 48),
        at(176, 48)
    );
    assert_eq!(at(176, 48), [12, 17, 13, 255]); // 已探索格不能显示/高亮敌人。
    engine
        .handle_command(EngineCommand::SetLayerMask {
            mask: RenderLayerMask::ALL.without(RenderLayer::Overlay).0,
        })
        .unwrap();
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let plain = target.read_color_blocking(owner.device(), owner.queue());
    assert_eq!(&plain[(48 * 256 + 66) * 4..(48 * 256 + 66) * 4 + 4], [83, 108, 50, 255]);
    assert!(owner.take_uncaptured_errors().is_empty());
    assert_eq!(engine.simulation().clock.tick, 0);
}

#[test]
fn overlay_is_user_intent_not_server_placement_prediction_and_rejects_bad_binding() {
    let mut engine = engine();
    let world = world();
    engine
        .handle_command(frame(world.observe(FactionId(0)).unwrap()))
        .unwrap();
    let value = serde_json::json!({"type":"ARENA_OVERLAY", "worldId":"0".repeat(32), "drag":[[7.0,6.0],[2.0,3.0]], "placement":{"kind":"bld.signal_beacon","origin":[4,4]}});
    engine.handle_command(command(value.clone())).unwrap();
    let scene = engine.frame_scene();
    assert_eq!(scene.layer_instances(RenderLayer::Overlay).len(), 2);
    assert!(
        scene
            .layer_instances(RenderLayer::Overlay)
            .iter()
            .all(|i| i.pick_id == 0)
    );
    for bad in [
        serde_json::json!({"worldId":"f".repeat(32)}),
        serde_json::json!({"drag":[[1e10,0.0],[1.0,1.0]]}),
    ] {
        let mut invalid = value.clone();
        for (key, val) in bad.as_object().unwrap() {
            invalid[key] = val.clone();
        }
        assert!(engine.handle_command(command(invalid)).is_err());
        assert_eq!(engine.frame_scene(), scene);
    }
    engine
        .handle_command(command(
            serde_json::json!({"type":"ARENA_OVERLAY", "worldId":"0".repeat(32), "drag":null,"placement":null}),
        ))
        .unwrap();
    assert!(engine.frame_scene().layer_instances(RenderLayer::Overlay).is_empty());
    assert_eq!(engine.simulation().clock.tick, 0);
}

#[test]
fn real_gpu_draws_hollow_drag_and_neutral_placement_without_hidden_occupancy_signal() {
    let Some(owner) = common::gpu() else {
        return;
    };
    let world = world();
    let mut engine = engine();
    let mut obs = world.observe(FactionId(0)).unwrap();
    let mut enemy = world.observe(FactionId(1)).unwrap().units.remove(0);
    enemy.position = rynvek_rts::math::Vec2::new(
        rynvek_core::fixed::Q32_32::from_int(5),
        rynvek_core::fixed::Q32_32::from_int(5),
    );
    obs.units = vec![enemy];
    obs.buildings.clear();
    obs.visibility_width = 2;
    obs.visibility = vec![0; 4];
    engine.handle_command(frame(obs.clone())).unwrap();
    engine.handle_command(command(serde_json::json!({"type":"ARENA_OVERLAY","worldId":"0".repeat(32),"drag":[[4.0,7.0],[2.0,4.0]],"placement":{"kind":"bld.signal_beacon","origin":[4,4]}}))).unwrap();
    engine.attach_gpu(owner).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    engine.enable_rendering(format).unwrap();
    engine.camera = Camera2D::new([4.0, 4.0], 32.0, [256, 256]);
    let target = OffscreenTarget::new(engine.gpu_owner().unwrap().device(), [256, 256], format);
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let pixels = target.read_color_blocking(owner.device(), owner.queue());
    let at = |x: usize, y: usize| &pixels[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
    assert_eq!(at(64, 176), [200, 239, 160, 255]);
    assert_eq!(at(96, 223), [200, 239, 160, 255]);
    assert_eq!(at(160, 128), [234, 214, 134, 255]);
    assert_eq!(at(96, 176), WorldPalette::NEUTRAL_DARK.background.0);
    assert_eq!(at(160, 160), WorldPalette::NEUTRAL_DARK.background.0);
    eprintln!(
        "[原生软件GL，非浏览器] 反向非正方拖框={:?}，落点轮廓={:?}，隐藏占地内部={:?}",
        at(64, 176),
        at(160, 128),
        at(160, 160)
    );
    // 隐藏占地有无敌人，预览全部像素必须相同，不能形成合法性/占用侧信道。
    obs.units.clear();
    obs.tick += 1;
    engine.handle_command(frame(obs)).unwrap();
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    assert_eq!(pixels, target.read_color_blocking(owner.device(), owner.queue()));
    engine
        .handle_command(command(
            serde_json::json!({"type":"ARENA_OVERLAY","worldId":"0".repeat(32),"drag":null,"placement":null}),
        ))
        .unwrap();
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let cleared = target.read_color_blocking(owner.device(), owner.queue());
    assert_eq!(
        &cleared[(176 * 256 + 64) * 4..(176 * 256 + 64) * 4 + 4],
        WorldPalette::NEUTRAL_DARK.background.0
    );
    assert!(owner.take_uncaptured_errors().is_empty());
    assert_eq!(engine.simulation().clock.tick, 0);
}

#[test]
fn applied_theme_reaches_frame_scene_and_does_not_reveal_hidden_units() {
    let arena = world();
    let mut render = engine();
    let mut obs = arena.observe(FactionId(0)).unwrap();
    let count = obs.units.len();
    let enemy = arena.observe(FactionId(1)).unwrap().units[0].clone();
    obs.units.push(enemy);
    obs.visibility.fill(0);
    render.handle_command(command(map_command(&arena))).unwrap();
    render.handle_command(frame(obs)).unwrap();
    let before = render.frame_scene();
    assert_eq!(before.layer_instances(RenderLayer::Organisms).len(), count);
    let background = 0xFF40_2010u32;
    let factions = [0xFF80_10E0, 0xFF20_C080, 0xFFFF_7F00, 0xFF00_FF7F, 0xFF7F_00FF, 0xFF00_FFFF, 0xFF01_0203];
    assert!(render
        .handle_command(command(serde_json::json!({
            "type":"ARENA_THEME","worldId":"f".repeat(32),"requestId":1,"background":background,"factions":factions
        })))
        .is_err());
    assert_eq!(render.frame_scene(), before);
    render
        .handle_command(command(serde_json::json!({
            "type":"ARENA_THEME","worldId":"0".repeat(32),"requestId":9,"background":background,"factions":factions
        })))
        .unwrap();
    let events = render.drain_events();
    assert!(events.iter().any(|e| matches!(e, EngineEvent::ArenaThemeApplied { request_id: 9, background: 0xFF40_2010, .. })));
    let scene = render.frame_scene();
    assert_eq!(scene.layer_instances(RenderLayer::Organisms).len(), count);
    assert!(scene.layer_instances(RenderLayer::Organisms).iter().all(|i| i.color == factions[0]));
    assert!(scene.layer_instances(RenderLayer::Structures).iter().any(|i| i.color == factions[0]));
    assert!(scene.layer_instances(RenderLayer::Terrain).iter().any(|i| i.color == background));
    if let Some(owner) = common::gpu() {
        let mut gpu_engine = engine();
        let mut obs = world().observe(FactionId(0)).unwrap();
        obs.buildings.clear();
        obs.units.clear();
        obs.visibility_width = 2;
        obs.visibility = vec![0, 0, 0, 0];
        let tiles: Vec<u8> = (0..64).map(|i| (i % 8 % 5) as u8).collect();
        gpu_engine
            .handle_command(command(serde_json::json!({"type":"ARENA_MAP","worldId":"0".repeat(32),"map":{
                "width":8,"height":8,"tilesHash":rynvek_core::hash::Blake3Hash::of(&tiles).to_hex(),"tiles":tiles
            }})))
            .unwrap();
        gpu_engine.handle_command(frame(obs)).unwrap();
        gpu_engine
            .handle_command(command(serde_json::json!({
                "type":"ARENA_THEME","worldId":"0".repeat(32),"requestId":3,"background":background,"factions":factions
            })))
            .unwrap();
        gpu_engine.attach_gpu(owner).unwrap();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        gpu_engine.enable_rendering(format).unwrap();
        gpu_engine.camera = Camera2D::new([4.0, 4.0], 32.0, [256, 256]);
        let target = OffscreenTarget::new(gpu_engine.gpu_owner().unwrap().device(), [256, 256], format);
        assert!(gpu_engine.render_frame(&target.color_view));
        let owner = gpu_engine.gpu_owner().unwrap();
        let pixels = target.read_color_blocking(owner.device(), owner.queue());
        let at = |x: usize, y: usize| &pixels[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
        assert_eq!(at(16, 144), [0x10, 0x20, 0x40, 0xFF], "未知格须用主题背景，不泄漏地形");
        assert!(owner.take_uncaptured_errors().is_empty());
        assert_eq!(gpu_engine.simulation().clock.tick, 0);
    }
}
