// crates/rynvek-wgpu/shaders/compute/lif_readout.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// LIF 6 维动作读出：对本 Tick 发放（脉冲环最低位为 1）的隐藏神经元累加输出权重，
// 再 `>> 2` 并钳制到 [-256, 256]，与 CPU 参照逐位一致。每个调用处理一个脑。

struct LifParams {
    brain_count: u32,
    n_hid: u32,
    plasticity_enabled: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: LifParams;
@group(1) @binding(1) var<storage, read> weights_base_q8: array<i32>;
@group(1) @binding(3) var<storage, read> spike_ring_in: array<vec2<u32>>;
@group(1) @binding(4) var<storage, read> weights_delta_in: array<i32>;
@group(3) @binding(0) var<storage, read_write> readout_q8: array<i32>;

@compute @workgroup_size(128, 1, 1)
fn lif_readout(@builtin(global_invocation_id) gid: vec3<u32>) {
    let b = gid.x;
    if (b >= params.brain_count) {
        return;
    }
    let n_hid = params.n_hid;
    let w_base = b * (n_hid * 16u + n_hid * 6u);
    var acc = array<i32, 6>(0, 0, 0, 0, 0, 0);
    for (var h = 0u; h < n_hid; h = h + 1u) {
        if ((spike_ring_in[b * n_hid + h].x & 1u) == 1u) {
            let out_off = w_base + n_hid * 16u + h * 6u;
            for (var k = 0u; k < 6u; k = k + 1u) {
                acc[k] = acc[k] + clamp(weights_base_q8[out_off + k] + weights_delta_in[out_off + k], -32767, 32767);
            }
        }
    }
    for (var k = 0u; k < 6u; k = k + 1u) {
        readout_q8[b * 6u + k] = clamp(acc[k] >> 2u, -256, 256);
    }
}
