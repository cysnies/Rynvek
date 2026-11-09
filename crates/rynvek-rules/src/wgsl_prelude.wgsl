// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// ---- Rynvek Rule IR 运行时前导（由 rynvek-rules 生成，勿手改）----

struct RuleParams {
    entity_count: u32,
    tick_lo: u32,
    tick_hi: u32,
    seed_lo: u32,
    seed_hi: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

// Philox4x32-10：与规范 02 §2.2 `philox_rng.wgsl` 逐字一致
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

// Q32.32 以 vec2<u32>(lo, hi) 表示（与 i64 小端字节布局一致）
const Q32_MAX: vec2<u32> = vec2<u32>(0xFFFFFFFFu, 0x7FFFFFFFu);
const Q32_MIN: vec2<u32> = vec2<u32>(0u, 0x80000000u);

fn q32_is_neg(a: vec2<u32>) -> bool {
    return (a.y >> 31u) == 1u;
}

// 饱和加法（对应 i64::saturating_add）
fn q32_add_sat(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let lo = a.x + b.x;
    let carry = select(0u, 1u, lo < a.x);
    let hi = a.y + b.y + carry;
    if ((((a.y ^ hi) & (b.y ^ hi)) >> 31u) == 1u) {
        return select(Q32_MAX, Q32_MIN, q32_is_neg(a));
    }
    return vec2<u32>(lo, hi);
}

// 饱和减法（对应 i64::saturating_sub）
fn q32_sub_sat(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let lo = a.x - b.x;
    let borrow = select(0u, 1u, a.x < b.x);
    let hi = a.y - b.y - borrow;
    if ((((a.y ^ b.y) & (a.y ^ hi)) >> 31u) == 1u) {
        return select(Q32_MAX, Q32_MIN, q32_is_neg(a));
    }
    return vec2<u32>(lo, hi);
}

fn q32_lt(a: vec2<u32>, b: vec2<u32>) -> bool {
    let ah = bitcast<i32>(a.y);
    let bh = bitcast<i32>(b.y);
    return ah < bh || (ah == bh && a.x < b.x);
}

fn q32_min(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return select(b, a, q32_lt(a, b));
}

fn q32_max(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return select(b, a, q32_lt(b, a));
}

fn q32_abs(a: vec2<u32>) -> vec2<u32> {
    return select(a, q32_sub_sat(vec2<u32>(0u, 0u), a), q32_is_neg(a));
}

fn add_with_carry(x: u32, y: u32) -> vec2<u32> {
    let s = x + y;
    return vec2<u32>(s, select(0u, 1u, s < x));
}

// 定点乘法：有符号 64×64→128 位乘积的 [32, 96) 位（对应 rynvek-core `Q32_32::mul`）
fn q32_mul(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let ll = mul_hi_lo_u32(a.x, b.x);
    let lh = mul_hi_lo_u32(a.x, b.y);
    let hl = mul_hi_lo_u32(a.y, b.x);
    let hh = mul_hi_lo_u32(a.y, b.y);
    let s0 = add_with_carry(ll.y, lh.x);
    let s1 = add_with_carry(s0.x, hl.x);
    var hi = lh.y + hl.y + hh.x + s0.y + s1.y;
    // 无符号乘积 → 有符号乘积的补码修正
    if (q32_is_neg(a)) {
        hi = hi - b.x;
    }
    if (q32_is_neg(b)) {
        hi = hi - a.x;
    }
    return vec2<u32>(s1.x, hi);
}

fn q32_from_q8(v: i32) -> vec2<u32> {
    return vec2<u32>(bitcast<u32>(v) << 24u, bitcast<u32>(v >> 8u));
}

fn q32_to_q8(a: vec2<u32>) -> i32 {
    let hi = bitcast<i32>(a.y);
    if (hi >= 128) {
        return 32767;
    }
    if (hi < -128) {
        return -32768;
    }
    return (hi << 8u) | bitcast<i32>(a.x >> 24u);
}
