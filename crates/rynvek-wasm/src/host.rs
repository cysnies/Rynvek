// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Worker 宿主状态机（与平台无关，原生可测）。
//!
//! ```text
//! AwaitingInit ──ENGINE_INIT──► Initializing ──设备就绪/降级──► Ready ──SHUTDOWN──► ShutDown
//!        │                           │ 期间到达的命令按序缓存，就绪后回放
//!        └──其他命令：ENGINE_ERROR    └──gpuMode=required 且无设备：ENGINE_ERROR(0x6001) → ShutDown
//! ```
//!
//! 画布所有权经 `transferControlToOffscreen` 永久移交，`ENGINE_INIT` 在整个 Worker 生命周期内只允许一次。

use std::collections::VecDeque;

use rynvek_core::RynvekErrorCode;
use rynvek_engine::protocol::parse_world_id;
use rynvek_engine::{Engine, EngineCommand, EngineConfig, EngineError, EngineEvent};

use crate::scheduler::TickScheduler;

/// 初始化期间最多缓存的命令数（防止主线程失控灌入）
pub const MAX_PENDING_COMMANDS: usize = 256;

/// 宿主阶段
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostPhase {
    AwaitingInit,
    Initializing,
    Ready,
    ShutDown,
}

/// `ENGINE_INIT` 校验后的初始化计划（设备获取由具体宿主异步 / 阻塞完成）
#[derive(Clone, Debug, PartialEq)]
pub struct InitPlan {
    pub config: EngineConfig,
    pub device_pixel_ratio: f32,
}

/// Worker 宿主
#[derive(Debug)]
pub struct WorkerHost {
    phase: HostPhase,
    engine: Option<Engine>,
    scheduler: TickScheduler,
    pending: VecDeque<EngineCommand>,
    events: Vec<EngineEvent>,
}

impl Default for WorkerHost {
    fn default() -> Self {
        Self::new()
    }
}

fn protocol_error(message: impl Into<String>) -> EngineError {
    EngineError {
        code: RynvekErrorCode::SimulationInvariantViolated,
        message: message.into(),
    }
}

impl WorkerHost {
    pub fn new() -> Self {
        Self {
            phase: HostPhase::AwaitingInit,
            engine: None,
            scheduler: TickScheduler::new(30),
            pending: VecDeque::new(),
            events: Vec::new(),
        }
    }

    pub fn phase(&self) -> HostPhase {
        self.phase
    }

