// crates/rynvek-wgpu/shaders/compute/field_row_sums.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// 统计摘要：每行内区之和（单行最多 256 × 32767 < 2^31，i32 不溢出），宿主以 i64 汇总，
// 经 3-Slot Readback Ring 异步回读，与 CPU `sum_interior_i64` 做守恒/差分校验。

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
@group(1) @binding(0) var<storage, read> padded: array<i32>;
@group(3) @binding(0) var<storage, read_write> row_sums: array<i32>;

@compute @workgroup_size(128, 1, 1)
fn field_row_sums(@builtin(global_invocation_id) gid: vec3<u32>) {
    let y = gid.x;
    if (y >= params.tile_dim) {
        return;
    }
    let base = (y + params.halo) * params.stride + params.halo;
    var acc = 0;
    for (var x = 0u; x < params.tile_dim; x = x + 1u) {
        acc = acc + padded[base + x];
    }
    row_sums[y] = acc;
}
