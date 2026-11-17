// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 引擎端到端：GPU 镜像异步核对、真实 DeviceLost 5 步恢复、降级至 CPU Reference、渲染与拾取。
//! 所有场景都与纯 CPU 引擎逐 Tick 比对状态根——GPU 永不影响权威结果。

mod common;

use std::time::Duration;

use rynvek_engine::{ComputeBackendKind, Engine, EngineConfig, EngineEvent, GpuMode};
use rynvek_field::channels::FieldChannelId;
use rynvek_render::OffscreenTarget;
use rynvek_wgpu::RecoveryPhase;

fn config(gpu_mode: GpuMode) -> EngineConfig {
    EngineConfig {
        gpu_mode,
        viewport: [256, 256],
        ..Default::default()
    }
}

/// 有 GPU 的引擎；无适配器时返回 `None`（跳过）
fn gpu_engine() -> Option<Engine> {
    let owner = common::gpu()?;
    let mut e = Engine::new(config(GpuMode::Required));
    e.attach_gpu(owner).unwrap();
    e.announce_ready();
    seed_world(&mut e);
    Some(e)
}

fn cpu_engine() -> Engine {
    let mut e = Engine::new(config(GpuMode::Disabled));
    seed_world(&mut e);
    e
}

fn seed_world(e: &mut Engine) {
    e.set_field_value(FieldChannelId::Nutrient, 100, 100, 20_000);
    e.set_field_value(FieldChannelId::SignalA, 30, 200, -9_000);
    e.set_field_source(FieldChannelId::Energy, 128, 128, 300);
    e.set_field_source(FieldChannelId::Hazard, 5, 250, 77);
}

/// 测试专用：轮询直到设备回调（含延迟送达的 lost 回调）、恢复流程与回读全部完成（主循环本身从不等待）
fn settle(e: &mut Engine) {
    for _ in 0..5000 {
        e.pump();
        if let Some(o) = e.gpu_owner() {
            let _ = o.wait_idle();
        }
        e.pump();
        let terminal = matches!(e.recovery_phase(), RecoveryPhase::Healthy | RecoveryPhase::Degraded);
        let lost = e.gpu_owner().is_some_and(|o| o.is_lost());
        if terminal && !lost && !e.readbacks_in_flight() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("恢复未在有限时间内完成：{:?}", e.recovery_phase());
}

fn assert_mirror_matches_cpu(e: &Engine) {
    for id in FieldChannelId::ALL {
        let gpu = e.gpu_field_interior_blocking(id).expect("GPU 镜像存在");
        assert!(
            gpu == e.cpu_field_interior(id),
            "通道 {} 的 GPU 镜像与 CPU 权威不一致",
            id.as_str()
        );
    }
}

#[test]
fn gpu_mirror_verifies_every_readback_and_never_changes_state_roots() {
    let Some(mut gpu) = gpu_engine() else { return };
    let mut cpu = cpu_engine();
    assert_eq!(gpu.backend(), ComputeBackendKind::Webgpu);
    for _ in 0..40 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    }
    settle(&mut gpu);
    let s = gpu.stats();
    eprintln!("[engine] {s:?}");
    assert_eq!(s.divergences, 0);
    assert!(s.verified_ticks > 0);
    // seed_world 标脏 → 首个 Tick 以重同步代替 GPU 推进；随后只在场更新 Tick 回读，
    // 要么因忙槽跳过 / 被覆盖 / 历史过期而未核对（从不等待），但绝不允许出现分歧
    assert_eq!(s.resyncs, 1);
    assert_eq!(s.gpu_ticks, 13);
    assert_eq!(
        s.verified_ticks + s.skipped_readbacks + s.dropped_readbacks + s.stale_readbacks,
        s.gpu_ticks
    );
    assert_mirror_matches_cpu(&gpu);
    assert_eq!(gpu.simulation().diagnostics_nonzero_count, 0, "INV-01");
}

#[test]
fn real_device_loss_recovers_through_five_steps_and_resumes() {
    let Some(mut gpu) = gpu_engine() else { return };
    let mut cpu = cpu_engine();
    for _ in 0..5 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    }
    gpu.drain_events();
    gpu.simulate_device_lost();
    // 丢失期间仿真照常推进（CPU 权威）
    for _ in 0..3 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    }
    settle(&mut gpu);
    assert_eq!(gpu.recovery_phase(), RecoveryPhase::Healthy);
    assert_eq!(gpu.gpu_owner().unwrap().generation(), 1);
    let events = gpu.drain_events();
    let lost = events
        .iter()
        .position(|e| matches!(e, EngineEvent::DeviceLost { .. }))
        .expect("DEVICE_LOST");
    let steps: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::RecoveryProgress { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    assert_eq!(steps, [1, 2, 3, 4, 5]);
    let recovered = events
        .iter()
        .find_map(|e| match e {
            EngineEvent::Recovered {
                generation,
                resume_tick,
            } => Some((*generation, *resume_tick)),
            _ => None,
        })
        .expect("RECOVERED");
    // 恢复点 = 侦测到丢失时 GPU 尚未完成的首个 Tick；lost 回调的送达时机因后端而异（GL 立即、Vulkan 可能延后）
    let EngineEvent::DeviceLost { tick: lost_tick, .. } = events[lost] else {
        unreachable!()
    };
    assert_eq!(recovered, (1, lost_tick), "从第一个未在 GPU 上完成的 Tick 恢复");
    assert!((6..=9).contains(&lost_tick), "{lost_tick}");
    for _ in 0..10 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    }
    settle(&mut gpu);
    assert_eq!(gpu.backend(), ComputeBackendKind::Webgpu);
    assert_eq!(gpu.stats().divergences, 0);
    assert_mirror_matches_cpu(&gpu);
}