    pub fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }

    pub fn engine_mut(&mut self) -> Option<&mut Engine> {
        self.engine.as_mut()
    }

    /// 解析一条 `postMessage` 负载（JSON，`canvas` 字段被忽略）
    pub fn parse_message(json: &str) -> Result<EngineCommand, EngineError> {
        EngineCommand::from_json(json).map_err(|e| protocol_error(format!("无法解析 Worker 消息：{e}")))
    }

    /// 校验 `ENGINE_INIT` 并进入 `Initializing`
    pub fn begin_init(&mut self, cmd: &EngineCommand) -> Result<InitPlan, EngineError> {
        let EngineCommand::EngineInit {
            width,
            height,
            device_pixel_ratio,
            world_id,
            gpu_mode,
        } = cmd
        else {
            return Err(protocol_error("begin_init 只接受 ENGINE_INIT"));
        };
        if self.phase != HostPhase::AwaitingInit {
            return Err(protocol_error("ENGINE_INIT 只允许一次：画布所有权已永久移交至 Worker"));
        }
        if *width == 0 || *height == 0 {
            return Err(EngineError {
                code: RynvekErrorCode::DimensionMismatch,
                message: format!("画布尺寸非法：{width}×{height}"),
            });
        }
        let world_id = match world_id {
            None => [0; 16],
            Some(hex) => parse_world_id(hex).ok_or_else(|| EngineError {
                code: RynvekErrorCode::DimensionMismatch,
                message: format!("worldId 必须是 32 位十六进制：{hex}"),
            })?,
        };
        self.phase = HostPhase::Initializing;
        Ok(InitPlan {
            config: EngineConfig {
                world_id,
                gpu_mode: *gpu_mode,
                viewport: [*width, *height],
                ..EngineConfig::default()
            },
            device_pixel_ratio: *device_pixel_ratio,
        })
    }

    /// 设备阶段完成：接管引擎、推送 `ENGINE_READY` 并回放缓存命令
    pub fn complete_init(&mut self, mut engine: Engine) {
        engine.announce_ready();
        self.engine = Some(engine);
        self.phase = HostPhase::Ready;
        self.collect_engine_events();
        while let Some(cmd) = self.pending.pop_front() {
            self.dispatch_ready(cmd);
            if self.phase != HostPhase::Ready {
                break;
            }
        }
        self.pending.clear();
    }

    /// 设备阶段失败（如 `gpuMode = required` 无适配器）：上报错误并终止
    pub fn fail_init(&mut self, err: &EngineError) {
        self.events.push(err.to_event());
        self.pending.clear();
        self.phase = HostPhase::ShutDown;
    }

    /// 处理一条非 `ENGINE_INIT` 命令（`ENGINE_INIT` 必须携带画布，走 [`Self::begin_init`]）
    pub fn handle(&mut self, cmd: EngineCommand) {
        match self.phase {
            HostPhase::AwaitingInit => {
                let msg = if matches!(cmd, EngineCommand::EngineInit { .. }) {
                    "ENGINE_INIT 必须经由画布入口提交"
                } else {
                    "引擎尚未初始化：首条消息必须是 ENGINE_INIT"
                };
                self.events.push(protocol_error(msg).to_event());
            }
            HostPhase::Initializing => {
                if matches!(cmd, EngineCommand::EngineInit { .. }) {
                    self.events
                        .push(protocol_error("ENGINE_INIT 只允许一次：画布所有权已永久移交至 Worker").to_event());
                } else if self.pending.len() >= MAX_PENDING_COMMANDS {
                    self.events
                        .push(protocol_error("初始化期间缓存命令过多，已丢弃").to_event());
                } else {
                    self.pending.push_back(cmd);
                }
            }
            HostPhase::Ready => self.dispatch_ready(cmd),
            // 关闭后静默丢弃
            HostPhase::ShutDown => {}
        }
    }

    /// 解析并处理一条 JSON 消息
    pub fn handle_json(&mut self, json: &str) {
        match Self::parse_message(json) {
            Ok(cmd) => self.handle(cmd),
            Err(e) => self.events.push(e.to_event()),
        }
    }

    fn dispatch_ready(&mut self, cmd: EngineCommand) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if matches!(cmd, EngineCommand::EngineInit { .. }) {
            self.events
                .push(protocol_error("ENGINE_INIT 只允许一次：画布所有权已永久移交至 Worker").to_event());
            return;
        }
        let pause_or_start = matches!(cmd, EngineCommand::Pause | EngineCommand::Start { .. });
        if let Err(e) = engine.handle_command(cmd) {
            engine.drain_events().into_iter().for_each(|ev| self.events.push(ev));
            self.events.push(e.to_event());
        }
        if pause_or_start {
            self.scheduler.set_tick_rate(engine.tick_rate());
        }
        if engine.is_shutdown() {
            self.phase = HostPhase::ShutDown;
        }
        self.collect_engine_events();
    }

    /// 帧回调（`requestAnimationFrame` 等价物）：按固定步长推进并泵送异步 GPU 事件，返回执行的 Tick 数
    pub fn on_frame(&mut self, now_ms: f64) -> u32 {
        let Some(engine) = self.engine.as_mut() else {
            return 0;
        };
        if self.phase != HostPhase::Ready {
            return 0;
        }
        let ticks = if engine.is_running() {
            if self.scheduler.tick_rate() != engine.tick_rate() {
                self.scheduler.set_tick_rate(engine.tick_rate());
            }
            self.scheduler.advance(now_ms)
        } else {
            self.scheduler.reset();
            0
        };
        let mut ran = 0;
        if ticks == 0 {
            engine.pump();
        } else {
            for _ in 0..ticks {
                match engine.step() {
                    Ok(_) => ran += 1,
                    Err(e) => {
                        // 仿真错误：停止连续运行并上报，等待主线程决策
                        let _ = engine.handle_command(EngineCommand::Pause);
                        engine.drain_events().into_iter().for_each(|ev| self.events.push(ev));
                        self.events.push(e.to_event());
                        break;
                    }
                }
            }
        }
        self.collect_engine_events();
        ran
    }

    fn collect_engine_events(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            self.events.extend(engine.drain_events());
        }
    }

    /// 取出待发送给主线程的事件（按产生顺序）
    pub fn drain_events(&mut self) -> Vec<EngineEvent> {
        self.collect_engine_events();
        std::mem::take(&mut self.events)
    }

    /// 取出事件并编码为 JSON 数组（ABI 桥直接交给 `JSON.parse`）
    pub fn drain_events_json(&mut self) -> String {
        events_to_json(&self.drain_events())
    }

    /// 原生宿主的完整初始化（阻塞获取设备，仅初始化阶段 / 测试使用）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn init_native(&mut self, cmd: &EngineCommand) -> Result<(), EngineError> {
        let plan = match self.begin_init(cmd) {
            Ok(p) => p,
            Err(e) => {
                self.events.push(e.to_event());
                return Err(e);
            }
        };
        use rynvek_engine::GpuMode;
        let mode = plan.config.gpu_mode;
        let mut engine = Engine::new(plan.config);
        if mode != GpuMode::Disabled {
            match rynvek_wgpu::GpuDeviceOwner::request_blocking(rynvek_wgpu::GpuDeviceRequest::from_env()) {
                Ok(owner) => {
                    if let Err(e) = engine.attach_gpu(owner) {
                        self.fail_init(&e);
                        return Err(e);
                    }
                }
                Err(e) if mode == GpuMode::Required => {
                    let e = EngineError::from(e);
                    self.fail_init(&e);
                    return Err(e);
                }
                Err(_) => {}
            }
        }
        self.complete_init(engine);
        Ok(())
    }
}

