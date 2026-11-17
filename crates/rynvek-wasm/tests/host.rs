// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Worker 宿主协议集成测试（原生）：初始化状态机、命令缓存与回放、固定步长调度、
//! 关闭语义，以及（有适配器时）经宿主驱动的真实 DeviceLost 恢复与 GPU 不改变状态根。

use std::time::Duration;

use rynvek_engine::{ComputeBackendKind, Engine, EngineCommand, EngineConfig, EngineEvent, GpuMode};
use rynvek_wasm::{HostPhase, WorkerHost};

fn init_json(gpu_mode: &str) -> String {
    format!(
        r#"{{"type":"ENGINE_INIT","canvas":{{}},"width":256,"height":256,"devicePixelRatio":1,"gpuMode":"{gpu_mode}"}}"#
    )
}

fn require_gpu() -> bool {
    std::env::var("RYNVEK_REQUIRE_GPU").as_deref() == Ok("1")
}

fn error_codes(events: &[EngineEvent]) -> Vec<u16> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::EngineError { code, .. } => Some(*code),
            _ => None,
        })
        .collect()
}

fn committed(events: &[EngineEvent]) -> Vec<(u64, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::TickCommitted { tick, state_root } => Some((*tick, state_root.clone())),
            _ => None,
        })
        .collect()
}

fn cpu_host() -> WorkerHost {
    let mut h = WorkerHost::new();
    let cmd = WorkerHost::parse_message(&init_json("disabled")).unwrap();
    h.init_native(&cmd).unwrap();
    h
}

