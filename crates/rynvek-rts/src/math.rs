// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! RTS 宏观层的确定性定点数学工具（D0：全部基于 `Q32_32` 与整数运算，零浮点）。
//!
//! - [`Vec2`]：以 Tile 为长度单位的二维定点向量（1 Tile = 1 Unit）；
//! - [`per_tick_share`]：把「每秒 N 毫单位」的速率无损拆分到 30 个 Tick；
//! - 速度、距离与角度常量的定点换算。

use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use rynvek_core::Q32_32;
use rynvek_core::constants::BASE_TICK_HZ;

/// 1 个资源 / 生命值单位对应的毫单位（`i64 milli-unit`，项目文档 §5.2.1）
pub const MILLI: i64 = 1000;

/// 由毫单位构造 `Q32_32`（向零截断）
#[inline]
pub const fn q_milli(milli: i64) -> Q32_32 {
    Q32_32::from_milli(milli)
}

/// 「Units/s」速度（毫单位）换算为「Units/Tick」定点速度：`v = speed_milli / (1000 × 30)`
#[inline]
pub const fn speed_per_tick(speed_milli_per_s: i64) -> Q32_32 {
    Q32_32((speed_milli_per_s << 32) / (MILLI * BASE_TICK_HZ as i64))
}

/// 把每秒 `rate_per_s` 的离散量均匀拆分到每个 Tick，返回第 `tick` 个 Tick 的份额。
///
/// 采用累计取整差分：`floor((t+1)·r/30) − floor(t·r/30)`。任意连续 30 个 Tick 的份额之和
/// 精确等于 `rate_per_s`，没有累计舍入漂移，且只依赖 Tick 序号（确定性）。负速率按绝对值拆分后取负。
#[inline]
pub const fn per_tick_share(rate_per_s: i64, tick: u64) -> i64 {
    fractional_share(rate_per_s, BASE_TICK_HZ as i64, tick)
}

/// 通用的有理速率拆分：每 `denom` 个步长共 `numer` 个离散量，返回第 `step` 步的份额
#[inline]
pub const fn fractional_share(numer: i64, denom: i64, step: u64) -> i64 {
    let neg = numer < 0;
    let n = numer.unsigned_abs() as u128;
    let d = denom as u128;
    let s = step as u128;
    let share = ((s + 1) * n / d - s * n / d) as i64;
    if neg { -share } else { share }
}

/// 以 i128 计算 `value × numer / denom` 并向零截断（用于百分比加成，避免溢出）
#[inline]
pub const fn mul_div(value: i64, numer: i64, denom: i64) -> i64 {
    ((value as i128 * numer as i128) / denom as i128) as i64
}

/// 二维定点向量（单位：Tile / Unit）
///
/// serde 形态为 Q32.32 原始值二元组 `[x_raw, y_raw]`（`|raw| < 2^53`，在 JavaScript `number` 中无损）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Vec2 {
    pub x: Q32_32,
    pub y: Q32_32,
}

impl Vec2 {
    pub const ZERO: Self = Self::new(Q32_32::ZERO, Q32_32::ZERO);

    #[inline]
    pub const fn new(x: Q32_32, y: Q32_32) -> Self {
        Self { x, y }
    }

    /// 由整数 Tile 坐标构造
    #[inline]
    pub const fn from_int(x: i32, y: i32) -> Self {
        Self::new(Q32_32::from_int(x), Q32_32::from_int(y))
    }

    /// 由毫单位坐标构造
    #[inline]
    pub const fn from_milli(x: i64, y: i64) -> Self {
        Self::new(Q32_32::from_milli(x), Q32_32::from_milli(y))
    }

    /// Tile `(x, y)` 的中心点 `(x + 0.5, y + 0.5)`
    #[inline]
    pub const fn tile_center(x: u32, y: u32) -> Self {
        Self::new(
            Q32_32(((x as i64) << 32) + Q32_32::HALF.0),
            Q32_32(((y as i64) << 32) + Q32_32::HALF.0),
        )
    }

    #[inline]
    pub const fn scale(self, k: Q32_32) -> Self {
        Self::new(self.x.mul(k), self.y.mul(k))
    }

    #[inline]
    pub const fn dot(self, o: Self) -> Q32_32 {
        Q32_32(self.x.mul(o.x).0.saturating_add(self.y.mul(o.y).0))
    }

    /// 二维叉积（行列式）`self.x·o.y − self.y·o.x`
    #[inline]
    pub const fn det(self, o: Self) -> Q32_32 {
        Q32_32(self.x.mul(o.y).0.saturating_sub(self.y.mul(o.x).0))
    }

    #[inline]
    pub const fn length_sq(self) -> Q32_32 {
        self.dot(self)
    }