/// 事件列表 → JSON 数组
pub fn events_to_json(events: &[EngineEvent]) -> String {
    let parts: Vec<String> = events.iter().map(EngineEvent::to_json).collect();
    format!("[{}]", parts.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_engine::GpuMode;

    fn init_cmd() -> EngineCommand {
        EngineCommand::EngineInit {
            width: 320,
            height: 240,
            device_pixel_ratio: 1.0,
            world_id: None,
            gpu_mode: GpuMode::Disabled,
        }
    }

    #[test]
    fn begin_init_validates_dimensions_world_id_and_only_once() {
        let mut h = WorkerHost::new();
        let bad = EngineCommand::EngineInit {
            width: 0,
            height: 1,
            device_pixel_ratio: 1.0,
            world_id: None,
            gpu_mode: GpuMode::Auto,
        };
        assert_eq!(h.begin_init(&bad).unwrap_err().code, RynvekErrorCode::DimensionMismatch);
        let bad_id = EngineCommand::EngineInit {
            width: 1,
            height: 1,
            device_pixel_ratio: 1.0,
            world_id: Some("xyz".into()),
            gpu_mode: GpuMode::Auto,
        };
        assert!(h.begin_init(&bad_id).is_err());
        assert_eq!(h.phase(), HostPhase::AwaitingInit);
        let plan = h.begin_init(&init_cmd()).unwrap();
        assert_eq!(plan.config.viewport, [320, 240]);
        assert_eq!(h.phase(), HostPhase::Initializing);
        assert_eq!(
            h.begin_init(&init_cmd()).unwrap_err().code,
            RynvekErrorCode::SimulationInvariantViolated
        );
        assert!(h.begin_init(&EngineCommand::Pause).is_err());
    }

    #[test]
    fn events_json_is_an_array() {
        assert_eq!(events_to_json(&[]), "[]");
        let ev = EngineEvent::FrameRendered { tick: 2 };
        assert_eq!(
            events_to_json(&[ev.clone(), ev]),
            r#"[{"type":"FRAME_RENDERED","tick":2},{"type":"FRAME_RENDERED","tick":2}]"#
        );
    }
}