#[test]
fn exhausted_recovery_degrades_to_cpu_reference_without_affecting_results() {
    let Some(mut gpu) = gpu_engine() else { return };
    let mut cpu = cpu_engine();
    gpu.faults.fail_reacquire = 3; // = 默认最大尝试次数
    for _ in 0..3 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    }
    gpu.simulate_device_lost();
    assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    settle(&mut gpu);
    assert_eq!(gpu.recovery_phase(), RecoveryPhase::Degraded);
    assert_eq!(gpu.backend(), ComputeBackendKind::CpuReference);
    assert!(!gpu.has_gpu_mirror());
    assert!(
        gpu.drain_events()
            .iter()
            .any(|e| matches!(e, EngineEvent::BackendDegraded { .. }))
    );
    for _ in 0..10 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    }
}

#[test]
fn failed_verification_retries_from_reacquire_then_recovers() {
    let Some(mut gpu) = gpu_engine() else { return };
    gpu.faults.fail_verification = 1;
    gpu.run_ticks(2).unwrap();
    gpu.drain_events();
    gpu.simulate_device_lost();
    gpu.step().unwrap();
    settle(&mut gpu);
    assert_eq!(gpu.recovery_phase(), RecoveryPhase::Healthy);
    // 首次校验失败 → 重新获取设备（第二代），再次校验通过
    assert_eq!(gpu.gpu_owner().unwrap().generation(), 2);
    let steps: Vec<u8> = gpu
        .drain_events()
        .iter()
        .filter_map(|e| match e {
            EngineEvent::RecoveryProgress { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    assert_eq!(steps, [1, 2, 3, 4, 2, 3, 4, 5]);
    gpu.run_ticks(3).unwrap();
    settle(&mut gpu);
    assert_mirror_matches_cpu(&gpu);
}

#[test]
fn engine_renders_world_and_resolves_gpu_pick() {
    let Some(mut e) = gpu_engine() else { return };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    e.enable_rendering(format).unwrap();
    // 世界 [0, 16)² 映射到 256×256：实体 0 位于格 (2, 2) → 像素 (32, 32)
    e.handle_command(rynvek_engine::EngineCommand::SetCamera {
        center_x: 8.0,
        center_y: 8.0,
        px_per_cell: 16.0,
    })
    .unwrap();
    e.run_ticks(2).unwrap();
    let target = OffscreenTarget::new(e.gpu_owner().unwrap().device(), [256, 256], format);
    e.request_pick(77, 32, 32);
    assert!(e.render_frame(&target.color_view));
    let mut pick = None;
    for _ in 0..2000 {
        e.pump();
        if let Some(ev) = e.drain_events().into_iter().find_map(|ev| match ev {
            EngineEvent::PickResult {
                request_id,
                dense_index,
            } => Some((request_id, dense_index)),
            _ => None,
        }) {
            pick = Some(ev);
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(pick, Some((77, Some(0))));
    let owner = e.gpu_owner().unwrap();
    let pixels = target.read_color_blocking(owner.device(), owner.queue());
    let at = |x: usize, y: usize| &pixels[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
    assert_eq!(at(32, 32), rynvek_render::WorldPalette::faction(0).0);
    assert!(owner.take_uncaptured_errors().is_empty());
}

#[test]
fn field_mirror_matches_cpu_at_every_multirate_boundary() {
    let Some(mut gpu) = gpu_engine() else { return };
    let mut cpu = cpu_engine();
    for tick in 1..=12 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
        settle(&mut gpu);
        assert_mirror_matches_cpu(&gpu);
        assert_eq!(gpu.stats().divergences, 0, "第 {tick} Tick 不应重复推进 GPU 场");
    }
}

#[test]
fn non_field_tick_uploads_edits_without_advancing_the_field() {
    let Some(mut gpu) = gpu_engine() else { return };
    let mut cpu = cpu_engine();
    assert_eq!(gpu.step().unwrap(), cpu.step().unwrap());
    gpu.set_field_value(FieldChannelId::Nutrient, 7, 9, 12345);
    cpu.set_field_value(FieldChannelId::Nutrient, 7, 9, 12345);
    for tick in 2..=7 {
        assert_eq!(gpu.step().unwrap(), cpu.step().unwrap(), "Tick {tick}");
        settle(&mut gpu);
        assert_mirror_matches_cpu(&gpu);
    }
    assert_eq!(gpu.stats().resyncs, 2);
    assert_eq!(gpu.stats().gpu_ticks, 2);
    assert_eq!(gpu.stats().divergences, 0);
}
