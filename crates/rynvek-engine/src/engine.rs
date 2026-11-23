// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek Engine 顶层门面。
//!
//! 职责划分：
//! - **权威**：`rynvek-simulation` 的 16 阶段 CPU Reference（D0_Strict）产出每 Tick 的 `TickCommitManifest`，
//!   GPU 永远不参与状态根计算，因此设备丢失、降级或驱动差异都不会改变确定性结果；
//! - **GPU 驻留镜像**：五通道场在 GPU 上以逐位一致的定点内核同步推进，供 L1 热力图零回读绑定，
//!   并经 3-Slot 回读环（`Slot[tick % 3]`，非阻塞）异步核对守恒摘要与 `s_diagnostics`（INV-01）；
//!   摘要不一致时记一次分歧并从 CPU 权威状态重新同步；
//! - **DeviceLost 编排**：驱动 [`DeviceLostRecovery`] 5 步状态机——隔离 → 释放 → 重获设备 →
//!   重建并从 CPU `Committed` 状态恢复 → Philox 差分探针校验；失败次数耗尽则降级至 CPU Reference；
//!   整个过程不阻塞主循环（校验探针经一次性 `map_async` 回读轮询完成）；
//! - **渲染与拾取**：`rynvek-render` 的 [`WorldRenderer`] 与非阻塞 GPU ID 拾取。

use std::collections::VecDeque;

use rynvek_compute::CapabilityProfile;
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::rng::philox4x32_10;
use rynvek_field::channels::FieldChannelId;
use rynvek_field::solver::{HALO, STRIDE, TILE_DIM};
use rynvek_render::{
    Camera2D, FieldOverlay, PickReadback, RenderLayerMask, WorldRenderer, collect_scene, renderer::create_pick_texture,
};
use rynvek_simulation::engine::{SimulationEngine, SimulationError};
use rynvek_simulation::phase::TickCommitManifest;
use rynvek_wgpu::kernel::GpuDiagnostics;
use rynvek_wgpu::kernels::{FieldKernels, PhiloxKernel};
use rynvek_wgpu::{
    DeviceLostRecovery, GpuDeviceOwner, GpuError, GpuResult, OneShotPoll, OneShotReadback, ReadbackRing, RecoveryEvent,
    RecoveryOutcome, RecoveryPhase,
};

use crate::gpu_field::GpuFieldMirror;
use crate::protocol::{ComputeBackendKind, EngineCommand, EngineEvent, GpuMode, to_hex};
use crate::provider::EntityProvider;

/// CPU 摘要历史保留的 Tick 数（覆盖回读环延迟；软件光栅器上映射可能滞后数十个 Tick）
const CPU_SUM_HISTORY: usize = 64;
/// 恢复校验探针的 Philox 样本数
const PROBE_SAMPLES: usize = 64;

/// 引擎配置
#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    pub world_id: [u8; 16],
    pub gpu_mode: GpuMode,
    /// Some 时仅呈现该阵营服务器观察，不运行本地仿真。
    pub arena_faction: Option<u8>,
    /// DeviceLost 恢复最大失败次数（达到即降级）
    pub max_recovery_attempts: u32,
    /// 画布设备像素尺寸
    pub viewport: [u32; 2],
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            world_id: [0; 16],
            gpu_mode: GpuMode::Auto,
            arena_faction: None,
            max_recovery_attempts: DeviceLostRecovery::DEFAULT_MAX_ATTEMPTS,
            viewport: [1280, 720],
        }
    }
}

/// 引擎错误（统一错误码）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub code: RynvekErrorCode,
    pub message: String,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EngineError {}

impl From<GpuError> for EngineError {
    fn from(e: GpuError) -> Self {
        Self {
            code: e.code,
            message: e.message,
        }
    }
}

impl From<SimulationError> for EngineError {
    fn from(e: SimulationError) -> Self {
        let (code, message) = match e {
            SimulationError::Invariant(c) => (c, "不变量检查失败".to_string()),
            SimulationError::Field(c) => (c, "场求解失败".to_string()),
            SimulationError::Spatial(c) => (c, "空间索引失败".to_string()),
            SimulationError::Dispatch(m) => (RynvekErrorCode::SimulationInvariantViolated, m),
        };
        Self { code, message }
    }
}

impl EngineError {
    pub fn to_event(&self) -> EngineEvent {
        EngineEvent::EngineError {
            code: self.code as u16,
            message: self.message.clone(),
        }
    }
}

/// 运行统计
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EngineStats {
    /// GPU 镜像推进的 Tick 数
    pub gpu_ticks: u64,
    /// 回读摘要与 CPU 一致的 Tick 数
    pub verified_ticks: u64,
    /// 摘要不一致次数
    pub divergences: u64,
    /// 回读环忙槽跳过次数
    pub skipped_readbacks: u64,
    /// 就绪后未及取走即被覆盖的回读数
    pub dropped_readbacks: u64,
    /// 送达时 CPU 摘要历史已过期、无法核对的回读数
    pub stale_readbacks: u64,
    /// 从 CPU 重新上传镜像的次数
    pub resyncs: u64,
}

