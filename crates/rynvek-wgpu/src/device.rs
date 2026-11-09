// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Dedicated Engine Worker 独占的 `wgpu::Device` 所有者（规范 07 §7.1 第 1 条，GAP-02）。
//!
//! - 每个引擎（浏览器中即 Dedicated Engine Worker）只创建**一个** `wgpu::Instance`，
//!   `backends = BROWSER_WEBGPU | PRIMARY`，并请求 `HighPerformance` Adapter；
//! - [`GpuDeviceOwner`] 不可克隆，`Device / Queue` 只以借用形式对外暴露，
//!   所有 GPU 资源都必须经由它创建——TS 层与其他模块不得再建第二套 GPU 上下文；
//! - 设备丢失通过 `set_device_lost_callback` 记录到原子共享状态，由上层的
//!   [`crate::recovery::DeviceLostRecovery`] 驱动 5 步恢复；
//! - **原生回退（ERRATA E-014）**：在无 `PRIMARY` 后端适配器的原生环境（无 GPU 的 CI / 容器）下，
//!   追加尝试 `GL` 后端（如 Mesa llvmpipe），使 CPU/GPU 差分测试可在软件光栅器上真实运行；
//!   可用环境变量 `WGPU_BACKEND`（如 `vulkan`、`gl`）强制指定后端。

use std::sync::{Arc, Mutex};

use rynvek_compute::CapabilityProfile;

use crate::error::{GpuError, GpuResult};
use crate::profile::{AdapterCapabilitySummary, classify_compute_capability};

/// 设备请求参数
#[derive(Clone, Debug)]
pub struct GpuDeviceRequest {
    /// 首选后端（规范：`BROWSER_WEBGPU | PRIMARY`）
    pub backends: wgpu::Backends,
    /// 首选后端无适配器时的回退后端（原生默认 `GL`；浏览器为空）
    pub fallback_backends: wgpu::Backends,
    pub power_preference: wgpu::PowerPreference,
    pub label: &'static str,
}

impl Default for GpuDeviceRequest {
    fn default() -> Self {
        Self {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::PRIMARY,
            fallback_backends: if cfg!(target_arch = "wasm32") {
                wgpu::Backends::empty()
            } else {
                wgpu::Backends::GL
            },
            power_preference: wgpu::PowerPreference::HighPerformance,
            label: "rynvek.engine.device",
        }
    }
}

impl GpuDeviceRequest {
    /// 读取 `WGPU_BACKEND` 环境变量覆盖后端（未设置时与 [`Default`] 相同）
    pub fn from_env() -> Self {
        let mut req = Self::default();
        // 空字符串视为未设置
        if let Some(backends) = wgpu::Backends::from_env().filter(|b| !b.is_empty()) {
            req.backends = backends;
            req.fallback_backends = wgpu::Backends::empty();
        }
        req
    }
}

/// 设备丢失信息
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceLostInfo {
    /// `wgpu::DeviceLostReason` 的名称
    pub reason: String,
    pub message: String,
    /// 丢失的是第几代设备
    pub generation: u32,
}

/// 设备独占所有者
#[derive(Debug)]
pub struct GpuDeviceOwner {
    request: GpuDeviceRequest,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    summary: AdapterCapabilitySummary,
    profile: CapabilityProfile,
    generation: u32,
    lost: Arc<Mutex<Option<DeviceLostInfo>>>,
    errors: Arc<Mutex<Vec<String>>>,
}

fn new_instance(backends: wgpu::Backends) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

async fn request_adapter(instance: &wgpu::Instance, power_preference: wgpu::PowerPreference) -> Option<wgpu::Adapter> {
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        })
        .await
        .ok()
}

struct Opened {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

async fn open_device(
    instance: &wgpu::Instance,
    req: &GpuDeviceRequest,
    generation: u32,
    lost: &Arc<Mutex<Option<DeviceLostInfo>>>,
    errors: &Arc<Mutex<Vec<String>>>,
) -> GpuResult<Opened> {
    let adapter = request_adapter(instance, req.power_preference)
        .await
        .ok_or_else(|| GpuError::adapter_unavailable(format!("后端 {:?} 无可用适配器", req.backends)))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some(req.label),
            required_features: wgpu::Features::empty(),
            // 直接采用适配器上限：分段缓冲需要 ≥ 32 MiB 的存储绑定，WebGPU 默认值即满足
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|e| GpuError::adapter_unavailable(format!("requestDevice 失败：{e}")))?;
    let lost_slot = Arc::clone(lost);
    device.set_device_lost_callback(move |reason, message| {
        if let Ok(mut slot) = lost_slot.lock() {
            slot.get_or_insert(DeviceLostInfo {
                reason: format!("{reason:?}"),
                message,
                generation,
            });
        }
    });
    let error_log = Arc::clone(errors);
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        if let Ok(mut log) = error_log.lock() {
            log.push(e.to_string());
        }
    }));
    Ok(Opened { adapter, device, queue })
}

