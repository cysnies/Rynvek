// crates/rynvek-core/src/fixed.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 确定性定点数算术（规范 02 §2.1）。
//!
//! 全部运算仅使用整数指令，保证跨 CPU 架构、跨语言（Rust / TypeScript / WGSL）位级一致。

#![deny(clippy::float_arithmetic)]
// 规范冻结了 `mul` 等 `const fn` 方法名（便于在常量上下文中求值），与 `std::ops` 同名属有意为之。
#![allow(clippy::should_implement_trait)]

/// 64-bit 有符号定点数 Q32.32（整数 32 位，小数 32 位，用于 D0 权威位置、速度、物理与经济比例）
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Q32_32(pub i64);

impl Q32_32 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1i64 << 32);
    pub const NEG_ONE: Self = Self(-(1i64 << 32));
    pub const HALF: Self = Self(1i64 << 31);
    pub const DT_30HZ: Self = Self(143_165_576); // floor(2^32 / 30)

    /// 由 32 位整数构造（左移 32 位，无精度损失）
    #[inline(always)]
    pub const fn from_int(v: i32) -> Self {
        Self((v as i64) << 32)
    }

    /// 由千分之一单位（milli-unit）构造，向零截断
    #[inline(always)]
    pub const fn from_milli(milli: i64) -> Self {
        Self((milli << 32) / 1000)
    }

    /// 向负无穷取整得到整数部分
    #[inline(always)]
    pub const fn to_int_floor(self) -> i32 {
        (self.0 >> 32) as i32
    }

    /// 饱和加法（溢出时钳制到 i64 边界）
    #[inline(always)]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// 饱和减法（溢出时钳制到 i64 边界）
    #[inline(always)]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// 定点乘法：以 i128 计算全精度乘积后算术右移 32 位
    #[inline(always)]
    pub const fn mul(self, rhs: Self) -> Self {
        let prod = (self.0 as i128) * (rhs.0 as i128);
        Self((prod >> 32) as i64)
    }

    /// 安全除法：除数为零时返回 `fallback`，否则以 i128 计算并向零截断
    #[inline(always)]
    pub const fn div_safe(self, rhs: Self, fallback: Self) -> Self {
        if rhs.0 == 0 {
            return fallback;
        }
        let num = (self.0 as i128) << 32;
        Self((num / (rhs.0 as i128)) as i64)
    }

    /// 确定性整数平方根（Newton-Raphson 位级收敛，跨一切 CPU 架构 100% 位级一致，零浮点指令）
    #[inline]
    pub const fn sqrt_deterministic(self) -> Self {
        if self.0 <= 0 {
            return Self::ZERO;
        }
        let val = (self.0 as u128) << 32;
        let mut x = 1u128 << ((128 - val.leading_zeros() + 1) >> 1);
        let mut i = 0;
        while i < 8 {
            x = (x + val / x) >> 1;
            i += 1;
        }
        let res = if x * x > val { x - 1 } else { x };
        Self(res as i64)
    }
}

/// 16-bit 有符号定点数 Q8.8（整数 8 位 [-128, 127]，小数 8 位精度 1/256，用于神经网络膜电位、突触权重与环境场）
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Q8_8(pub i16);

impl Q8_8 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(256);
    pub const NEG_ONE: Self = Self(-256);

    /// 定点乘法：i32 全精度乘积右移 8 位后饱和钳制到 i16 范围
    #[inline(always)]
    pub const fn mul(self, rhs: Self) -> Self {
        let prod = (self.0 as i32) * (rhs.0 as i32);
        Self(const_clamp_i32(prod >> 8, i16::MIN as i32, i16::MAX as i32) as i16)
    }

    /// 硬双曲正切激活：钳制到 [-1.0, +1.0]
    #[inline(always)]
    pub const fn hard_tanh(self) -> Self {
        Self(const_clamp_i16(self.0, -256, 256))
    }
}