/// 故障注入（测试与 Studio 演练用）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaultInjection {
    /// 接下来 N 次重获设备直接失败
    pub fail_reacquire: u32,
    /// 接下来 N 次校验探针判定失败
    pub fail_verification: u32,
}

#[derive(Debug)]
struct RenderResources {
    renderer: WorldRenderer,
    pick_texture: wgpu::Texture,
    pick_view: wgpu::TextureView,
    picker: PickReadback,
}

#[derive(Debug)]
struct GpuResources {
    diag: GpuDiagnostics,
    field_kernels: FieldKernels,
    mirror: GpuFieldMirror,
    ring: ReadbackRing,
    philox: PhiloxKernel,
    render: Option<RenderResources>,
}

#[derive(Debug)]
struct PendingVerification {
    readback: OneShotReadback,
    expected: Vec<[u32; 4]>,
    force_fail: bool,
}

/// 引擎门面
#[derive(Debug)]
pub struct Engine {
    config: EngineConfig,
    sim: SimulationEngine,
    arena_observation: Option<Box<rynvek_rts::observation::FactionObservation>>,
    arena_map: Option<Box<crate::protocol::ArenaMapData>>,
    arena_selection: crate::observation::Selection,
    arena_theme: crate::observation::ArenaTheme,
    owner: Option<GpuDeviceOwner>,
    gpu: Option<GpuResources>,
    recovery: DeviceLostRecovery,
    verification: Option<PendingVerification>,
    awaiting_external_reacquire: bool,
    events: VecDeque<EngineEvent>,
    cpu_sums: VecDeque<(u64, [i64; 5])>,
    stats: EngineStats,
    field_dirty: bool,
    render_format: Option<wgpu::TextureFormat>,
    last_tick: u64,
    running: bool,
    tick_rate: u32,
    shutdown: bool,
    pub camera: Camera2D,
    pub layer_mask: RenderLayerMask,
    pub heatmap_channel: FieldChannelId,
    /// 热力图顶端对应的 Q8.8 原始值
    pub heatmap_value_max: f32,
    pub faults: FaultInjection,
}

impl Engine {
    /// 仅 CPU Reference 的引擎（GPU 由宿主随后经 [`Self::attach_gpu`] 接入）
    pub fn new(config: EngineConfig) -> Self {
        let camera = Camera2D::fit(TILE_DIM as f32, config.viewport);
        Self {
            sim: SimulationEngine::new(config.world_id),
            arena_observation: None,
            arena_map: None,
            arena_selection: Default::default(),
            arena_theme: Default::default(),
            recovery: DeviceLostRecovery::new(config.max_recovery_attempts),
            config,
            owner: None,
            gpu: None,
            verification: None,
            awaiting_external_reacquire: false,
            events: VecDeque::new(),
            cpu_sums: VecDeque::new(),
            stats: EngineStats::default(),
            field_dirty: false,
            render_format: None,
            last_tick: 0,
            running: false,
            tick_rate: 30,
            shutdown: false,
            camera,
            layer_mask: RenderLayerMask::ALL,
            heatmap_channel: FieldChannelId::Nutrient,
            heatmap_value_max: 1024.0,
            faults: FaultInjection::default(),
        }
    }

