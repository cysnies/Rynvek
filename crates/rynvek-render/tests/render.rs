// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 真实设备上的世界渲染测试：像素回读、GPU ID 拾取、层顺序与热力图。
//! 无可用适配器时跳过；`RYNVEK_REQUIRE_GPU=1` 时改为失败。

use rynvek_render::{
    Camera2D, FieldOverlay, FrameScene, InstanceData, InstanceShape, OffscreenTarget, PickId, PickReadback,
    RenderLayer, RenderLayerMask, WorldPalette, WorldRenderer,
};
use rynvek_wgpu::kernel::create_storage_buffer;
use rynvek_wgpu::{GpuDeviceOwner, GpuDeviceRequest};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const VIEW: [u32; 2] = [128, 128];

fn gpu() -> Option<GpuDeviceOwner> {
    match GpuDeviceOwner::request_blocking(GpuDeviceRequest::from_env()) {
        Ok(g) => {
            eprintln!(
                "[rynvek-render] 适配器：{} / {:?}",
                g.adapter_info().name,
                g.adapter_info().backend
            );
            Some(g)
        }
        Err(e) => {
            if std::env::var("RYNVEK_REQUIRE_GPU").as_deref() == Ok("1") {
                panic!("RYNVEK_REQUIRE_GPU=1 但无法创建设备：{e}");
            }
            eprintln!("[rynvek-render] 跳过：{e}");
            None
        }
    }
}

struct Frame {
    color: Vec<u8>,
    pick: Vec<u32>,
}

impl Frame {
    fn rgba(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * VIEW[0] + x) * 4) as usize;
        [self.color[i], self.color[i + 1], self.color[i + 2], self.color[i + 3]]
    }
    fn pick(&self, x: u32, y: u32) -> u32 {
        self.pick[(y * VIEW[0] + x) as usize]
    }
}

/// 8×8 世界格、每格 16 像素 → 128×128 视口恰好覆盖 [0, 8)²
fn camera() -> Camera2D {
    Camera2D::new([4.0, 4.0], 16.0, VIEW)
}

fn draw(gpu: &GpuDeviceOwner, scene: &FrameScene, field: Option<FieldOverlay<'_>>) -> Frame {
    let (device, queue) = (gpu.device(), gpu.queue());
    let mut renderer = WorldRenderer::new(device, FORMAT).unwrap();
    let target = OffscreenTarget::new(device, VIEW, FORMAT);
    let mut enc = device.create_command_encoder(&Default::default());
    renderer.render(
        device,
        queue,
        &mut enc,
        &target.color_view,
        &target.pick_view,
        scene,
        &camera(),
        field,
    );
    queue.submit([enc.finish()]);
    let frame = Frame {
        color: target.read_color_blocking(device, queue),
        pick: target.read_pick_blocking(device, queue),
    };
    assert!(gpu.take_uncaptured_errors().is_empty());
    frame
}

fn organism(x: f32, y: f32, dense: u32) -> InstanceData {
    InstanceData::new(
        RenderLayer::Organisms,
        [x, y],
        1.0,
        WorldPalette::faction(0),
        InstanceShape::Circle,
    )
    .with_pick(PickId::from_dense_index(dense).unwrap())
}

#[test]
fn terrain_draws_background_and_grid_lines() {
    let Some(gpu) = gpu() else { return };
    let frame = draw(&gpu, &FrameScene::new(0, RenderLayerMask::ALL), None);
    let p = WorldPalette::NEUTRAL_DARK;
    // 格内部为底色，格边界首像素为网格线
    assert_eq!(frame.rgba(8, 8), p.background.0);
    assert_eq!(frame.rgba(16, 8), p.grid_line.0);
    assert_eq!(frame.pick(8, 8), 0);
}

#[test]
fn instanced_organism_renders_and_is_pickable_by_gpu_id() {
    let Some(gpu) = gpu() else { return };
    let mut scene = FrameScene::new(5, RenderLayerMask::ALL);
    scene.push(organism(4.5, 4.5, 6)); // 屏幕中心 (72, 72)
    scene.push(organism(1.5, 6.5, 41)); // (24, 104)
    let frame = draw(&gpu, &scene, None);
    assert_eq!(frame.rgba(72, 72), WorldPalette::faction(0).0);
    assert_eq!(frame.pick(72, 72), 7, "PickId = dense_index + 1");
    assert_eq!(frame.pick(24, 104), 42);
    // 圆形裁剪：方块角落不属于对象
    assert_eq!(frame.pick(65, 65), 0);
    assert_eq!(frame.pick(2, 2), 0);
}