#[test]
fn commands_before_init_are_rejected_and_init_announces_cpu_backend() {
    let mut h = WorkerHost::new();
    h.handle_json(r#"{"type":"STEP","ticks":2}"#);
    h.handle_json("{not json");
    h.handle_json(&init_json("disabled"));
    assert_eq!(error_codes(&h.drain_events()), vec![0x3003, 0x3003, 0x3003]);
    assert_eq!(h.phase(), HostPhase::AwaitingInit);

    let cmd = WorkerHost::parse_message(&init_json("disabled")).unwrap();
    h.init_native(&cmd).unwrap();
    assert_eq!(h.phase(), HostPhase::Ready);
    let ev = h.drain_events();
    assert!(matches!(
        ev.as_slice(),
        [EngineEvent::EngineReady {
            backend: ComputeBackendKind::CpuReference,
            profile: None,
            generation: 0,
            ..
        }]
    ));
    // 第二次 ENGINE_INIT：无论走哪个入口都被拒绝
    h.handle_json(&init_json("auto"));
    assert!(h.init_native(&cmd).is_err());
    assert_eq!(error_codes(&h.drain_events()), vec![0x3003, 0x3003]);
    assert_eq!(h.phase(), HostPhase::Ready);
}

#[test]
fn step_pause_start_follow_fixed_tick_rate() {
    let mut h = cpu_host();
    h.drain_events();
    h.handle_json(r#"{"type":"STEP","ticks":3}"#);
    let ticks: Vec<u64> = committed(&h.drain_events()).into_iter().map(|(t, _)| t).collect();
    assert_eq!(ticks, vec![1, 2, 3]);

    // 暂停状态下帧回调不推进 Tick
    assert_eq!((0..10).map(|i| h.on_frame(f64::from(i) * 16.0)).sum::<u32>(), 0);

    h.handle_json(r#"{"type":"START","tickRate":30}"#);
    let mut total = 0;
    for i in 0..=60 {
        total += h.on_frame(10_000.0 + f64::from(i) * 1000.0 / 60.0);
    }
    assert_eq!(total, 30, "60 Hz 帧率下 1 秒恰好 30 Tick");
    let ticks: Vec<u64> = committed(&h.drain_events()).into_iter().map(|(t, _)| t).collect();
    assert_eq!(ticks, (4..=33).collect::<Vec<_>>());

    h.handle_json(r#"{"type":"PAUSE"}"#);
    assert_eq!(h.on_frame(20_000.0), 0);
    assert_eq!(h.on_frame(30_000.0), 0);
    assert_eq!(h.engine().unwrap().last_tick(), 33);
}

#[test]
fn commands_during_async_init_are_buffered_and_replayed_in_order() {
    let mut h = WorkerHost::new();
    let cmd = WorkerHost::parse_message(&init_json("disabled")).unwrap();
    let plan = h.begin_init(&cmd).unwrap();
    assert_eq!(h.phase(), HostPhase::Initializing);
    h.handle_json(r#"{"type":"STEP","ticks":2}"#);
    h.handle_json(r#"{"type":"SET_HEATMAP_CHANNEL","channel":"plasma"}"#);
    h.handle_json(r#"{"type":"STEP"}"#);
    h.handle_json(&init_json("disabled"));
    // 初始化尚未完成：只有重复 ENGINE_INIT 的错误
    assert_eq!(error_codes(&h.drain_events()), vec![0x3003]);

    h.complete_init(Engine::new(plan.config));
    let ev = h.drain_events();
    assert!(matches!(ev[0], EngineEvent::EngineReady { .. }));
    assert_eq!(
        committed(&ev).into_iter().map(|(t, _)| t).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    // 未知热力图通道在回放时按序报错（0x1004），不影响后续命令
    assert_eq!(error_codes(&ev), vec![0x1004]);
}

#[test]
fn failed_required_init_and_shutdown_are_terminal() {
    let mut h = WorkerHost::new();
    let cmd = WorkerHost::parse_message(&init_json("required")).unwrap();
    h.begin_init(&cmd).unwrap();
    h.handle_json(r#"{"type":"STEP"}"#);
    h.fail_init(&rynvek_wgpu::GpuError::adapter_unavailable("测试：无适配器").into());
    assert_eq!(h.phase(), HostPhase::ShutDown);
    assert_eq!(error_codes(&h.drain_events()), vec![0x6001]);
    h.handle_json(r#"{"type":"STEP"}"#);
    assert!(h.drain_events().is_empty(), "关闭后静默丢弃");

    let mut h = cpu_host();
    h.handle_json(r#"{"type":"START"}"#);
    h.handle_json(r#"{"type":"SHUTDOWN"}"#);
    assert_eq!(h.phase(), HostPhase::ShutDown);
    h.drain_events();
    assert_eq!(h.on_frame(0.0) + h.on_frame(1000.0), 0);
    h.handle_json(r#"{"type":"STEP"}"#);
    assert!(h.drain_events().is_empty());
}

#[test]
fn pick_without_renderer_answers_miss_with_same_request_id() {
    let mut h = cpu_host();
    h.drain_events();
    h.handle_json(r#"{"type":"PICK_REQUEST","requestId":41,"x":3,"y":4}"#);
    assert_eq!(
        h.drain_events(),
        vec![EngineEvent::PickResult {
            request_id: 41,
            dense_index: None
        }]
    );
}

/// 有适配器时：经宿主走完整 DeviceLost 恢复；同一命令序列下 GPU 宿主与纯 CPU 宿主的状态根逐 Tick 相同
#[test]
fn host_drives_real_device_lost_recovery_without_touching_state_roots() {
    let mut gpu = WorkerHost::new();
    let cmd = WorkerHost::parse_message(&init_json("auto")).unwrap();
    gpu.init_native(&cmd).unwrap();
    let ready = gpu.drain_events();
    let Some(EngineEvent::EngineReady { backend, adapter, .. }) = ready.first() else {
        panic!("缺少 ENGINE_READY：{ready:?}");
    };
    if *backend != ComputeBackendKind::Webgpu {
        assert!(!require_gpu(), "RYNVEK_REQUIRE_GPU=1 但宿主未获得 GPU");
        eprintln!("[rynvek-wasm] 跳过 GPU 宿主测试：无适配器");
        return;
    }
    eprintln!("[rynvek-wasm] 适配器：{adapter:?}");
    let mut cpu = cpu_host();
    cpu.drain_events();

    let script = [
        r#"{"type":"STEP","ticks":5}"#,
        r#"{"type":"SIMULATE_DEVICE_LOST"}"#,
        r#"{"type":"STEP","ticks":5}"#,
    ];
    let mut gpu_events = Vec::new();
    let mut cpu_events = Vec::new();
    for msg in script {
        gpu.handle_json(msg);
        cpu.handle_json(msg);
        gpu_events.extend(gpu.drain_events());
        cpu_events.extend(cpu.drain_events());
    }
    // 只泵送不推进，直到恢复完成（主循环从不等待；测试以帧回调轮询）
    let mut now = 0.0;
    for _ in 0..5000 {
        gpu.on_frame(now);
        now += 16.0;
        gpu_events.extend(gpu.drain_events());
        if gpu_events.iter().any(|e| matches!(e, EngineEvent::Recovered { .. })) {
            break;
        }
        if let Some(o) = gpu.engine().and_then(Engine::gpu_owner) {
            let _ = o.wait_idle();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let lost = gpu_events
        .iter()
        .position(|e| matches!(e, EngineEvent::DeviceLost { .. }))
        .expect("应上报 DEVICE_LOST");
    let recovered = gpu_events
        .iter()
        .position(|e| matches!(e, EngineEvent::Recovered { generation: 1, .. }))
        .expect("应以第 1 代设备恢复");
    let steps: Vec<u8> = gpu_events[lost..recovered]
        .iter()
        .filter_map(|e| match e {
            EngineEvent::RecoveryProgress { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    assert_eq!(steps, vec![1, 2, 3, 4, 5]);

    gpu.handle_json(r#"{"type":"STEP","ticks":3}"#);
    cpu.handle_json(r#"{"type":"STEP","ticks":3}"#);
    gpu_events.extend(gpu.drain_events());
    cpu_events.extend(cpu.drain_events());
    let g = committed(&gpu_events);
    assert_eq!(g.len(), 13);
    assert_eq!(g, committed(&cpu_events), "GPU 镜像与恢复流程绝不改变权威状态根");
    assert!(error_codes(&gpu_events).is_empty(), "{gpu_events:?}");
    let e = gpu.engine().unwrap();
    assert_eq!(e.backend(), ComputeBackendKind::Webgpu);
    assert_eq!(e.stats().divergences, 0);
}

#[test]
fn engine_config_from_init_carries_world_id_and_viewport() {
    let mut h = WorkerHost::new();
    let id = "00112233445566778899aabbccddeeff";
    let cmd = EngineCommand::EngineInit {
        width: 640,
        height: 360,
        device_pixel_ratio: 2.0,
        world_id: Some(id.into()),
        gpu_mode: GpuMode::Disabled,
        arena_faction: None,
    };
    let plan = h.begin_init(&cmd).unwrap();
    assert_eq!(
        plan.config,
        EngineConfig {
            world_id: rynvek_engine::protocol::parse_world_id(id).unwrap(),
            gpu_mode: GpuMode::Disabled,
            viewport: [640, 360],
            ..EngineConfig::default()
        }
    );
    assert_eq!(plan.device_pixel_ratio, 2.0);
}

#[test]
fn remote_arena_worker_never_advances_a_parallel_simulation() {
    let mut host = WorkerHost::new();
    let init = WorkerHost::parse_message(
        r#"{"type":"ENGINE_INIT","width":256,"height":256,"gpuMode":"disabled","arenaFaction":0}"#,
    )
    .unwrap();
    host.init_native(&init).unwrap();
    host.drain_events();
    host.handle_json(r#"{"type":"STEP","ticks":1}"#);
    assert_eq!(
        host.engine().unwrap().last_tick(),
        0,
        "远程阵营显示宿主不得推进演示世界"
    );
    assert!(
        host.drain_events()
            .iter()
            .any(|ev| matches!(ev, EngineEvent::EngineError { .. }))
    );
}

#[test]
fn remote_ready_confirms_the_configured_read_only_faction() {
    for faction in [0, 255] {
        let mut host = WorkerHost::new();
        let init = WorkerHost::parse_message(&format!(
            r#"{{"type":"ENGINE_INIT","width":256,"height":256,"gpuMode":"disabled","arenaFaction":{faction}}}"#,
        ))
        .unwrap();
        host.init_native(&init).unwrap();
        let events = serde_json::to_value(host.drain_events()).unwrap();
        assert_eq!(events[0]["type"], "ENGINE_READY");
        assert_eq!(events[0]["arenaFaction"], faction, "必须回显实际初始化的只读阵营能力");
        host.handle_json(r#"{"type":"START","tickRate":30}"#);
        assert!(!error_codes(&host.drain_events()).is_empty());
        assert_eq!(host.engine().unwrap().last_tick(), 0);
    }
}