    /// 原生宿主：按 `gpu_mode` 阻塞获取设备（仅初始化阶段）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_native(config: EngineConfig) -> Result<Self, EngineError> {
        let mode = config.gpu_mode;
        let mut engine = Self::new(config);
        if mode != GpuMode::Disabled {
            match GpuDeviceOwner::request_blocking(rynvek_wgpu::GpuDeviceRequest::from_env()) {
                Ok(owner) => engine.attach_gpu(owner)?,
                Err(e) if mode == GpuMode::Required => return Err(e.into()),
                Err(_) => {}
            }
        }
        engine.announce_ready();
        Ok(engine)
    }

    /// 接入独占设备并建立 GPU 镜像（从 CPU 当前权威状态上传）
    pub fn attach_gpu(&mut self, owner: GpuDeviceOwner) -> Result<(), EngineError> {
        self.owner = Some(owner);
        if let Err(e) = self.rebuild_resources() {
            self.owner = None;
            self.gpu = None;
            if self.config.gpu_mode == GpuMode::Required {
                return Err(e.into());
            }
            self.events
                .push_back(EngineEvent::BackendDegraded { reason: e.to_string() });
        }
        Ok(())
    }

    /// 推送 `ENGINE_READY`
    pub fn announce_ready(&mut self) {
        let (profile, adapter, generation) = match &self.owner {
            Some(o) => (
                Some(format!("{:?}", o.profile())),
                Some(format!("{} ({:?})", o.adapter_info().name, o.adapter_info().backend)),
                o.generation(),
            ),
            None => (None, None, 0),
        };
        self.events.push_back(EngineEvent::EngineReady {
            backend: self.backend(),
            profile,
            adapter,
            generation,
            arena_faction: self.config.arena_faction,
        });
    }

    /// 启用渲染（画布 / 离屏颜色格式）
    pub fn enable_rendering(&mut self, format: wgpu::TextureFormat) -> Result<(), EngineError> {
        self.render_format = Some(format);
        if let (Some(owner), Some(gpu)) = (&self.owner, &mut self.gpu) {
            gpu.render = Some(Self::build_render(owner.device(), format, self.camera.viewport)?);
        }
        Ok(())
    }

    fn build_render(device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2]) -> GpuResult<RenderResources> {
        let pick_texture = create_pick_texture(device, size);
        Ok(RenderResources {
            renderer: WorldRenderer::new(device, format)?,
            pick_view: pick_texture.create_view(&Default::default()),
            pick_texture,
            picker: PickReadback::new(device),
        })
    }

    fn rebuild_resources(&mut self) -> GpuResult<()> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| GpuError::adapter_unavailable("尚未接入设备"))?;
        let device = owner.device();
        let diag = GpuDiagnostics::new(device);
        let field_kernels = FieldKernels::new(device)?;
        let mirror = GpuFieldMirror::new(device, &field_kernels, &self.sim.field, &diag.buffer)?;
        mirror.upload_from(owner.queue(), &self.sim.field)?;
        let render = match self.render_format {
            Some(f) => Some(Self::build_render(device, f, self.camera.viewport)?),
            None => None,
        };
        self.gpu = Some(GpuResources {
            ring: ReadbackRing::new(device, "rynvek.engine.field.summary", GpuFieldMirror::SUMMARY_BYTES),
            philox: PhiloxKernel::new(device)?,
            diag,
            field_kernels,
            mirror,
            render,
        });
        self.field_dirty = false;
        let errors = owner.take_uncaptured_errors();
        if !errors.is_empty() {
            self.gpu = None;
            return Err(GpuError::allocation(errors.join("; ")));
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // 状态查询
    // ------------------------------------------------------------------

    pub fn simulation(&self) -> &SimulationEngine {
        &self.sim
    }

    pub fn stats(&self) -> EngineStats {
        let mut s = self.stats;
        if let Some(g) = &self.gpu {
            s.skipped_readbacks += g.ring.state().skipped;
            s.dropped_readbacks += g.ring.state().dropped;
        }
        s
    }

    pub fn recovery_phase(&self) -> RecoveryPhase {
        self.recovery.phase()
    }

    pub fn recovery(&self) -> &DeviceLostRecovery {
        &self.recovery
    }

    pub fn gpu_owner(&self) -> Option<&GpuDeviceOwner> {
        self.owner.as_ref()
    }

    /// 是否仍有在途的摘要回读
    pub fn readbacks_in_flight(&self) -> bool {
        self.gpu.as_ref().is_some_and(|g| g.ring.has_in_flight())
    }

    pub fn has_gpu_mirror(&self) -> bool {
        self.gpu.is_some()
    }

    /// 当前计算后端
    pub fn backend(&self) -> ComputeBackendKind {
        if self.gpu.is_some() && self.recovery.can_submit_gpu_work() {
            ComputeBackendKind::Webgpu
        } else {
            ComputeBackendKind::CpuReference
        }
    }

    pub fn last_tick(&self) -> u64 {
        self.last_tick
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn tick_rate(&self) -> u32 {
        self.tick_rate
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    /// 取出全部待发事件
    pub fn drain_events(&mut self) -> Vec<EngineEvent> {
        self.events.drain(..).collect()
    }

    // ------------------------------------------------------------------
    // 世界编辑（CPU 权威 + 镜像标脏）
    // ------------------------------------------------------------------

    /// 设置某通道源项（下一 GPU Tick 前整体重同步镜像）
    pub fn set_field_source(&mut self, channel: FieldChannelId, x: usize, y: usize, value_q8: i16) {
        self.sim.field.channel_mut(channel).sources[y * TILE_DIM + x] = value_q8;
        self.field_dirty = true;
    }

    /// 写入场内区脉冲
    pub fn set_field_value(&mut self, channel: FieldChannelId, x: usize, y: usize, value_q8: i16) {
        self.sim.field.set_interior(channel, x, y, value_q8);
        self.field_dirty = true;
    }

    // ------------------------------------------------------------------
    // 主循环
    // ------------------------------------------------------------------

    /// 推进一个权威 Tick
    pub fn step(&mut self) -> Result<TickCommitManifest, EngineError> {
        self.require_local_simulation()?;
        self.pump();
        let manifest = self.sim.tick()?;
        self.last_tick = manifest.tick;
        self.cpu_sums.push_back((manifest.tick, self.sim.field.interior_sums()));
        while self.cpu_sums.len() > CPU_SUM_HISTORY {
            self.cpu_sums.pop_front();
        }
        if self.gpu.is_some() && self.recovery.can_submit_gpu_work() {
            if self.owner.as_ref().is_some_and(GpuDeviceOwner::is_lost) {
                self.on_device_lost(manifest.tick);
            } else {
                self.gpu_tick(manifest.tick);
            }
        }
        self.events.push_back(EngineEvent::TickCommitted {
            tick: manifest.tick,
            state_root: to_hex(&manifest.state_root_hash),
        });
        Ok(manifest)
    }

    /// 连续推进 N 个 Tick
    pub fn run_ticks(&mut self, n: u32) -> Result<Option<TickCommitManifest>, EngineError> {
        let mut last = None;
        for _ in 0..n {
            last = Some(self.step()?);
        }
        Ok(last)
    }

    fn gpu_tick(&mut self, tick: u64) {
        let (Some(owner), Some(gpu)) = (&self.owner, &mut self.gpu) else {
            return;
        };
        let (device, queue) = (owner.device(), owner.queue());
        if self.field_dirty {
            // CPU 已推进到 tick：直接上传推进后的权威状态，本 Tick 不再在 GPU 上重复推进
            if gpu.mirror.upload_from(queue, &self.sim.field).is_ok() {
                self.field_dirty = false;
                self.stats.resyncs += 1;
            }
            return;
        }
        // 复用 CPU 主时钟的场步频：第 1、4、7……Tick 更新，不能每 Tick 偷跑。
        // 脏态上传仍优先执行，否则非场 Tick 的用户编辑会滞留在 GPU 镜像之外。
        if !self.sim.clock.is_field_step_due() {
            return;
        }
        gpu.diag.reset(queue);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rynvek.engine.tick"),
        });
        gpu.mirror.encode_tick(&gpu.field_kernels, &mut enc, &gpu.diag.buffer);
        let slot = gpu.ring.encode_copy(&mut enc, tick, gpu.mirror.summary_buffer(), 0);
        queue.submit([enc.finish()]);
        if let Some(s) = slot {
            gpu.ring.request_map(s);
        }
        self.stats.gpu_ticks += 1;
    }

    /// 非阻塞推进：设备回调、DeviceLost 侦测、恢复状态机、摘要核对与拾取结果
    pub fn pump(&mut self) {
        if let Some(owner) = &self.owner {
            owner.poll_nonblocking();
        }
        let lost_now = self.owner.as_ref().is_some_and(GpuDeviceOwner::is_lost);
        match self.recovery.phase() {
            // Tick 之间侦测到丢失：GPU 需从下一个 Tick 起重新计算
            RecoveryPhase::Healthy if lost_now && self.gpu.is_some() => self.on_device_lost(self.last_tick + 1),
            // 新设备在重建 / 校验期间再次丢失
            RecoveryPhase::Reacquired | RecoveryPhase::Rebuilt if lost_now => {
                let reason = self.lost_reason();
                self.events.push_back(EngineEvent::DeviceLost {
                    tick: self.last_tick + 1,
                    reason,
                });
                self.verification = None;
                self.apply(RecoveryEvent::DeviceLost {
                    tick: self.last_tick + 1,
                });
            }
            _ => {}
        }
        self.drive_recovery();
        self.process_readbacks();
        self.process_picks();
    }

    fn lost_reason(&self) -> String {
        self.owner
            .as_ref()
            .and_then(GpuDeviceOwner::lost_info)
            .map_or_else(|| "unknown".to_string(), |i| format!("{}: {}", i.reason, i.message))
    }

    /// 第 1 步：隔离；`tick` 为 GPU 尚未完成、恢复后需从 CPU 权威状态接续的首个 Tick
    fn on_device_lost(&mut self, tick: u64) {
        let reason = self.lost_reason();
        self.events.push_back(EngineEvent::DeviceLost { tick, reason });
        self.apply(RecoveryEvent::DeviceLost { tick });
        self.drive_recovery();
    }

    fn progress(&mut self, step: u8) {
        self.events.push_back(EngineEvent::RecoveryProgress {
            step,
            phase: format!("{:?}", self.recovery.phase()),
        });
    }

    /// 向状态机投递事件并处理结果
    fn apply(&mut self, event: RecoveryEvent) {
        match self.recovery.handle(event) {
            Ok(RecoveryOutcome::Next(step)) => {
                // 刚完成的步骤 = 下一步骤编号 - 1（失败回退时报告当前待执行步骤之前的阶段）
                self.progress(step as u8 - 1);
            }
            Ok(RecoveryOutcome::Resumed {
                resume_tick,
                generation,
            }) => {
                self.progress(5);
                self.events.push_back(EngineEvent::Recovered {
                    generation,
                    resume_tick,
                });
            }
            Ok(RecoveryOutcome::Degraded) => {
                self.gpu = None;
                self.verification = None;
                self.owner = None;
                self.events.push_back(EngineEvent::BackendDegraded {
                    reason: format!(
                        "DeviceLost 恢复连续失败 {} 次，已降级至 CPU Reference",
                        self.recovery.failed_attempts()
                    ),
                });
            }
            Err(e) => self.events.push_back(EngineEvent::EngineError {
                code: RynvekErrorCode::GpuDeviceLost as u16,
                message: e.to_string(),
            }),
        }
    }

    fn drive_recovery(&mut self) {
        loop {
            match self.recovery.phase() {
                RecoveryPhase::Healthy | RecoveryPhase::Degraded => break,
                RecoveryPhase::Quarantined => {
                    // 第 2 步：释放旧设备上的全部资源
                    self.gpu = None;
                    self.verification = None;
                    self.apply(RecoveryEvent::ResourcesReleased);
                }
                RecoveryPhase::Released => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let result = self.reacquire_blocking();
                        self.complete_reacquire(result);
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        // 浏览器中 requestDevice 只能异步完成：交由 rynvek-wasm 驱动
                        self.awaiting_external_reacquire = true;
                        break;
                    }
                }
                RecoveryPhase::Reacquired => {
                    // 第 4 步：重建管线 / 缓冲并从 CPU 权威状态恢复
                    match self.rebuild_resources() {
                        Ok(()) => self.apply(RecoveryEvent::StateRestored),
                        Err(_) => self.apply(RecoveryEvent::RestoreFailed),
                    }
                }
                RecoveryPhase::Rebuilt => {
                    // 第 5 步：差分探针（非阻塞）
                    if self.verification.is_none() && !self.start_verification() {
                        self.apply(RecoveryEvent::VerificationFailed);
                        continue;
                    }
                    match self.poll_verification() {
                        None => break,
                        Some(true) => self.apply(RecoveryEvent::VerificationPassed),
                        Some(false) => self.apply(RecoveryEvent::VerificationFailed),
                    }
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn reacquire_blocking(&mut self) -> GpuResult<CapabilityProfile> {
        if self.faults.fail_reacquire > 0 {
            self.faults.fail_reacquire -= 1;
            return Err(GpuError::adapter_unavailable("故障注入：重获设备失败"));
        }
        match &mut self.owner {
            Some(owner) => owner.reacquire_blocking(),
            None => Err(GpuError::adapter_unavailable("设备所有者缺失")),
        }
    }

    /// 第 3 步结果（原生内部调用；浏览器由 [`Self::finish_external_reacquire`] 调用）
    fn complete_reacquire(&mut self, result: GpuResult<CapabilityProfile>) {
        match result {
            Ok(profile) => self.apply(RecoveryEvent::DeviceReacquired { profile }),
            Err(_) => self.apply(RecoveryEvent::ReacquireFailed),
        }
    }

    /// 浏览器宿主：是否正等待异步重获设备
    pub fn awaiting_external_reacquire(&self) -> bool {
        self.awaiting_external_reacquire
    }

    /// 浏览器宿主：取出设备所有者以在 `async` 上下文中调用 `reacquire().await`
    pub fn take_owner_for_reacquire(&mut self) -> Option<GpuDeviceOwner> {
        if !self.awaiting_external_reacquire {
            return None;
        }
        if self.faults.fail_reacquire > 0 {
            return None;
        }
        self.owner.take()
    }

    /// 浏览器宿主：归还设备所有者与重获结果，继续恢复流程
    pub fn finish_external_reacquire(&mut self, owner: Option<GpuDeviceOwner>, result: GpuResult<CapabilityProfile>) {
        self.awaiting_external_reacquire = false;
        if owner.is_some() {
            self.owner = owner;
        }
        let result = if self.faults.fail_reacquire > 0 {
            self.faults.fail_reacquire -= 1;
            Err(GpuError::adapter_unavailable("故障注入：重获设备失败"))
        } else {
            result
        };
        self.complete_reacquire(result);
        self.drive_recovery();
    }

    fn start_verification(&mut self) -> bool {
        let (Some(owner), Some(gpu)) = (&self.owner, &self.gpu) else {
            return false;
        };
        let (device, queue) = (owner.device(), owner.queue());
        let key = [self.last_tick as u32, (self.last_tick >> 32) as u32 ^ 0x5259_4E56];
        let counters: Vec<[u32; 4]> = (0..PROBE_SAMPLES as u32)
            .map(|i| [i, owner.generation(), 0x6002, !i])
            .collect();
        let expected = counters.iter().map(|c| philox4x32_10(*c, key)).collect();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rynvek.engine.recovery.probe"),
        });
        let Ok(output) = gpu.philox.encode(device, &mut enc, &gpu.diag.buffer, key, &counters) else {
            return false;
        };
        let mut readback = OneShotReadback::encode(device, &mut enc, &output, (PROBE_SAMPLES * 16) as u64);
        queue.submit([enc.finish()]);
        readback.after_submit();
        let force_fail = self.faults.fail_verification > 0;
        if force_fail {
            self.faults.fail_verification -= 1;
        }
        self.verification = Some(PendingVerification {
            readback,
            expected,
            force_fail,
        });
        true
    }

    /// `None` = 仍在等待 GPU
    fn poll_verification(&mut self) -> Option<bool> {
        if let Some(owner) = &self.owner {
            owner.poll_nonblocking();
        }
        let pending = self.verification.as_ref()?;
        let verdict = match pending.readback.poll() {
            OneShotPoll::Pending => return None,
            OneShotPoll::Failed => false,
            OneShotPoll::Ready(bytes) => {
                let got: Vec<[u32; 4]> = bytemuck::pod_collect_to_vec(&bytes);
                got == pending.expected && !pending.force_fail
            }
        };
        self.verification = None;
        Some(verdict)
    }

    fn process_readbacks(&mut self) {
        let (Some(owner), Some(gpu)) = (&self.owner, &mut self.gpu) else {
            return;
        };
        gpu.ring.poll(owner.device());
        for (tick, bytes) in gpu.ring.drain_ready() {
            let Some(summary) = GpuFieldMirror::parse_summary(&bytes) else {
                continue;
            };
            if summary.diagnostics[0] != 0 {
                // INV-01：GPU 诊断计数器必须为 0；交由 Phase 14 不变量检查终止仿真
                self.sim.diagnostics_nonzero_count += u64::from(summary.diagnostics[0]);
            }
            let Some(&(_, cpu)) = self.cpu_sums.iter().find(|(t, _)| *t == tick) else {
                self.stats.stale_readbacks += 1;
                continue;
            };
            if cpu == summary.interior_sums {
                self.stats.verified_ticks += 1;
            } else {
                self.stats.divergences += 1;
                self.field_dirty = true;
                for (i, id) in FieldChannelId::ALL.iter().enumerate() {
                    if cpu[i] != summary.interior_sums[i] {
                        self.events.push_back(EngineEvent::GpuDivergence {
                            tick,
                            channel: id.as_str().to_string(),
                        });
                    }
                }
            }
        }
    }

    fn process_picks(&mut self) {
        let Some(render) = self.gpu.as_mut().and_then(|g| g.render.as_mut()) else {
            return;
        };
        if let Some(r) = render.picker.try_take() {
            self.events.push_back(EngineEvent::PickResult {
                request_id: r.request_id,
                dense_index: r.id.map(|p| p.dense_index()),
            });
        }
    }

    // ------------------------------------------------------------------
    // 渲染与拾取
    // ------------------------------------------------------------------

    /// 登记拾取请求（画布像素坐标）；无渲染资源时立即回复未命中
    pub fn request_pick(&mut self, request_id: u32, x: u32, y: u32) {
        match self.gpu.as_mut().and_then(|g| g.render.as_mut()) {
            Some(render) if self.config.arena_faction.is_none() => render.picker.request(request_id, x, y),
            _ => self.events.push_back(EngineEvent::PickResult {
                request_id,
                dense_index: None,
            }),
        }
    }

    /// 调整画布尺寸
    pub fn resize(&mut self, width: u32, height: u32) {
        self.camera.viewport = [width.max(1), height.max(1)];
        if let (Some(owner), Some(render)) = (&self.owner, self.gpu.as_mut().and_then(|g| g.render.as_mut())) {
            render.pick_texture = create_pick_texture(owner.device(), self.camera.viewport);
            render.pick_view = render.pick_texture.create_view(&Default::default());
            render.picker.reset();
        }
    }

    fn require_local_simulation(&self) -> Result<(), EngineError> {
        if self.config.arena_faction.is_some() {
            return Err(EngineError {
                code: RynvekErrorCode::SimulationInvariantViolated,
                message: "远程阵营显示禁止 STEP/START；命令必须提交服务器权威宿主".into(),
            });
        }
        Ok(())
    }

    /// 与 GPU 上传共用的只读场景路径；远程模式初始化前为空，不显示四实体演示。
    pub fn frame_scene(&self) -> rynvek_render::FrameScene {
        if self.config.arena_faction.is_some() {
            return self.arena_observation.as_ref().map_or_else(
                || rynvek_render::FrameScene::new(0, rynvek_render::RenderLayerMask::NONE),
                |obs| {
                    crate::observation::presentation_scene(
                        obs,
                        self.layer_mask,
                        self.arena_map.as_deref(),
                        &self.arena_selection,
                        &self.arena_theme,
                    )
                },
            );
        }
        collect_scene(
            &self.sim,
            self.last_tick,
            self.camera.lod(),
            self.layer_mask,
            &[&EntityProvider::default()],
        )
    }

    /// 录制并提交一帧到 `color_view`（尺寸必须等于 `camera.viewport`）；设备不可用时返回 `false`
    pub fn render_frame(&mut self, color_view: &wgpu::TextureView) -> bool {
        if !self.recovery.can_submit_gpu_work() {
            return false;
        }
        let scene = self.frame_scene();
        let (Some(owner), Some(gpu)) = (&self.owner, &mut self.gpu) else {
            return false;
        };
        if owner.is_lost() {
            return false;
        }
        let Some(render) = gpu.render.as_mut() else {
            return false;
        };
        let (device, queue) = (owner.device(), owner.queue());
        let field = FieldOverlay {
            buffer: gpu.mirror.channel_buffer(self.heatmap_channel),
            tile_dim: TILE_DIM as u32,
            halo: HALO as u32,
            origin: [0.0, 0.0],
            cell_size: 1.0,
            value_max: self.heatmap_value_max,
        };
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rynvek.engine.frame"),
        });
        render.renderer.palette.background = self.arena_theme.background;
        render.renderer.palette.fog = self.arena_theme.background.with_alpha(0xc0);
        render.renderer.render(
            device,
            queue,
            &mut enc,
            color_view,
            &render.pick_view,
            &scene,
            &self.camera,
            self.config.arena_faction.is_none().then_some(field),
        );
        render.picker.encode(&mut enc, &render.pick_texture);
        queue.submit([enc.finish()]);
        render.picker.after_submit();
        self.events
            .push_back(EngineEvent::FrameRendered { tick: self.last_tick });
        true
    }

    /// 故障注入：销毁当前设备
    pub fn simulate_device_lost(&mut self) {
        if let Some(owner) = &self.owner {
            owner.destroy_device();
        }
    }

    /// 阻塞回读 GPU 镜像某通道内区（仅测试 / 调试）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn gpu_field_interior_blocking(&self, channel: FieldChannelId) -> Option<Vec<i16>> {
        let (owner, gpu) = (self.owner.as_ref()?, self.gpu.as_ref()?);
        gpu.mirror
            .tile(channel)
            .download_interior_blocking(owner.device(), owner.queue())
            .ok()
    }

    /// CPU 权威通道内区（与 [`Self::gpu_field_interior_blocking`] 同布局）
    pub fn cpu_field_interior(&self, channel: FieldChannelId) -> Vec<i16> {
        let buf = self.sim.field.channel(channel).current_buf();
        let mut out = Vec::with_capacity(TILE_DIM * TILE_DIM);
        for y in 0..TILE_DIM {
            let start = (y + HALO) * STRIDE + HALO;
            out.extend_from_slice(&buf[start..start + TILE_DIM]);
        }
        out
    }

    fn require_arena_binding(&self, world_id: &str) -> Result<(), EngineError> {
        if self.shutdown
            || self.config.arena_faction.is_none()
            || crate::protocol::parse_world_id(world_id) != Some(self.config.world_id)
        {
            return Err(EngineError {
                code: RynvekErrorCode::SimulationInvariantViolated,
                message: "表现消息与只读世界绑定不符".into(),
            });
        }
        Ok(())
    }

    /// 执行一条协议命令（`ENGINE_INIT` 由宿主处理）
    pub fn handle_command(&mut self, cmd: EngineCommand) -> Result<(), EngineError> {
        match cmd {
            EngineCommand::EngineInit { .. } => {}
            EngineCommand::Resize { width, height, .. } => self.resize(width, height),
            EngineCommand::Step { ticks } => {
                self.require_local_simulation()?;
                self.run_ticks(ticks)?;
            }
            EngineCommand::Start { tick_rate } => {
                self.require_local_simulation()?;
                self.running = true;
                self.tick_rate = tick_rate.clamp(1, 240);
            }
            EngineCommand::Pause => self.running = false,
            EngineCommand::SetCamera {
                center_x,
                center_y,
                px_per_cell,
            } => {
                self.camera = Camera2D::new([center_x, center_y], px_per_cell, self.camera.viewport);
            }
            EngineCommand::PickRequest { request_id, x, y } => self.request_pick(request_id, x, y),
            EngineCommand::SetLayerMask { mask } => self.layer_mask = RenderLayerMask(mask),
            EngineCommand::SetHeatmapChannel { channel } => {
                self.heatmap_channel = FieldChannelId::ALL
                    .into_iter()
                    .find(|c| c.as_str() == channel)
                    .ok_or_else(|| EngineError {
                        code: RynvekErrorCode::DimensionMismatch,
                        message: format!("未知场通道 {channel}"),
                    })?;
            }
            EngineCommand::ArenaMap { world_id, map } => {
                self.require_arena_binding(&world_id)?;
                crate::observation::validate_map(&map)?;
                if self.arena_map.as_ref().is_some_and(|old| old != &map)
                    || self
                        .arena_observation
                        .as_ref()
                        .is_some_and(|obs| !crate::observation::map_matches(&map, obs))
                {
                    return Err(EngineError {
                        code: RynvekErrorCode::DimensionMismatch,
                        message: "地图不可替换，且必须匹配观察网格".into(),
                    });
                }
                self.arena_map = Some(map);
            }
            EngineCommand::ArenaTheme {world_id,request_id,background,factions} => {
                self.require_arena_binding(&world_id)?;
                if std::iter::once(&background).chain(factions.iter()).any(|c|c>>24!=255) {
                    return Err(EngineError {code:RynvekErrorCode::DimensionMismatch,message:"主题只许八个不透明颜色，不改变可见性规则".into()});
                }
                self.arena_theme=crate::observation::ArenaTheme {background:rynvek_render::Rgba8(background.to_le_bytes()),factions:factions.map(|c|rynvek_render::Rgba8(c.to_le_bytes()))};
                self.events.push_back(EngineEvent::ArenaThemeApplied {world_id,request_id,background,factions});
            }
            EngineCommand::ArenaSelection {
                world_id,
                units,
                building,
            } => {
                self.require_arena_binding(&world_id)?;
                if units.len() > 65_536 {
                    return Err(EngineError {
                        code: RynvekErrorCode::DimensionMismatch,
                        message: "选择数量超过上限".into(),
                    });
                }
                self.arena_selection.units = units.into_iter().collect();
                self.arena_selection.building = building;
            }
            EngineCommand::ArenaOverlay {
                world_id,
                drag,
                placement,
            } => {
                self.require_arena_binding(&world_id)?;
                if drag
                    .as_ref()
                    .is_some_and(|points| points.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1_048_576.0))
                {
                    return Err(EngineError {
                        code: RynvekErrorCode::DimensionMismatch,
                        message: "拖框坐标不是有界有限值".into(),
                    });
                }
                self.arena_selection.drag = drag;
                self.arena_selection.placement = placement;
            }
            EngineCommand::ArenaObservation { world_id, observation } => {
                if self.shutdown
                    || crate::protocol::parse_world_id(&world_id) != Some(self.config.world_id)
                    || self.config.arena_faction != Some(observation.faction.0)
                    || self
                        .arena_observation
                        .as_ref()
                        .is_some_and(|old| observation.tick < old.tick)
                {
                    return Err(EngineError {
                        code: RynvekErrorCode::SimulationInvariantViolated,
                        message: "观察的世界/阵营/时序与只读宿主不符".into(),
                    });
                }
                crate::observation::validate(&observation)?;
                if self
                    .arena_map
                    .as_ref()
                    .is_some_and(|map| !crate::observation::map_matches(map, &observation))
                {
                    return Err(EngineError {
                        code: RynvekErrorCode::DimensionMismatch,
                        message: "观察与静态地图尺寸不符".into(),
                    });
                }
                self.last_tick = observation.tick;
                self.arena_observation = Some(observation);
            }
            EngineCommand::SimulateDeviceLost => self.simulate_device_lost(),
            EngineCommand::Shutdown => {
                self.running = false;
                self.shutdown = true;
                self.gpu = None;
                self.arena_observation = None;
                self.arena_map = None;
                self.arena_selection = Default::default();
                self.arena_theme = Default::default();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_only_engine_runs_and_reports_cpu_backend() {
        let mut e = Engine::new(EngineConfig {
            gpu_mode: GpuMode::Disabled,
            ..Default::default()
        });
        e.announce_ready();
        let m = e.run_ticks(3).unwrap().unwrap();
        assert_eq!(m.tick, e.last_tick());
        assert_eq!(e.backend(), ComputeBackendKind::CpuReference);
        let events = e.drain_events();
        assert!(matches!(
            events[0],
            EngineEvent::EngineReady {
                backend: ComputeBackendKind::CpuReference,
                ..
            }
        ));
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(ev, EngineEvent::TickCommitted { .. }))
                .count(),
            3
        );
        // 无渲染资源时拾取立即回复未命中
        e.request_pick(4, 0, 0);
        assert_eq!(
            e.drain_events(),
            [EngineEvent::PickResult {
                request_id: 4,
                dense_index: None
            }]
        );
    }

    #[test]
    fn protocol_commands_drive_engine_state() {
        let mut e = Engine::new(EngineConfig::default());
        e.handle_command(EngineCommand::Start { tick_rate: 1000 }).unwrap();
        assert!(e.is_running());
        assert_eq!(e.tick_rate(), 240);
        e.handle_command(EngineCommand::SetHeatmapChannel {
            channel: "hazard".into(),
        })
        .unwrap();
        assert_eq!(e.heatmap_channel, FieldChannelId::Hazard);
        assert!(
            e.handle_command(EngineCommand::SetHeatmapChannel { channel: "x".into() })
                .is_err()
        );
        e.handle_command(EngineCommand::SetLayerMask { mask: 0b1011 }).unwrap();
        assert_eq!(e.layer_mask, RenderLayerMask(0b1011));
        e.handle_command(EngineCommand::Step { ticks: 2 }).unwrap();
        assert_eq!(e.simulation().clock.tick, e.last_tick());
        e.handle_command(EngineCommand::Shutdown).unwrap();
        assert!(e.is_shutdown() && !e.is_running());
    }
}
