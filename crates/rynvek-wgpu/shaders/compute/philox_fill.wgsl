// crates/rynvek-wgpu/shaders/compute/philox_fill.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// 批量计算 Philox4x32-10：out[i] = philox4x32_10(counters[i], key)（CPU/GPU 差分测试与随机流预生成）。

struct PhiloxParams {
    count: u32,
    key_lo: u32,
    key_hi: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: PhiloxParams;
@group(1) @binding(0) var<storage, read> counters: array<vec4<u32>>;
@group(2) @binding(0) var<storage, read_write> outputs: array<vec4<u32>>;

@compute @workgroup_size(128, 1, 1)
fn philox_fill(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.count) {
        return;
    }
    outputs[i] = philox4x32_10(counters[i], vec2<u32>(params.key_lo, params.key_hi));
}