/// `i32` 的常量上下文钳制（Rust 1.85 的 `Ord::clamp` 尚不是 `const fn`，语义与其完全一致）
#[inline(always)]
const fn const_clamp_i32(v: i32, min: i32, max: i32) -> i32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// `i16` 的常量上下文钳制（语义与 `Ord::clamp` 完全一致）
#[inline(always)]
const fn const_clamp_i16(v: i16, min: i16, max: i16) -> i16 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 以 Q32.32 原始值表示的 2.5 与 1.5（对应 Golden Vector GV-001）
    const TWO_AND_HALF: Q32_32 = Q32_32(10_737_418_240);
    const ONE_AND_HALF: Q32_32 = Q32_32(6_442_450_944);

    #[test]
    fn q32_constants_are_exact() {
        assert_eq!(Q32_32::ONE, Q32_32::from_int(1));
        assert_eq!(Q32_32::NEG_ONE, Q32_32::from_int(-1));
        assert_eq!(Q32_32::HALF.0 * 2, Q32_32::ONE.0);
        assert_eq!(Q32_32::DT_30HZ.0, (1i64 << 32) / 30);
    }

    #[test]
    fn q32_from_milli_and_floor() {
        assert_eq!(Q32_32::from_milli(1000), Q32_32::ONE);
        assert_eq!(Q32_32::from_milli(500), Q32_32::HALF);
        assert_eq!(Q32_32::from_milli(-1500).to_int_floor(), -2);
        assert_eq!(Q32_32::from_milli(2999).to_int_floor(), 2);
    }

    #[test]
    fn q32_mul_matches_golden_vector() {
        assert_eq!(TWO_AND_HALF.mul(ONE_AND_HALF), Q32_32(16_106_127_360));
        assert_eq!(Q32_32::NEG_ONE.mul(TWO_AND_HALF), Q32_32(-TWO_AND_HALF.0));
    }

    #[test]
    fn q32_div_safe_matches_golden_vector_and_handles_zero() {
        assert_eq!(TWO_AND_HALF.div_safe(ONE_AND_HALF, Q32_32::ZERO), Q32_32(7_158_278_826));
        assert_eq!(Q32_32::ONE.div_safe(Q32_32::ZERO, Q32_32::NEG_ONE), Q32_32::NEG_ONE);
    }

    #[test]
    fn q32_sqrt_is_exact_for_perfect_squares() {
        assert_eq!(Q32_32::from_int(9).sqrt_deterministic(), Q32_32::from_int(3));
        assert_eq!(Q32_32::from_int(1).sqrt_deterministic(), Q32_32::ONE);
        assert_eq!(Q32_32(1i64 << 30).sqrt_deterministic(), Q32_32::HALF); // sqrt(0.25) = 0.5
        assert_eq!(Q32_32::ZERO.sqrt_deterministic(), Q32_32::ZERO);
        assert_eq!(Q32_32::NEG_ONE.sqrt_deterministic(), Q32_32::ZERO);
    }

    #[test]
    fn q32_sqrt_is_floor_of_true_root() {
        // 对一组原始值验证 res^2 <= v * 2^32 < (res + 1)^2
        for raw in [1i64, 2, 3, 12_345, 1 << 20, 987_654_321_012, i64::MAX] {
            let res = Q32_32(raw).sqrt_deterministic().0 as u128;
            let val = (raw as u128) << 32;
            assert!(res * res <= val, "raw = {raw}");
            assert!((res + 1) * (res + 1) > val, "raw = {raw}");
        }
    }

    #[test]
    fn q32_saturating_ops_clamp() {
        assert_eq!(Q32_32(i64::MAX).saturating_add(Q32_32::ONE), Q32_32(i64::MAX));
        assert_eq!(Q32_32(i64::MIN).saturating_sub(Q32_32::ONE), Q32_32(i64::MIN));
    }

    #[test]
    fn q8_mul_saturates_and_hard_tanh_clamps() {
        assert_eq!(Q8_8::ONE.mul(Q8_8::NEG_ONE), Q8_8::NEG_ONE);
        assert_eq!(Q8_8(128).mul(Q8_8(128)), Q8_8(64)); // 0.5 * 0.5 = 0.25
        assert_eq!(Q8_8(i16::MAX).mul(Q8_8(i16::MAX)), Q8_8(i16::MAX));
        assert_eq!(Q8_8(i16::MIN).mul(Q8_8(i16::MAX)), Q8_8(i16::MIN));
        assert_eq!(Q8_8(1000).hard_tanh(), Q8_8::ONE);
        assert_eq!(Q8_8(-1000).hard_tanh(), Q8_8::NEG_ONE);
        assert_eq!(Q8_8(100).hard_tanh(), Q8_8(100));
    }

    #[test]
    fn layouts_are_transparent() {
        assert_eq!(core::mem::size_of::<Q32_32>(), 8);
        assert_eq!(core::mem::size_of::<Q8_8>(), 2);
    }
}
