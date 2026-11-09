// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! L3 战略调制层：信标信息素场注入与四维神经调质广播 `m = [DA, 5-HT, NE, ACh] ∈ [-1, 1]^4`。
//!
//! 规范只规定了接口（`configure_signal_beacon` 的 `signal_strength ∈ [-50, 50]` 与 `neuromodulators`），
//! 以下为确定性实现（ERRATA E-027）：
//! - **场注入**：每个场步前，信标作用半径 `R` 内的格被钉向抛物衰减目标
//!   `target(d) = s · (R² − d²) / R²`；仅当 `|target| ≥ |v|` 时覆盖（信标只「抬高」不「抹平」更强的场）。
//!   多个信标按实体序号依次施加。
//! - **调质广播**：单位感受到的调质为半径内全部广播源向量之和，再逐分量钳制到 `[-1, 1]`；
//!   求和满足交换律，结果与遍历顺序无关。
//! - **Leviathan 移动调质光环**：把自身（从信标处）接收到的调质在半径 `8.0` 内再广播一跳，不递归转发。

use rynvek_core::Q32_32;
use serde::{Deserialize, Serialize};

use crate::fields::FieldGrid;
use crate::ids::FieldId;
use crate::math::Vec2;

/// 调质分量下标
pub mod channel {
    pub const DA: usize = 0;
    pub const SEROTONIN: usize = 1;
    pub const NE: usize = 2;
    pub const ACH: usize = 3;
}

/// Q8.8 中 `1.0` 的原始值
pub const MOD_ONE_Q8: i16 = 256;

/// 四维调质向量（Q8.8，每分量 `[-256, 256]`）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Modulators(pub [i16; 4]);

impl Modulators {
    pub const ZERO: Self = Self([0; 4]);

    /// 由毫单位（`[-1000, 1000]`）构造，四舍五入到 Q8.8
    pub fn from_milli(m: [i64; 4]) -> Self {
        Self(m.map(|v| {
            let v = v.clamp(-1000, 1000);
            ((v * 256 + v.signum() * 500) / 1000) as i16
        }))
    }

    /// 分量和后钳制到 `[-1, 1]`
    pub fn saturating_sum<'a>(items: impl IntoIterator<Item = &'a Modulators>) -> Self {
        let mut acc = [0i32; 4];
        for m in items {
            for (a, v) in acc.iter_mut().zip(m.0) {
                *a += v as i32;
            }
        }
        Self(acc.map(|a| a.clamp(-(MOD_ONE_Q8 as i32), MOD_ONE_Q8 as i32) as i16))
    }

    /// 转为毫单位（观测与工具返回）
    pub fn to_milli(self) -> [i64; 4] {
        self.0.map(|v| (v as i64 * 1000) / 256)
    }
}

/// 信标配置（由 `configure_signal_beacon` 设定）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeaconConfig {
    /// 注入的信号通道（`field.signal_a` 或 `field.signal_b`）
    pub channel: FieldId,
    /// 注入强度（Q8.8，`[-12800, 12800]` 即 `[-50, 50]`）
    pub strength_q8: i16,
    pub modulators: Modulators,
}

impl Default for BeaconConfig {
    fn default() -> Self {
        Self {
            channel: FieldId::SignalA,
            strength_q8: 0,
            modulators: Modulators::ZERO,
        }
    }
}

impl BeaconConfig {
    /// 由毫单位强度构造（钳制到 `[-50, 50]`，四舍五入到 Q8.8）
    pub fn strength_from_milli(strength_milli: i64) -> i16 {
        let s = strength_milli.clamp(-50_000, 50_000);
        ((s * 256 + s.signum() * 500) / 1000) as i16
    }
}

/// 一个调质广播源（信标或 Leviathan）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModulatorSource {
    pub position: Vec2,
    pub radius: Q32_32,
    pub modulators: Modulators,
}