impl GpuDeviceOwner {
    /// 创建唯一的 Instance 并请求设备；无适配器时报 `WebGpuAdapterUnavailable (0x6001)`
    pub async fn request(req: GpuDeviceRequest) -> GpuResult<Self> {
        let lost = Arc::new(Mutex::new(None));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let mut instance = new_instance(req.backends);
        let opened = match open_device(&instance, &req, 0, &lost, &errors).await {
            Ok(o) => o,
            Err(primary_err) if !req.fallback_backends.is_empty() => {
                instance = new_instance(req.fallback_backends);
                open_device(&instance, &req, 0, &lost, &errors)
                    .await
                    .map_err(|fallback_err| {
                        GpuError::adapter_unavailable(format!(
                            "{}；回退后端：{}",
                            primary_err.message, fallback_err.message
                        ))
                    })?
            }
            Err(e) => return Err(e),
        };
        Ok(Self::assemble(req, instance, opened, 0, lost, errors))
    }

    /// 原生环境下的阻塞封装（只用于初始化与测试，绝不在 Tick 主循环中调用）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_blocking(req: GpuDeviceRequest) -> GpuResult<Self> {
        pollster::block_on(Self::request(req))
    }

    fn assemble(
        request: GpuDeviceRequest,
        instance: wgpu::Instance,
        opened: Opened,
        generation: u32,
        lost: Arc<Mutex<Option<DeviceLostInfo>>>,
        errors: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        let info = opened.adapter.get_info();
        let summary = AdapterCapabilitySummary::from_adapter(&opened.adapter);
        let profile = classify_compute_capability(&summary);
        Self {
            request,
            instance,
            adapter: opened.adapter,
            device: opened.device,
            queue: opened.queue,
            info,
            summary,
            profile,
            generation,
            lost,
            errors,
        }
    }

    /// DeviceLost 恢复第 3 步：在同一 Instance 上重新请求 Adapter 与 Device
    pub async fn reacquire(&mut self) -> GpuResult<CapabilityProfile> {
        let generation = self.generation + 1;
        let lost = Arc::new(Mutex::new(None));
        let opened = open_device(&self.instance, &self.request, generation, &lost, &self.errors).await?;
        let rebuilt = Self::assemble(
            self.request.clone(),
            self.instance.clone(),
            opened,
            generation,
            lost,
            Arc::clone(&self.errors),
        );
        *self = rebuilt;
        Ok(self.profile)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn reacquire_blocking(&mut self) -> GpuResult<CapabilityProfile> {
        pollster::block_on(self.reacquire())
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    pub fn capability_summary(&self) -> &AdapterCapabilitySummary {
        &self.summary
    }

    /// 由 `adapter.limits()` 判定的 `ComputeCapabilityProfile`
    pub fn profile(&self) -> CapabilityProfile {
        self.profile
    }

    /// 当前设备代数（每次成功 [`Self::reacquire`] +1）
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// 设备是否已丢失
    pub fn is_lost(&self) -> bool {
        self.lost.lock().map(|l| l.is_some()).unwrap_or(true)
    }

    pub fn lost_info(&self) -> Option<DeviceLostInfo> {
        self.lost.lock().ok().and_then(|l| l.clone())
    }

    /// 取出并清空未被错误作用域捕获的 wgpu 错误
    pub fn take_uncaptured_errors(&self) -> Vec<String> {
        self.errors
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }

    /// 主动销毁设备：触发与驱动重置 / GPU 进程崩溃相同的 `device.lost` 路径（故障注入与关停用）
    pub fn destroy_device(&self) {
        self.device.destroy();
        // 部分后端在下一次 poll 时才回调 lost
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    /// 非阻塞推进设备回调（主循环中唯一允许的 poll 方式）
    pub fn poll_nonblocking(&self) {
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    /// 阻塞等待全部已提交工作完成：仅用于测试、关停与离线工具，**禁止在 Tick 主循环中调用**
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_idle(&self) -> GpuResult<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| GpuError::device_lost(format!("poll 失败：{e}")))
    }
}