#[test]
fn fog_layer_darkens_without_erasing_pick_ids_and_overlay_draws_on_top() {
    let Some(gpu) = gpu() else { return };
    let mut scene = FrameScene::new(0, RenderLayerMask::ALL);
    scene.push(organism(4.5, 4.5, 0));
    scene.push(InstanceData::new(
        RenderLayer::FogOfWar,
        [4.0, 4.0],
        8.0,
        WorldPalette::NEUTRAL_DARK.fog,
        InstanceShape::Square,
    ));
    let mut ring = InstanceData::new(
        RenderLayer::Overlay,
        [2.5, 2.5],
        1.0,
        WorldPalette::NEUTRAL_DARK.accent,
        InstanceShape::Square,
    );
    ring = ring.with_pick(PickId(99));
    scene.push(ring);
    let frame = draw(&gpu, &scene, None);
    let orange = WorldPalette::faction(0).0;
    let under_fog = frame.rgba(72, 72);
    assert!(under_fog[0] < orange[0] / 2, "迷雾必须压暗下层：{under_fog:?}");
    assert_eq!(frame.pick(72, 72), 1, "L6 迷雾不可擦除拾取编号");
    assert_eq!(
        frame.rgba(40, 40),
        WorldPalette::NEUTRAL_DARK.accent.0,
        "L7 绘制在迷雾之上"
    );
    assert_eq!(frame.pick(40, 40), 99);
}

#[test]
fn layer_mask_hides_disabled_layers() {
    let Some(gpu) = gpu() else { return };
    let mut scene = FrameScene::new(0, RenderLayerMask::ALL.without(RenderLayer::Organisms));
    scene.push(organism(4.5, 4.5, 0));
    let frame = draw(&gpu, &scene, None);
    assert_eq!(frame.pick(72, 72), 0);
    assert_eq!(frame.rgba(72, 72), WorldPalette::NEUTRAL_DARK.background.0);
}

#[test]
fn heatmap_reads_gpu_resident_field_buffer() {
    let Some(gpu) = gpu() else { return };
    let (tile, halo) = (8u32, 2u32);
    let stride = tile + 2 * halo;
    let mut padded = vec![0i32; (stride * stride) as usize];
    padded[((3 + halo) * stride + 5 + halo) as usize] = 256; // 世界格 (5, 3)
    let buffer = create_storage_buffer(
        gpu.device(),
        "test.field",
        bytemuck::cast_slice(&padded),
        wgpu::BufferUsages::empty(),
    )
    .unwrap();
    let field = FieldOverlay {
        buffer: &buffer,
        tile_dim: tile,
        halo,
        origin: [0.0, 0.0],
        cell_size: 1.0,
        value_max: 256.0,
    };
    let frame = draw(&gpu, &FrameScene::new(0, RenderLayerMask::ALL), Some(field));
    let hot = frame.rgba(5 * 16 + 8, 3 * 16 + 8);
    let cold = frame.rgba(2 * 16 + 8, 6 * 16 + 8);
    assert_eq!(cold, WorldPalette::NEUTRAL_DARK.background.0);
    // 满值 → 接近热端黄色 #F0E442
    assert!(hot[0] > 180 && hot[1] > 160 && hot[2] < 100, "{hot:?}");
    // 关闭 L1 后热力图消失
    let off = draw(
        &gpu,
        &FrameScene::new(0, RenderLayerMask::ALL.without(RenderLayer::FieldHeatmap)),
        Some(field),
    );
    assert_eq!(
        off.rgba(5 * 16 + 8, 3 * 16 + 8),
        WorldPalette::NEUTRAL_DARK.background.0
    );
}

#[test]
fn async_pick_readback_resolves_latest_request() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let mut renderer = WorldRenderer::new(device, FORMAT).unwrap();
    let target = OffscreenTarget::new(device, VIEW, FORMAT);
    let mut picker = PickReadback::new(device);
    let mut scene = FrameScene::new(0, RenderLayerMask::ALL);
    scene.push(organism(4.5, 4.5, 11));
    picker.request(1, 0, 0);
    picker.request(2, 72, 72); // 同帧后到者覆盖
    let mut enc = device.create_command_encoder(&Default::default());
    renderer.render(
        device,
        queue,
        &mut enc,
        &target.color_view,
        &target.pick_view,
        &scene,
        &camera(),
        None,
    );
    assert!(picker.encode(&mut enc, &target.pick));
    queue.submit([enc.finish()]);
    picker.after_submit();
    assert!(picker.is_busy());
    let mut result = None;
    for _ in 0..1000 {
        gpu.poll_nonblocking();
        if let Some(r) = picker.try_take() {
            result = Some(r);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let r = result.expect("拾取结果应在有限时间内送达");
    assert_eq!(r.request_id, 2);
    assert_eq!(r.id, Some(PickId(12)));
    assert!(!picker.is_busy());
}
