// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! wasm-bindgen ABI 桥（仅 `wasm32`）：Dedicated Engine Worker 的 JS 可见入口。
//!
//! 本模块是整个工作区中唯一放开 `unsafe_code` 的位置：`#[wasm_bindgen]` 宏展开出的导出符号
//! （`#[export_name]`）与 ABI 胶水会触发该 lint；模块内手写代码本身不含任何 `unsafe` 块。
//!
//! JS 侧（`@rynvek/engine` 的 Worker 脚本）用法：
//!
//! ```text
//! const worker = new EngineWorker();
//! onmessage = async (e) => {
//!   if (e.data.type === "ENGINE_INIT") post(await worker.init(e.data.canvas, JSON.stringify({...e.data, canvas: undefined})));
//!   else post(worker.post(JSON.stringify(e.data)));
//! };
//! const loop = (now) => { post(worker.frame(now)); requestAnimationFrame(loop); };
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use rynvek_engine::{Engine, EngineCommand, EngineError, GpuMode};
use rynvek_wgpu::{GpuDeviceOwner, GpuDeviceRequest};
use wasm_bindgen::prelude::*;
use web_sys::OffscreenCanvas;

use crate::host::{HostPhase, WorkerHost};

/// 画布呈现状态
struct Presenter {
    canvas: OffscreenCanvas,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// 当前配置所对应的设备代数（DeviceLost 重获后需以新设备重新配置）
    generation: Option<u32>,
}

struct Inner {
    host: WorkerHost,
    presenter: Option<Presenter>,
    reacquiring: bool,
}

/// Dedicated Engine Worker 入口对象
#[wasm_bindgen]
pub struct EngineWorker {
    inner: Rc<RefCell<Inner>>,
}

impl std::fmt::Debug for EngineWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineWorker").field("phase", &self.phase()).finish()
    }
}

impl Default for EngineWorker {
    fn default() -> Self {
        Self::new()
    }
}

/// 优先选择非 sRGB 格式（热力图与色板在着色器中已按线性值输出）
fn pick_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| formats.first().copied())
}

impl Presenter {
    fn configure(&mut self, owner: &GpuDeviceOwner) {
        self.surface.configure(owner.device(), &self.config);
        self.generation = Some(owner.generation());
    }

    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        self.canvas.set_width(w);
        self.canvas.set_height(h);
        self.config.width = w;
        self.config.height = h;
        // 下一帧按新尺寸重新配置
        self.generation = None;
    }

    /// 录制一帧；设备 / Surface 不可用时跳过
    fn present(&mut self, engine: &mut Engine) {
        let Some(owner) = engine.gpu_owner() else {
            return;
        };
        if owner.is_lost() {
            return;
        }
        if self.generation != Some(owner.generation()) {
            self.configure(owner);
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.generation = None;
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        if engine.render_frame(&view) {
            if let Some(owner) = engine.gpu_owner() {
                owner.queue().present(frame);
            }
        }
    }
}

