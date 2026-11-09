// crates/rynvek-wgpu/shaders/compute/field_diffusion.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// 规范 06 §6.1 五点拉普拉斯 Q8.8 反应-扩散算子的 GPU 版本，与 rynvek-field
// `solve_reaction_diffusion_tile_q8` 逐位一致：
//   - Q8.8 原始值以 i32 承载（WGSL 无 i16；写回前钳制到 [min, max] ⊂ i16）；
//   - 所有中间量为 i32，`>>` 对有符号整数为算术右移，与 Rust 一致；
//   - 二维网格 Workgroup 8×8×1（规范 01 §1.1）。
// 每个 CFL 子步由宿主依次调度 `field_fill_halo`（Neumann 零梯度拷边）与 `field_diffuse`。

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
@group(1) @binding(0) var<storage, read> src_padded: array<i32>;
@group(1) @binding(1) var<storage, read> sources_q8: array<i32>;
@group(2) @binding(0) var<storage, read_write> dst_padded: array<i32>;

@compute @workgroup_size(8, 8, 1)
fn field_diffuse(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if (x >= params.tile_dim || y >= params.tile_dim) {
        return;
    }
    let s = params.stride;
    let px = x + params.halo;
    let py = y + params.halo;
    let idx = py * s + px;

    let center = rv_checked_q8(src_padded[idx]);
    let north = rv_checked_q8(src_padded[(py - 1u) * s + px]);
    let south = rv_checked_q8(src_padded[(py + 1u) * s + px]);
    let west = rv_checked_q8(src_padded[py * s + (px - 1u)]);
    let east = rv_checked_q8(src_padded[py * s + (px + 1u)]);

    let retain_q8 = 256 - params.decay_q8;
    let laplacian = north + south + west + east - 4 * center;
    let decayed = (center * retain_q8) >> 8u;
    let diffused = (laplacian * params.diffusion_q8) >> 8u;
    var src_term = 0;
    if (params.apply_sources != 0u) {
        src_term = rv_checked_q8(sources_q8[y * params.tile_dim + x]);
    }

    dst_padded[idx] = clamp(decayed + diffused + src_term, params.min_val_q8, params.max_val_q8);
}
