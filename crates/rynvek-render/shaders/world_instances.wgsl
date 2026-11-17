// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// L0、L2–L7 实例化图元（GPU Instancing）与 GPU ID 拾取：
//   - 每个实例 6 个顶点（两个三角形组成的方块），形状在片元阶段裁剪（方块 / 圆 / 空心环）；
//   - location(1) 输出到 R32Uint 拾取目标：pick_id = dense_index + 1，0 表示无对象；
//   - 不可拾取层（L6 迷雾）使用写掩码为空的管线，不会擦除下层的拾取编号。

struct CameraUniform {
    center: vec2<f32>,
    viewport: vec2<f32>,
    px_per_cell: f32,
    grid_pixels: f32,
    _pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VsIn {
    @builtin(vertex_index) vi: u32,
    @location(0) position: vec2<f32>,
    @location(1) size: f32,
    @location(2) rotation: f32,
    @location(3) color: u32,
    @location(4) pick_id: u32,
    @location(5) layer_shape: vec2<u32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: u32,
    @location(2) @interpolate(flat) pick_id: u32,
    @location(3) @interpolate(flat) shape: u32,
    @location(4) @interpolate(flat) half_extent_px: vec2<f32>,
}

@vertex
fn vs_instance(in: VsIn) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[in.vi % 6u];
    // 至少 1.5 像素半径，避免远景下图元消失
    var half_px = vec2<f32>(max(in.size * camera.px_per_cell * 0.5, 1.5));
    var angle = in.rotation;
    // 矩形标签复用第二标量为高度；其余形状保持原旋转语义与32字节ABI。
    if (in.layer_shape.y == 3u) {
        half_px.y = max(in.rotation * camera.px_per_cell * 0.5, 1.5);
        angle = 0.0;
    }
    let cs = cos(angle);
    let sn = sin(angle);
    let local = vec2<f32>(c.x * cs - c.y * sn, c.x * sn + c.y * cs) * half_px;
    let screen = (in.position - camera.center) * camera.px_per_cell + camera.viewport * 0.5 + local;
    let ndc = screen / camera.viewport * 2.0 - 1.0;

    var out: VsOut;
    out.clip = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = c;
    out.color = in.color;
    out.pick_id = in.pick_id;
    out.shape = in.layer_shape.y;
    out.half_extent_px = half_px;
    return out;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) pick: u32,
}

@fragment
fn fs_instance(in: VsOut) -> FsOut {
    let r = length(in.uv);
    if (in.shape == 1u && r > 1.0) {
        discard;
    }
    if (in.shape == 2u && (r > 1.0 || r < 0.7)) {
        discard;
    }
    if (in.shape == 3u) {
        let edge = (vec2<f32>(1.0) - abs(in.uv)) * in.half_extent_px;
        if (min(edge.x, edge.y) > 1.5) { discard; }
    }
    var out: FsOut;
    out.color = unpack4x8unorm(in.color);
    out.pick = in.pick_id;
    return out;
}