    #[inline]
    pub const fn length(self) -> Q32_32 {
        self.length_sq().sqrt_deterministic()
    }

    /// 单位化；零向量返回零向量
    #[inline]
    pub const fn normalized(self) -> Self {
        let len = self.length();
        if len.0 == 0 {
            return Self::ZERO;
        }
        Self::new(self.x.div_safe(len, Q32_32::ZERO), self.y.div_safe(len, Q32_32::ZERO))
    }

    /// 若长度超过 `max` 则等比缩短到 `max`
    #[inline]
    pub const fn clamp_length(self, max: Q32_32) -> Self {
        let len_sq = self.length_sq();
        if len_sq.0 <= max.mul(max).0 {
            return self;
        }
        self.normalized().scale(max)
    }

    /// 两点距离平方
    #[inline]
    pub const fn distance_sq(self, o: Self) -> Q32_32 {
        Self::new(self.x.saturating_sub(o.x), self.y.saturating_sub(o.y)).length_sq()
    }

    /// 逆时针旋转 90°：`(x, y) → (−y, x)`
    #[inline]
    pub const fn perp(self) -> Self {
        Self::new(Q32_32(-self.y.0), self.x)
    }

    /// 所在 Tile（向负无穷取整）；负坐标返回 `None`
    #[inline]
    pub const fn tile(self) -> Option<(u32, u32)> {
        if self.x.0 < 0 || self.y.0 < 0 {
            return None;
        }
        Some(((self.x.0 >> 32) as u32, (self.y.0 >> 32) as u32))
    }
}

impl serde::Serialize for Vec2 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [self.x.0, self.y.0].serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for Vec2 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let [x, y] = <[i64; 2]>::deserialize(d)?;
        Ok(Self::new(Q32_32(x), Q32_32(y)))
    }
}

impl Add for Vec2 {
    type Output = Self;
    #[inline]
    fn add(self, o: Self) -> Self {
        Self::new(self.x.saturating_add(o.x), self.y.saturating_add(o.y))
    }
}

impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl Sub for Vec2 {
    type Output = Self;
    #[inline]
    fn sub(self, o: Self) -> Self {
        Self::new(self.x.saturating_sub(o.x), self.y.saturating_sub(o.y))
    }
}

impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

impl Neg for Vec2 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self::new(Q32_32(-self.x.0), Q32_32(-self.y.0))
    }
}

/// `cos 30° = √3 / 2` 的 Q32.32 表示（round(0.8660254037844386 × 2^32)）
pub const COS_30: Q32_32 = Q32_32(3_719_550_787);
/// `sin 30° = 1/2`
pub const SIN_30: Q32_32 = Q32_32::HALF;
/// `cos 45° = √2 / 2` 的 Q32.32 表示（round(0.7071067811865476 × 2^32)），用于 90° 扇形 AOE 判定
pub const COS_45: Q32_32 = Q32_32(3_037_000_500);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_tick_share_sums_exactly_per_second() {
        for rate in [30_000i64, 25_000, 12_000, 10_000, 40_000, 1_500, -6_000, 69_000, 103_500] {
            for second in 0..3u64 {
                let sum: i64 = (second * 30..(second + 1) * 30).map(|t| per_tick_share(rate, t)).sum();
                assert_eq!(sum, rate, "rate {rate} 第 {second} 秒");
            }
        }
    }

    #[test]
    fn speed_conversion_matches_units_per_second() {
        // 4.5 Units/s × 30 Tick 应回到 4.5（误差小于 30 个最低位）
        let v = speed_per_tick(4_500);
        let back = v.0 * 30;
        assert!((back - Q32_32::from_milli(4_500).0).abs() < 30);
    }

    #[test]
    fn trig_constants_are_correctly_rounded() {
        let c30 = COS_30.mul(COS_30).0 + SIN_30.mul(SIN_30).0;
        assert!((c30 - Q32_32::ONE.0).abs() < 4);
        let c45 = COS_45.mul(COS_45).0 * 2;
        assert!((c45 - Q32_32::ONE.0).abs() < 4);
    }

    #[test]
    fn vector_ops_are_exact_on_integers() {
        let a = Vec2::from_int(3, 4);
        assert_eq!(a.length(), Q32_32::from_int(5));
        assert_eq!(a.perp(), Vec2::from_int(-4, 3));
        assert_eq!(a.det(Vec2::from_int(1, 0)), Q32_32::from_int(-4));
        assert_eq!(Vec2::tile_center(2, 3).tile(), Some((2, 3)));
        assert_eq!(Vec2::from_int(10, 0).clamp_length(Q32_32::from_int(2)), Vec2::from_int(2, 0));
    }
}
