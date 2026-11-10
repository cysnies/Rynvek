// crates/rynvek-core/src/fixed.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 确定性定点数算术（规范 02 §2.1 & Audit v1.1 §5 边界修复）。
//!
//! 全部运算仅使用整数指令，保证跨 CPU 架构、跨语言（Rust / TypeScript / WGSL）位级一致：
//! - `from_milli` 先提升至 `i128` 左移 32 位再除以 1000，消除 `|milli| >= 2^31` 时的提前移位溢出；
//! - 提供 `checked_*`（超出范围返回 [`RynvekErrorCode::FixedPointOverflow`]）与 `saturating_*`（钳制至 `i64` 极值）双族接口；
//! - `mul` 与 `div_safe` 统一在 `i128` 中间结果上执行饱和钳制，杜绝高位截断导致的符号翻转。

#![deny(clippy::float_arithmetic)]
// 规范冻结了 `mul` 等 `const fn` 方法名（便于在常量上下文中求值），与 `std::ops` 同名属有意为之。
#![allow(clippy::should_implement_trait)]

use crate::error::RynvekErrorCode;

/// 64-bit 有符号定点数 Q32.32（整数 32 位，小数 32 位，用于 D0 权威位置、速度、物理与经济比例）
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Q32_32(pub i64);

impl Q32_32 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1i64 << 32);
    pub const NEG_ONE: Self = Self(-(1i64 << 32));
    pub const HALF: Self = Self(1i64 << 31);
    pub const MIN: Self = Self(i64::MIN);
    pub const MAX: Self = Self(i64::MAX);
    pub const DT_30HZ: Self = Self(143_165_576); // floor(2^32 / 30)

    /// 由 32 位整数构造（左移 32 位，无精度损失）
    #[inline(always)]
    pub const fn from_int(v: i32) -> Self {
        Self((v as i64) << 32)
    }

    /// 由千分之一单位（milli-unit）受检构造，向零截断；超出 Q32.32 可表示范围时返回 [`RynvekErrorCode::FixedPointOverflow`]
    #[inline(always)]
    pub const fn checked_from_milli(milli: i64) -> Result<Self, RynvekErrorCode> {
        let scaled = ((milli as i128) << 32) / 1000;
        if scaled < (i64::MIN as i128) || scaled > (i64::MAX as i128) {
            Err(RynvekErrorCode::FixedPointOverflow)
        } else {
            Ok(Self(scaled as i64))
        }
    }

    /// 由千分之一单位（milli-unit）饱和构造，向零截断；超出 Q32.32 范围时钳制到 `i64::MIN` / `i64::MAX`
    #[inline(always)]
    pub const fn saturating_from_milli(milli: i64) -> Self {
        let scaled = ((milli as i128) << 32) / 1000;
        Self(const_clamp_i128_to_i64(scaled))
    }

    /// 由千分之一单位（milli-unit）构造（默认等价于 [`Self::saturating_from_milli`]，在 `i128` 域先乘后除防止移位溢出）
    #[inline(always)]
    pub const fn from_milli(milli: i64) -> Self {
        Self::saturating_from_milli(milli)
    }

    /// 向负无穷取整得到整数部分
    #[inline(always)]
    pub const fn to_int_floor(self) -> i32 {
        (self.0 >> 32) as i32
    }

    /// 受检加法：溢出时返回 [`RynvekErrorCode::FixedPointOverflow`]
    #[inline(always)]
    pub const fn checked_add(self, rhs: Self) -> Result<Self, RynvekErrorCode> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Ok(Self(v)),
            None => Err(RynvekErrorCode::FixedPointOverflow),
        }
    }

    /// 受检减法：溢出时返回 [`RynvekErrorCode::FixedPointOverflow`]
    #[inline(always)]
    pub const fn checked_sub(self, rhs: Self) -> Result<Self, RynvekErrorCode> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Ok(Self(v)),
            None => Err(RynvekErrorCode::FixedPointOverflow),
        }
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

    /// 受检定点乘法：以 i128 计算全精度乘积后算术右移 32 位；超出 i64 范围时返回 [`RynvekErrorCode::FixedPointOverflow`]
    #[inline(always)]
    pub const fn checked_mul(self, rhs: Self) -> Result<Self, RynvekErrorCode> {
        let prod = ((self.0 as i128) * (rhs.0 as i128)) >> 32;
        if prod < (i64::MIN as i128) || prod > (i64::MAX as i128) {
            Err(RynvekErrorCode::FixedPointOverflow)
        } else {
            Ok(Self(prod as i64))
        }
    }

    /// 饱和定点乘法：以 i128 计算全精度乘积后算术右移 32 位，超出范围时饱和钳制至 `i64::MIN` / `i64::MAX`
    #[inline(always)]
    pub const fn saturating_mul(self, rhs: Self) -> Self {
        let prod = ((self.0 as i128) * (rhs.0 as i128)) >> 32;
        Self(const_clamp_i128_to_i64(prod))
    }

    /// 定点乘法（默认采用饱和语义，避免高位截断产生错误符号）
    #[inline(always)]
    pub const fn mul(self, rhs: Self) -> Self {
        Self::saturating_mul(self, rhs)
    }

    /// 受检定点除法：除数为零或商超出 i64 范围时返回 [`RynvekErrorCode::FixedPointOverflow`]，否则向零截断
    #[inline(always)]
    pub const fn checked_div(self, rhs: Self) -> Result<Self, RynvekErrorCode> {
        if rhs.0 == 0 {
            return Err(RynvekErrorCode::FixedPointOverflow);
        }
        let num = (self.0 as i128) << 32;
        let quot = num / (rhs.0 as i128);
        if quot < (i64::MIN as i128) || quot > (i64::MAX as i128) {
            Err(RynvekErrorCode::FixedPointOverflow)
        } else {
            Ok(Self(quot as i64))
        }
    }

    /// 饱和定点除法：除数为零时返回 `fallback`，商超出 i64 范围时饱和钳制到 `i64::MIN` / `i64::MAX`
    #[inline(always)]
    pub const fn saturating_div(self, rhs: Self, fallback: Self) -> Self {
        if rhs.0 == 0 {
            return fallback;
        }
        let num = (self.0 as i128) << 32;
        let quot = num / (rhs.0 as i128);
        Self(const_clamp_i128_to_i64(quot))
    }

    /// 安全除法：除数为零时返回 `fallback`，否则以 i128 计算、向零截断并饱和钳制到 i64 边界
    #[inline(always)]
    pub const fn div_safe(self, rhs: Self, fallback: Self) -> Self {
        Self::saturating_div(self, rhs, fallback)
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

/// 将 `i128` 饱和钳制到 `i64` 可表示范围
#[inline(always)]
const fn const_clamp_i128_to_i64(v: i128) -> i64 {
    if v < (i64::MIN as i128) {
        i64::MIN
    } else if v > (i64::MAX as i128) {
        i64::MAX
    } else {
        v as i64
    }
}

/// `i32` 的常量上下文钳制
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

/// `i16` 的常量上下文钳制
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
    fn q32_from_milli_handles_large_values_without_shift_overflow() {
        assert_eq!(Q32_32::from_milli(1000), Q32_32::ONE);
        assert_eq!(Q32_32::from_milli(500), Q32_32::HALF);
        assert_eq!(Q32_32::from_milli(-1500).to_int_floor(), -2);
        assert_eq!(Q32_32::from_milli(2999).to_int_floor(), 2);
        // Audit §5.1：milli = 2,147,483,648 在旧实现中左移 32 位会溢出为负数，修复后精确为 9,223,372,036,854,775
        assert_eq!(Q32_32::from_milli(2_147_483_648), Q32_32(9_223_372_036_854_775));
        assert_eq!(Q32_32::from_milli(-2_147_483_648), Q32_32(-9_223_372_036_854_775));
        assert_eq!(
            Q32_32::checked_from_milli(3_000_000_000_000),
            Err(RynvekErrorCode::FixedPointOverflow)
        );
        assert_eq!(Q32_32::saturating_from_milli(3_000_000_000_000), Q32_32::MAX);
        assert_eq!(Q32_32::saturating_from_milli(-3_000_000_000_000), Q32_32::MIN);
    }

    #[test]
    fn q32_checked_and_saturating_mul_div_handle_boundaries() {
        assert_eq!(TWO_AND_HALF.mul(ONE_AND_HALF), Q32_32(16_106_127_360));
        assert_eq!(Q32_32::NEG_ONE.mul(TWO_AND_HALF), Q32_32(-TWO_AND_HALF.0));
        let big = Q32_32::from_int(65_536);
        assert_eq!(big.checked_mul(big), Err(RynvekErrorCode::FixedPointOverflow));
        assert_eq!(big.saturating_mul(big), Q32_32::MAX);
        assert_eq!(big.saturating_mul(Q32_32::from_int(-65_536)), Q32_32::MIN);

        let tiny = Q32_32(1);
        assert_eq!(big.checked_div(tiny), Err(RynvekErrorCode::FixedPointOverflow));
        assert_eq!(big.div_safe(tiny, Q32_32::ZERO), Q32_32::MAX);
        assert_eq!(Q32_32::ONE.checked_div(Q32_32::ZERO), Err(RynvekErrorCode::FixedPointOverflow));
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
        assert_eq!(Q32_32(i64::MAX).checked_add(Q32_32::ONE), Err(RynvekErrorCode::FixedPointOverflow));
        assert_eq!(Q32_32(i64::MIN).checked_sub(Q32_32::ONE), Err(RynvekErrorCode::FixedPointOverflow));
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
