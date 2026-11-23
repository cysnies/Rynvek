// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规范 Tier2 定点物理核；不依赖 ECS、世界权限或调度器。
//! 圆形、无摩擦弹性、容量与速度范围是明确实现剖面，不是原规范冻结值。
#![deny(clippy::float_arithmetic)]
use rynvek_core::{RynvekErrorCode, fixed::Q32_32};
use serde::{Deserialize, Serialize};
pub const ONE: i64 = Q32_32::ONE.0;
const SIDE: i64 = 256 * ONE;
const ERROR: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub position_q32: [i64; 2],
    pub velocity_q32: [i64; 2],
    pub acceleration_q32: [i64; 2],
    pub radius_q32: i64,
    /// 质量不是 CPU 配额，也不表示生物代谢资源。
    pub mass_q32: i64,
    pub fixed: bool,
}
impl Body {
    fn validate_components(&self) -> Result<(), RynvekErrorCode> {
        if !(ONE / 4..=2 * ONE).contains(&self.radius_q32)
            || !(ONE..=1024 * ONE).contains(&self.mass_q32)
            || self.velocity_q32.iter().any(|v| !(-8 * ONE..=8 * ONE).contains(v))
            || self
                .acceleration_q32
                .iter()
                .any(|v| !(-32 * ONE..=32 * ONE).contains(v))
            || (self.fixed && (self.velocity_q32 != [0; 2] || self.acceleration_q32 != [0; 2]))
        {
            return Err(ERROR);
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), RynvekErrorCode> {
        self.validate_components()?;
        if self
            .position_q32
            .iter()
            .any(|v| *v < self.radius_q32 || *v > SIDE - self.radius_q32)
        {
            return Err(ERROR);
        }
        Ok(())
    }
    fn inverse_mass(&self) -> i64 {
        if self.fixed {
            0
        } else {
            ((i128::from(ONE) * i128::from(ONE)) / i128::from(self.mass_q32)) as i64
        }
    }
}

/// 每120Hz子步先速度后位置；全场壁是明确的外部约束。
pub fn integrate_substep(body: &mut Body) -> Result<(), RynvekErrorCode> {
    body.validate()?;
    if !body.fixed {
        for axis in 0..2 {
            body.velocity_q32[axis] += body.acceleration_q32[axis] / 120;
            if !(-8 * ONE..=8 * ONE).contains(&body.velocity_q32[axis]) {
                return Err(ERROR);
            }
            body.position_q32[axis] += body.velocity_q32[axis] / 120;
        }
    }
    reflect_walls(body)?;
    body.validate()
}
pub fn separation(a: &Body, b: &Body) -> Result<i64, RynvekErrorCode> {
    a.validate()?;
    b.validate()?;
    Ok(geometry(a, b).1)
}
fn mul(a: i64, b: i64) -> i64 {
    Q32_32(a).mul(Q32_32(b)).0
}
fn div(a: i64, b: i64) -> i64 {
    Q32_32(a).div_safe(Q32_32(b), Q32_32::ZERO).0
}
fn geometry(a: &Body, b: &Body) -> ([i64; 2], i64) {
    let d = [
        b.position_q32[0] - a.position_q32[0],
        b.position_q32[1] - a.position_q32[1],
    ];
    let sq = i128::from(d[0]) * i128::from(d[0]) + i128::from(d[1]) * i128::from(d[1]);
    (d, (sq as u128).isqrt() as i64)
}
pub fn reflect_walls(b: &mut Body) -> Result<(), RynvekErrorCode> {
    b.validate_components()?;
    if b.fixed {
        return b.validate();
    }
    for axis in 0..2 {
        let lo = b.radius_q32;
        let hi = SIDE - lo;
        if b.position_q32[axis] < lo {
            b.position_q32[axis] = lo;
            b.velocity_q32[axis] = b.velocity_q32[axis].abs();
        }
        if b.position_q32[axis] > hi {
            b.position_q32[axis] = hi;
            b.velocity_q32[axis] = -b.velocity_q32[axis].abs();
        }
    }
    b.validate()
}
pub fn resolve_contact(a: &mut Body, b: &mut Body) -> Result<(), RynvekErrorCode> {
    a.validate()?;
    b.validate()?;
    let (d, dist) = geometry(a, b);
    let penetration = a.radius_q32 + b.radius_q32 - dist;
    if penetration < 0 {
        return Ok(());
    }
    let ia = a.inverse_mass();
    let ib = b.inverse_mass();
    let sum = ia + ib;
    if sum == 0 {
        return if penetration <= 64 { Ok(()) } else { Err(ERROR) };
    }
    let n = if dist == 0 {
        [ONE, 0]
    } else {
        [div(d[0], dist), div(d[1], dist)]
    };
    let relative = mul(b.velocity_q32[0] - a.velocity_q32[0], n[0]) + mul(b.velocity_q32[1] - a.velocity_q32[1], n[1]);
    if relative < 0 {
        let impulse = div(-2 * relative, sum);
        for (axis, normal) in n.into_iter().enumerate() {
            a.velocity_q32[axis] -= mul(mul(impulse, ia), normal);
            b.velocity_q32[axis] += mul(mul(impulse, ib), normal);
        }
    }
    if penetration > 0 {
        let move_a = mul(penetration + 4, div(ia, sum));
        let move_b = penetration + 4 - move_a;
        for (axis, normal) in n.into_iter().enumerate() {
            a.position_q32[axis] -= mul(move_a, normal);
            b.position_q32[axis] += mul(move_b, normal);
        }
    }
    // 每次接触后立刻落实速度界，不能让密集接触在下次检查前放大中间态。
    if a.velocity_q32
        .iter()
        .chain(b.velocity_q32.iter())
        .any(|v| !(-8 * ONE..=8 * ONE).contains(v))
    {
        return Err(ERROR);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body() -> Body {
        Body {
            position_q32: [10 * ONE; 2],
            velocity_q32: [0; 2],
            acceleration_q32: [0; 2],
            radius_q32: ONE,
            mass_q32: ONE,
            fixed: false,
        }
    }
    #[test]
    fn public_circle_kernel_rejects_extreme_input_before_geometry_or_abs() {
        let mut bad = body();
        bad.velocity_q32 = [i64::MIN, i64::MAX];
        bad.position_q32 = [i64::MIN, i64::MAX];
        assert!(integrate_substep(&mut bad).is_err());
        assert!(reflect_walls(&mut bad).is_err());
        assert!(resolve_contact(&mut bad, &mut body()).is_err());
        assert!(separation(&bad, &body()).is_err());
    }
    #[test]
    fn circle_uses_original_signed_q32_rounding_not_a_second_arithmetic() {
        assert_eq!(mul(-1, ONE / 2), -1);
        assert_eq!(div(-1, 2 * ONE), 0);
        assert_eq!(mul(-ONE / 3, ONE / 7), Q32_32(-ONE / 3).mul(Q32_32(ONE / 7)).0);
    }
}
