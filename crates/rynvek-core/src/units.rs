// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 编译期物理量纲（D041、D084）。
//!
//! 以零开销的幽灵类型（phantom type）为 `Q32_32` 附加量纲标签，
//! 使「长度 + 时间」之类的量纲错误在编译期即被拒绝；仅显式声明的量纲组合可以相乘。

use core::marker::PhantomData;
use core::ops::{Add, Mul, Neg, Sub};

use crate::fixed::Q32_32;

/// 量纲标记 trait：每种量纲都是一个零大小的标记类型
pub trait Dimension: Copy + Eq + Ord + core::hash::Hash + core::fmt::Debug + Default {
    /// 量纲的人类可读名称（用于诊断输出）
    const NAME: &'static str;
}

/// 声明一个量纲标记类型
macro_rules! define_dimension {
    ($(#[$meta:meta])* $name:ident, $label:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name;

        impl Dimension for $name {
            const NAME: &'static str = $label;
        }
    };
}

define_dimension!(
    /// 无量纲比例（例如经济比例、缩放系数）
    Dimensionless,
    "1"
);
define_dimension!(
    /// 长度：世界单位（World Unit）
    Length,
    "unit"
);
define_dimension!(
    /// 时间：秒
    Duration,
    "s"
);
define_dimension!(
    /// 速度：世界单位 / 秒
    Velocity,
    "unit/s"
);
define_dimension!(
    /// 加速度：世界单位 / 秒²
    Acceleration,
    "unit/s^2"
);

/// 带量纲的 Q32.32 定点物理量（内存布局与 `Q32_32` 完全相同）
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity<D: Dimension> {
    value: Q32_32,
    _dimension: PhantomData<D>,
}

/// 长度量
pub type LengthQ = Quantity<Length>;
/// 时间量
pub type DurationQ = Quantity<Duration>;
/// 速度量
pub type VelocityQ = Quantity<Velocity>;
/// 加速度量
pub type AccelerationQ = Quantity<Acceleration>;
/// 无量纲比例
pub type RatioQ = Quantity<Dimensionless>;

impl<D: Dimension> Quantity<D> {
    /// 零值
    pub const ZERO: Self = Self::new(Q32_32::ZERO);

    /// 以定点数值构造物理量
    #[inline]
    pub const fn new(value: Q32_32) -> Self {
        Self {
            value,
            _dimension: PhantomData,
        }
    }

    /// 取出底层定点数值（丢弃量纲标签）
    #[inline]
    pub const fn value(self) -> Q32_32 {
        self.value
    }

    /// 乘以无量纲系数，量纲保持不变
    #[inline]
    pub const fn scale(self, factor: Q32_32) -> Self {
        Self::new(self.value.mul(factor))
    }
}

impl<D: Dimension> Add for Quantity<D> {
    type Output = Self;

    /// 同量纲饱和加法
    fn add(self, rhs: Self) -> Self {
        Self::new(self.value.saturating_add(rhs.value))
    }
}

impl<D: Dimension> Sub for Quantity<D> {
    type Output = Self;

    /// 同量纲饱和减法
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.value.saturating_sub(rhs.value))
    }
}

impl<D: Dimension> Neg for Quantity<D> {
    type Output = Self;

    /// 取负（对 `i64::MIN` 饱和）
    fn neg(self) -> Self {
        Self::new(Q32_32::ZERO.saturating_sub(self.value))
    }
}

/// 声明「量纲 A × 量纲 B = 量纲 C」的合法乘法组合（双向交换律）
macro_rules! impl_dimension_product {
    ($lhs:ty, $rhs:ty => $out:ty) => {
        impl Mul<Quantity<$rhs>> for Quantity<$lhs> {
            type Output = Quantity<$out>;

            fn mul(self, rhs: Quantity<$rhs>) -> Quantity<$out> {
                Quantity::new(self.value.mul(rhs.value))
            }
        }

        impl Mul<Quantity<$lhs>> for Quantity<$rhs> {
            type Output = Quantity<$out>;

            fn mul(self, rhs: Quantity<$lhs>) -> Quantity<$out> {
                Quantity::new(self.value.mul(rhs.value))
            }
        }
    };
}

impl_dimension_product!(Velocity, Duration => Length);
impl_dimension_product!(Acceleration, Duration => Velocity);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_times_duration_is_length() {
        let v = VelocityQ::new(Q32_32::from_int(6));
        let dt = DurationQ::new(Q32_32::HALF);
        let dx: LengthQ = v * dt;
        assert_eq!(dx.value(), Q32_32::from_int(3));
        let dx2: LengthQ = dt * v;
        assert_eq!(dx, dx2);
    }

    #[test]
    fn acceleration_times_duration_is_velocity() {
        let a = AccelerationQ::new(Q32_32::from_int(-4));
        let dt = DurationQ::new(Q32_32::from_int(2));
        let dv: VelocityQ = a * dt;
        assert_eq!(dv.value(), Q32_32::from_int(-8));
    }

    #[test]
    fn same_dimension_arithmetic_and_scaling() {
        let a = LengthQ::new(Q32_32::from_int(5));
        let b = LengthQ::new(Q32_32::from_int(2));
        assert_eq!((a + b).value(), Q32_32::from_int(7));
        assert_eq!((a - b).value(), Q32_32::from_int(3));
        assert_eq!((-a).value(), Q32_32::from_int(-5));
        assert_eq!(a.scale(Q32_32::HALF), LengthQ::new(Q32_32::from_milli(2500)));
        assert_eq!(LengthQ::ZERO.value(), Q32_32::ZERO);
    }

    #[test]
    fn quantities_are_zero_cost() {
        assert_eq!(core::mem::size_of::<LengthQ>(), core::mem::size_of::<Q32_32>());
        assert_eq!(Velocity::NAME, "unit/s");
    }
}
