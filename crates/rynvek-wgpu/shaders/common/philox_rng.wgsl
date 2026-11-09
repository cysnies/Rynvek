// crates/rynvek-wgpu/shaders/common/philox_rng.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// 规范 02 §2.2：与 rynvek-core `philox4x32_10` 位级对齐的 10 轮 32 位无符号乘法展开（逐字照录，勿改）。

fn mul_hi_lo_u32(a: u32, b: u32) -> vec2<u32> {
    let a_lo = a & 0xFFFFu;
    let a_hi = a >> 16u;
    let b_lo = b & 0xFFFFu;
    let b_hi = b >> 16u;

    let p0 = a_lo * b_lo;
    let p1 = a_lo * b_hi;
    let p2 = a_hi * b_lo;
    let p3 = a_hi * b_hi;

    let mid = (p0 >> 16u) + (p1 & 0xFFFFu) + (p2 & 0xFFFFu);
    let lo = (p0 & 0xFFFFu) | ((mid & 0xFFFFu) << 16u);
    let hi = p3 + (p1 >> 16u) + (p2 >> 16u) + (mid >> 16u);
    return vec2<u32>(lo, hi); // .x = lo, .y = hi
}

fn philox4x32_10(ctr_in: vec4<u32>, key_in: vec2<u32>) -> vec4<u32> {
    var ctr = ctr_in;
    var key = key_in;
    for (var r: u32 = 0u; r < 10u; r = r + 1u) {
        let p0 = mul_hi_lo_u32(0xD2511F53u, ctr.x);
        let p1 = mul_hi_lo_u32(0xCD9E8D57u, ctr.z);
        ctr = vec4<u32>(
            p1.y ^ ctr.y ^ key.x,
            p1.x,
            p0.y ^ ctr.w ^ key.y,
            p0.x
        );
        key.x = key.x + 0x9E3779B9u;
        key.y = key.y + 0xBB67AE85u;
    }
    return ctr;
}
