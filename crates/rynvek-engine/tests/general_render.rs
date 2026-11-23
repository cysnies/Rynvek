// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 原General投影→原WorldRenderer真实回读；设备可能是llvmpipe，绝不冒称浏览器硬件。
mod common;
use rynvek_engine::{Engine, EngineCommand, EngineConfig};
use rynvek_render::{Camera2D, OffscreenTarget, WorldPalette};
use rynvek_simulation::{
    SimulationEngine,
    physics::{Body, ONE},
};
#[test]
fn remote_general_original_renderer_reads_actual_circle_radius_and_clears_rejected_private_frame() {
    let Some(owner) = common::gpu() else {
        return;
    };
    let authority = SimulationEngine::with_bodies(
        [0x54; 16],
        &[Body {
            position_q32: [64 * ONE, 128 * ONE],
            radius_q32: ONE,
            velocity_q32: [0; 2],
            acceleration_q32: [0; 2],
            mass_q32: ONE,
            fixed: true,
        }],
    )
    .unwrap();
    let frame = rynvek_engine::general::GeneralRenderFrame::capture(&authority);
    let mut engine = Engine::new(EngineConfig {
        world_id: authority.world_id,
        general_remote: true,
        viewport: [256, 256],
        ..Default::default()
    });
    engine.attach_gpu(owner).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    engine.enable_rendering(format).unwrap();
    engine.camera = Camera2D::new([64., 128.], 16., [256, 256]);
    engine
        .handle_command(
            EngineCommand::from_json(&serde_json::json!({"type":"GENERAL_FRAME","frame":frame}).to_string()).unwrap(),
        )
        .unwrap();
    let target = OffscreenTarget::new(engine.gpu_owner().unwrap().device(), [256, 256], format);
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let rgba = target.read_color_blocking(owner.device(), owner.queue());
    let at = |x: usize, y: usize| &rgba[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
    assert_eq!(at(128, 128), WorldPalette::faction(0).0);
    assert_eq!(at(136, 128), WorldPalette::faction(0).0);
    assert_ne!(at(152, 128), WorldPalette::faction(0).0); // 圆外保留原Renderer坐标网格，不把它误认成身体。
    assert!(owner.take_uncaptured_errors().is_empty());
    let mut bad = frame;
    bad.world_id = "ff".repeat(16);
    assert!(
        engine
            .handle_command(EngineCommand::GeneralFrame { frame: Box::new(bad) })
            .is_err()
    );
    assert_eq!(engine.frame_scene().instance_count(), 0);
    assert!(engine.render_frame(&target.color_view));
    let owner = engine.gpu_owner().unwrap();
    let cleared = target.read_color_blocking(owner.device(), owner.queue());
    assert_eq!(
        &cleared[(128 * 256 + 128) * 4..(128 * 256 + 128) * 4 + 4],
        WorldPalette::NEUTRAL_DARK.background.0
    );
    assert_eq!(engine.simulation().clock.tick, 0);
}
