// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-render
//!
//! W7 表现层的 WebGPU 世界渲染器（项目文档第 4.1 / 6.1 节，规范 07）。
//!
//! - [`layer`]：L0–L7 八个渲染层与绘制顺序（ERRATA E-015 冻结各层语义）；
//! - [`lod`]：随摄像机缩放切换的 L0–L4 神经多尺度可视化级别；
//! - [`camera`]：二维正交世界摄像机（32×32 像素网格）；
//! - [`palette`]：`--rv-*` 世界画布色板与 Okabe-Ito 色盲安全阵营色；
//! - [`instance`]：GPU Instancing 的 32 字节实例记录与 [`PickId`]；
//! - [`scene`]：[`VisualizationProvider`] 只读快照 → [`FrameScene`] 的解耦契约；
//! - [`renderer`]：[`WorldRenderer`]（地形 + 场热力图 + 分层实例化绘制 + R32Uint ID 拾取目标）；
//! - [`picking`]：非阻塞 GPU ID 拾取回读。
//!
//! **只读铁律**：渲染层只消费 `Phase 15` 发布的只读快照（`&S`），类型签名上不存在修改世界权威状态的途径；
//! 所有 GPU 资源均经由 `rynvek-wgpu` 的独占设备创建。

#![deny(unsafe_code)]

pub mod camera;
pub mod instance;
pub mod layer;
pub mod lod;
pub mod palette;
pub mod picking;
pub mod renderer;
pub mod scene;

pub use camera::Camera2D;
pub use instance::{InstanceData, InstanceShape, PickId};
pub use layer::{RenderLayer, RenderLayerMask};
pub use lod::NeuralLod;
pub use palette::{OKABE_ITO, Rgba8, WorldPalette};
pub use picking::{PickReadback, PickResult};
pub use renderer::{FieldOverlay, OffscreenTarget, WorldRenderer};
pub use scene::{FrameScene, VisualizationProvider, collect_scene};