impl ModulatorSource {
    #[inline]
    pub fn covers(&self, p: Vec2) -> bool {
        self.position.distance_sq(p) <= self.radius.mul(self.radius)
    }
}

/// 在 `p` 处感受到的调质：所有覆盖源之和后钳制
pub fn received_at(p: Vec2, sources: &[ModulatorSource]) -> Modulators {
    Modulators::saturating_sum(sources.iter().filter(|s| s.covers(p)).map(|s| &s.modulators))
}

/// 把信标的信号注入写入场（每个场步前调用）；`center` 为信标中心 Tile 坐标
pub fn inject_beacon(fields: &mut FieldGrid, center: (i64, i64), radius_tiles: u32, cfg: &BeaconConfig) {
    if cfg.strength_q8 == 0 {
        return;
    }
    let r = radius_tiles as i64;
    let r2 = r * r;
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            if d2 > r2 {
                continue;
            }
            let (x, y) = (center.0 + dx, center.1 + dy);
            if x < 0 || y < 0 || x >= fields.width() as i64 || y >= fields.height() as i64 {
                continue;
            }
            let target = (cfg.strength_q8 as i64 * (r2 - d2) / r2) as i16;
            let v = fields.get(cfg.channel, x, y);
            if target.unsigned_abs() >= v.unsigned_abs() {
                fields.set(cfg.channel, x as u32, y as u32, target);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milli_conversion_rounds_and_clamps() {
        assert_eq!(
            Modulators::from_milli([1000, -1000, 500, 2000]).0,
            [256, -256, 128, 256]
        );
        assert_eq!(Modulators::from_milli([1, -1, 2, 0]).0, [0, 0, 1, 0]);
        assert_eq!(BeaconConfig::strength_from_milli(50_000), 12_800);
        assert_eq!(BeaconConfig::strength_from_milli(-80_000), -12_800);
        assert_eq!(Modulators([256, -128, 0, 64]).to_milli(), [1000, -500, 0, 250]);
    }

    #[test]
    fn broadcast_sums_and_saturates_within_radius() {
        let s = |x: i32, m: [i16; 4]| ModulatorSource {
            position: Vec2::from_int(x, 0),
            radius: Q32_32::from_int(16),
            modulators: Modulators(m),
        };
        let sources = [s(0, [200, 0, 100, 0]), s(10, [200, -50, 0, 0]), s(100, [-256, 0, 0, 0])];
        assert_eq!(received_at(Vec2::from_int(5, 0), &sources).0, [256, -50, 100, 0]);
        assert_eq!(received_at(Vec2::from_int(17, 0), &sources).0, [200, -50, 0, 0]);
        assert_eq!(received_at(Vec2::from_int(50, 0), &sources), Modulators::ZERO);
        // 交换律：与顺序无关
        let rev: Vec<_> = sources.iter().rev().copied().collect();
        assert_eq!(
            received_at(Vec2::from_int(5, 0), &rev),
            received_at(Vec2::from_int(5, 0), &sources)
        );
    }

    #[test]
    fn beacon_pins_parabolic_profile_without_flattening_stronger_field() {
        let mut g = FieldGrid::new(256, 256);
        let cfg = BeaconConfig {
            channel: FieldId::SignalB,
            strength_q8: 12_800,
            modulators: Modulators::ZERO,
        };
        g.set(FieldId::SignalB, 100, 108, 12_000);
        inject_beacon(&mut g, (100, 100), 16, &cfg);
        assert_eq!(g.get(FieldId::SignalB, 100, 100), 12_800);
        assert_eq!(
            g.get(FieldId::SignalB, 108, 100),
            9_600,
            "d = 8：12800 × (256 − 64) / 256"
        );
        assert_eq!(g.get(FieldId::SignalB, 100, 108), 12_000, "更强的既有场不被覆盖");
        assert_eq!(g.get(FieldId::SignalB, 117, 100), 0);
        assert_eq!(g.get(FieldId::SignalA, 100, 100), 0);
    }
}
