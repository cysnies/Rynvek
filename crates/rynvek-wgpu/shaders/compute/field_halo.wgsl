// crates/rynvek-wgpu/shaders/compute/field_halo.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// Neumann 零梯度拷边：Halo 格 (x, y) ← 内区 (clamp(x), clamp(y))。
// 与 rynvek-field `fill_neumann_halo`（先左右、后整行上下含角）结果完全相同：
// 角点最终取内区角点值，等价于两个坐标同时钳制。只写 Halo、只读内区，无数据竞争。

struct FieldParams {
    tile_dim: u32,
    halo: u32,
    stride: u32,
    apply_sources: u32,
    diffusion_q8: i32,
    decay_q8: i32,
    min_val_q8: i32,
    max_val_q8: i32,
}

@group(0) @binding(0) var<uniform> params: FieldParams;
@group(2) @binding(0) var<storage, read_write> padded: array<i32>;

@compute @workgroup_size(8, 8, 1)
fn field_fill_halo(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    let s = params.stride;
    if (x >= s || y >= s) {
        return;
    }
    let lo = params.halo;
    let hi = params.halo + params.tile_dim - 1u;
    let inside = x >= lo && x <= hi && y >= lo && y <= hi;
    if (inside) {
        return;
    }
    let sx = clamp(x, lo, hi);
    let sy = clamp(y, lo, hi);
    padded[y * s + x] = padded[sy * s + sx];
}