#[wasm_bindgen]
impl EngineWorker {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                host: WorkerHost::new(),
                presenter: None,
                reacquiring: false,
            })),
        }
    }

    /// 处理 `ENGINE_INIT`：接管 OffscreenCanvas、创建唯一 Instance 并异步请求设备。
    /// `message_json` 为去掉 `canvas` 字段后的消息体；Promise 解析为事件 JSON 数组。
    pub fn init(&self, canvas: OffscreenCanvas, message_json: String) -> js_sys::Promise {
        let inner = Rc::clone(&self.inner);
        wasm_bindgen_futures::future_to_promise(async move {
            let cmd = match WorkerHost::parse_message(&message_json) {
                Ok(c @ EngineCommand::EngineInit { .. }) => c,
                Ok(other) => {
                    let mut st = inner.borrow_mut();
                    st.host.handle(other);
                    return Ok(JsValue::from_str(&st.host.drain_events_json()));
                }
                Err(e) => return Ok(JsValue::from_str(&crate::host::events_to_json(&[e.to_event()]))),
            };
            let plan = {
                let mut st = inner.borrow_mut();
                match st.host.begin_init(&cmd) {
                    Ok(p) => p,
                    Err(e) => return Ok(JsValue::from_str(&crate::host::events_to_json(&[e.to_event()]))),
                }
            };
            let mode = plan.config.gpu_mode;
            let [width, height] = plan.config.viewport;
            let mut engine = Engine::new(plan.config);
            let mut presenter = None;
            if mode != GpuMode::Disabled {
                // 不持有 RefCell 借用跨越 await：期间到达的命令由宿主缓存
                match GpuDeviceOwner::request(GpuDeviceRequest::default()).await {
                    Ok(owner) => match attach(&mut engine, owner, canvas, width, height) {
                        Ok(p) => presenter = p,
                        Err(e) => {
                            let mut st = inner.borrow_mut();
                            st.host.fail_init(&e);
                            return Ok(JsValue::from_str(&st.host.drain_events_json()));
                        }
                    },
                    Err(e) if mode == GpuMode::Required => {
                        let mut st = inner.borrow_mut();
                        st.host.fail_init(&EngineError::from(e));
                        return Ok(JsValue::from_str(&st.host.drain_events_json()));
                    }
                    // Auto：无适配器时以 CPU Reference 运行（画布保持空白）
                    Err(_) => {}
                }
            }
            let mut st = inner.borrow_mut();
            st.presenter = presenter;
            st.host.complete_init(engine);
            Ok(JsValue::from_str(&st.host.drain_events_json()))
        })
    }

    /// 处理一条非 `ENGINE_INIT` 消息，返回事件 JSON 数组
    pub fn post(&self, message_json: &str) -> String {
        let mut st = self.inner.borrow_mut();
        let st = &mut *st;
        match WorkerHost::parse_message(message_json) {
            Ok(cmd) => {
                if let (EngineCommand::Resize { width, height, .. }, Some(p)) = (&cmd, st.presenter.as_mut()) {
                    p.resize(*width, *height);
                }
                st.host.handle(cmd);
            }
            Err(e) => return crate::host::events_to_json(&[e.to_event()]),
        }
        if st.host.phase() == HostPhase::ShutDown {
            st.presenter = None;
        }
        st.host.drain_events_json()
    }

    /// 帧回调：推进 Tick、泵送 GPU 事件、呈现画面，并在需要时启动异步设备重获
    pub fn frame(&self, now_ms: f64) -> String {
        let need_reacquire = {
            let mut st = self.inner.borrow_mut();
            let st = &mut *st;
            st.host.on_frame(now_ms);
            if let (Some(p), Some(engine)) = (st.presenter.as_mut(), st.host.engine_mut()) {
                p.present(engine);
            }
            let need = !st.reacquiring && st.host.engine().is_some_and(Engine::awaiting_external_reacquire);
            if need {
                st.reacquiring = true;
            }
            need
        };
        if need_reacquire {
            self.spawn_reacquire();
        }
        self.inner.borrow_mut().host.drain_events_json()
    }

    /// 当前宿主阶段（调试用）：`awaiting-init` / `initializing` / `ready` / `shut-down`
    pub fn phase(&self) -> String {
        match self.inner.borrow().host.phase() {
            HostPhase::AwaitingInit => "awaiting-init",
            HostPhase::Initializing => "initializing",
            HostPhase::Ready => "ready",
            HostPhase::ShutDown => "shut-down",
        }
        .to_string()
    }
}

impl EngineWorker {
    /// DeviceLost 第 3 步：浏览器 `requestDevice` 只能异步完成
    fn spawn_reacquire(&self) {
        let inner = Rc::clone(&self.inner);
        wasm_bindgen_futures::spawn_local(async move {
            let owner = inner
                .borrow_mut()
                .host
                .engine_mut()
                .and_then(Engine::take_owner_for_reacquire);
            let (owner, result) = match owner {
                Some(mut o) => {
                    let r = o.reacquire().await;
                    (Some(o), r)
                }
                None => (
                    None,
                    Err(rynvek_wgpu::GpuError::adapter_unavailable("设备所有者不可用")),
                ),
            };
            let mut st = inner.borrow_mut();
            if let Some(engine) = st.host.engine_mut() {
                engine.finish_external_reacquire(owner, result);
            }
            st.reacquiring = false;
        });
    }
}

/// 接入设备并从同一 Instance 为 OffscreenCanvas 创建 Surface
fn attach(
    engine: &mut Engine,
    owner: GpuDeviceOwner,
    canvas: OffscreenCanvas,
    width: u32,
    height: u32,
) -> Result<Option<Presenter>, EngineError> {
    let surface = owner
        .instance()
        .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas.clone()))
        .map_err(|e| {
            EngineError::from(rynvek_wgpu::GpuError::adapter_unavailable(format!(
                "无法创建 Surface：{e}"
            )))
        })?;
    let caps = surface.get_capabilities(owner.adapter());
    let format = pick_surface_format(&caps.formats);
    engine.attach_gpu(owner)?;
    let Some(format) = format else {
        // 无可用呈现格式：仍保留 GPU 计算镜像，仅不渲染
        return Ok(None);
    };
    engine.enable_rendering(format)?;
    let alpha_mode = caps
        .alpha_modes
        .first()
        .copied()
        .unwrap_or(wgpu::CompositeAlphaMode::Auto);
    let mut presenter = Presenter {
        canvas,
        surface,
        config: wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        },
        generation: None,
    };
    presenter.resize(width, height);
    if let Some(owner) = engine.gpu_owner() {
        presenter.configure(owner);
    }
    Ok(Some(presenter))
}
