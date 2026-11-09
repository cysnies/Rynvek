// crates/rynvek-render/shaders/world_fullscreen.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// L0 地形（底色 + 32×32 网格线）与 L1 场热力图的全屏绘制。
// 热力图直接读取 rynvek-wgpu 中 GPU 驻留的含 Halo 场缓冲（i32 承载的 Q8.8），零 CPU 回读。
// BindGroup 约定：@group(0) Frame（摄像机与叠加参数 Uniform）；@group(1) StateIn（只读场缓冲）。

struct CameraUniform {
    center: vec2<f32>,
    viewport: vec2<f32>,
    px_per_cell: f32,
    grid_pixels: f32,
    _pad: vec2<f32>,
}

struct OverlayParams {
    tile_dim: u32,
    halo: u32,
    stride: u32,
    flags: u32, // bit0 地形；bit1 网格线；bit2 热力图
    origin: vec2<f32>,
    cell_size: f32,
    value_max: f32,
    background: u32,
    grid_line: u32,
    heat_low: u32,
    heat_high: u32,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<uniform> overlay: OverlayParams;
@group(1) @binding(0) var<storage, read> field_padded: array<i32>;

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) pick: u32,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    // 覆盖整个视口的单个三角形
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_world(@builtin(position) frag: vec4<f32>) -> FsOut {
    let world = (frag.xy - camera.viewport * 0.5) / camera.px_per_cell + camera.center;
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);

    if ((overlay.flags & 1u) != 0u) {
        color = unpack4x8unorm(overlay.background);
        // 缩放足够大时绘制 1px 网格线
        if ((overlay.flags & 2u) != 0u && camera.px_per_cell >= 8.0) {
            let f = fract(world) * camera.px_per_cell;
            if (f.x < 1.0 || f.y < 1.0) {
                color = unpack4x8unorm(overlay.grid_line);
            }
        }
    }

    if ((overlay.flags & 4u) != 0u) {
        let local = (world - overlay.origin) / overlay.cell_size;
        let dim = f32(overlay.tile_dim);
        if (local.x >= 0.0 && local.y >= 0.0 && local.x < dim && local.y < dim) {
            let cx = u32(local.x);
            let cy = u32(local.y);
            let raw = field_padded[(cy + overlay.halo) * overlay.stride + cx + overlay.halo];
            let v = clamp(f32(raw) / overlay.value_max, 0.0, 1.0);
            if (v > 0.0) {
                let heat = mix(unpack4x8unorm(overlay.heat_low), unpack4x8unorm(overlay.heat_high), v);
                let a = 0.25 + 0.6 * v;
                color = vec4<f32>(mix(color.rgb, heat.rgb, a), max(color.a, a));
            }
        }
    }

    var out: FsOut;
    out.color = color;
    out.pick = 0u;
    return out;
}
