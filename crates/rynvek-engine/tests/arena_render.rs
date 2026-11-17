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
