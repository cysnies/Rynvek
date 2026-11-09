// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 36 条指令的权威整数语义（CPU Reference）。WGSL 后端逐条生成与此处位级一致的表达式。
//!
//! 值统一以 `i64` 原始值承载：`Bool ∈ {0,1}`、`I32` 为 `i32`、`Q8_8` 为 `i16`、`Q32_32` 为 `i64`。
//!
//! | 类型 | 加/减 | 乘 | 除（除数为 0 → 0） | 取负/绝对值 |
//! | :-- | :-- | :-- | :-- | :-- |
//! | `I32` | 回绕 | 回绕 | 向零截断，`MIN/-1` 回绕 | 回绕 |
//! | `Q8_8` | 饱和到 i16 | `(a*b)>>8` 饱和 | `(a<<8)/b` 饱和 | 饱和 |
//! | `Q32_32` | 饱和到 i64 | `(a*b)>>32`（i128，截断，与 `rynvek-core` 一致） | 不支持 | 饱和 |

use crate::ir::{Opcode, ValueType};

#[inline]
fn sat_i16(v: i64) -> i64 {
    v.clamp(i16::MIN as i64, i16::MAX as i64)
}

#[inline]
fn as_i32(v: i64) -> i32 {
    v as i32
}

/// 二元算术
pub fn binary(op: Opcode, ty: ValueType, a: i64, b: i64) -> i64 {
    use Opcode::*;
    match ty {
        ValueType::I32 => {
            let (a, b) = (as_i32(a), as_i32(b));
            (match op {
                Add => a.wrapping_add(b),
                Sub => a.wrapping_sub(b),
                Mul => a.wrapping_mul(b),
                Div => {
                    if b == 0 {
                        0
                    } else {
                        a.wrapping_div(b)
                    }
                }
                Min => a.min(b),
                Max => a.max(b),
                _ => unreachable_op(op),
            }) as i64
        }
        ValueType::Q8_8 => match op {
            Add => sat_i16(a + b),
            Sub => sat_i16(a - b),
            Mul => sat_i16((a * b) >> 8),
            Div => {
                if b == 0 {
                    0
                } else {
                    // 与 WGSL 相同：i32 内计算并向零截断
                    sat_i16(((a as i32) << 8).wrapping_div(b as i32) as i64)
                }
            }
            Min => a.min(b),
            Max => a.max(b),
            _ => unreachable_op(op),
        },
        ValueType::Q32_32 => match op {
            Add => a.saturating_add(b),
            Sub => a.saturating_sub(b),
            Mul => (((a as i128) * (b as i128)) >> 32) as i64,
            Min => a.min(b),
            Max => a.max(b),
            _ => unreachable_op(op),
        },
        _ => unreachable_op(op),
    }
}

/// 一元算术
pub fn unary(op: Opcode, ty: ValueType, a: i64) -> i64 {
    match (op, ty) {
        (Opcode::Neg, ValueType::I32) => as_i32(a).wrapping_neg() as i64,
        (Opcode::Abs, ValueType::I32) => as_i32(a).wrapping_abs() as i64,
        (Opcode::Neg, ValueType::Q8_8) => sat_i16(-a),
        (Opcode::Abs, ValueType::Q8_8) => sat_i16(a.abs()),
        (Opcode::Neg, ValueType::Q32_32) => a.saturating_neg(),
        (Opcode::Abs, ValueType::Q32_32) => a.saturating_abs(),
        _ => unreachable_op(op),
    }
}

/// `Clamp(x, lo, hi) = min(max(x, lo), hi)`（与 WGSL 内建 `clamp` 对整数的定义一致；lo > hi 时结果为 hi）
pub fn clamp(x: i64, lo: i64, hi: i64) -> i64 {
    x.max(lo).min(hi)
}

/// 比较（操作数已提升为同一类型，原始值有序性即数值有序性）
pub fn compare(op: Opcode, a: i64, b: i64) -> i64 {
    use Opcode::*;
    (match op {
        CmpEq => a == b,
        CmpNe => a != b,
        CmpLt => a < b,
        CmpLe => a <= b,
        CmpGt => a > b,
        CmpGe => a >= b,
        _ => unreachable_op(op),
    }) as i64
}

/// 类型转换
pub fn cast(from: ValueType, to: ValueType, v: i64) -> i64 {
    use ValueType::*;
    match (from, to) {
        (a, b) if a == b => v,
        (Bool, I32) => v,
        (I32, Q8_8) => v.clamp(-128, 127) << 8,
        (I32, Q32_32) => v << 32,
        (Q8_8, I32) => v >> 8,
        (Q8_8, Q32_32) => v << 24,
        (Q32_32, I32) => v >> 32,
        (Q32_32, Q8_8) => sat_i16(v >> 24),
        _ => unreachable!("Pass 1 已拒绝的转换 {from:?} → {to:?}"),
    }
}

/// `RandomQ8_8`：Philox 输出字 0 的高 8 位，即 `[0, 1)` 的 Q8.8 原始值
pub fn random_q8_from_u32(word: u32) -> i64 {
    (word >> 24) as i64
}

/// 场沉积累加（Q8.8 饱和）
pub fn deposit_add(acc: i64, v: i64) -> i64 {
    sat_i16(acc + v)
}

/// 组件原始值规范化（宿主输入可能越界，按类型截断/饱和）
pub fn normalize(ty: ValueType, v: i64) -> i64 {
    match ty {
        ValueType::Bool => (v != 0) as i64,
        ValueType::I32 => v as i32 as i64,
        ValueType::Q8_8 => sat_i16(v),
        _ => v,
    }
}

#[cold]
fn unreachable_op(op: Opcode) -> ! {
    unreachable!("Pass 1 已保证 {op:?} 的操作数类型合法")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q8_arith_saturates() {
        assert_eq!(binary(Opcode::Add, ValueType::Q8_8, 32_000, 1_000), 32_767);
        assert_eq!(binary(Opcode::Mul, ValueType::Q8_8, 512, 384), 768); // 2.0 × 1.5
        assert_eq!(binary(Opcode::Div, ValueType::Q8_8, 256, 0), 0);
        assert_eq!(unary(Opcode::Neg, ValueType::Q8_8, -32_768), 32_767);
    }

    #[test]
    fn i32_arith_wraps() {
        assert_eq!(binary(Opcode::Add, ValueType::I32, i32::MAX as i64, 1), i32::MIN as i64);
        assert_eq!(
            binary(Opcode::Div, ValueType::I32, i32::MIN as i64, -1),
            i32::MIN as i64
        );
        assert_eq!(binary(Opcode::Div, ValueType::I32, -7, 2), -3);
    }

    #[test]
    fn q32_mul_matches_core() {
        use rynvek_core::fixed::Q32_32;
        let (a, b) = (Q32_32::from_int(-3).0 + 12345, Q32_32::from_int(7).0 - 999);
        assert_eq!(binary(Opcode::Mul, ValueType::Q32_32, a, b), Q32_32(a).mul(Q32_32(b)).0);
    }

    #[test]
    fn casts_follow_documented_rules() {
        assert_eq!(cast(ValueType::I32, ValueType::Q8_8, 200), 127 << 8);
        assert_eq!(cast(ValueType::Q8_8, ValueType::I32, -1), -1);
        assert_eq!(cast(ValueType::Q32_32, ValueType::Q8_8, 1i64 << 40), 32_767);
        assert_eq!(cast(ValueType::Q8_8, ValueType::Q32_32, 256), 1i64 << 32);
    }
}
