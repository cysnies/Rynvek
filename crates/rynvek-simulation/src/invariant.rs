// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 仿真不变量检查器（规范 03 §3.2，`Phase 14`）。

use rynvek_core::error::RynvekErrorCode;

/// 五项零容忍不变量标识
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum InvariantId {
    NoNaNOrInf = 1,
    ResourceNonNegative = 2,
    MassEnergyConservation = 3,
    HandleReferentialIntegrity = 4,
    CoordinateCanonicalBounds = 5,
}

impl InvariantId {
    pub const ALL: [InvariantId; 5] = [
        Self::NoNaNOrInf,
        Self::ResourceNonNegative,
        Self::MassEnergyConservation,
        Self::HandleReferentialIntegrity,
        Self::CoordinateCanonicalBounds,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoNaNOrInf => "INV-01",
            Self::ResourceNonNegative => "INV-02",
            Self::MassEnergyConservation => "INV-03",
            Self::HandleReferentialIntegrity => "INV-04",
            Self::CoordinateCanonicalBounds => "INV-05",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvariantReport {
    pub failed: Vec<(InvariantId, RynvekErrorCode)>,
}

impl InvariantReport {
    pub fn ok() -> Self {
        Self { failed: Vec::new() }
    }

    pub fn is_ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Phase 14 检查所需的只读快照（M2 最小可测子集）
#[derive(Clone, Debug, Default)]
pub struct InvariantSnapshot {
    /// GPU / 定点诊断计数器 `s_diagnostics[0]`
    pub diagnostics_nonzero_count: u64,
    pub biomass: i64,
    pub flux: i64,
    pub neurogel: i64,
    pub biomass_capacity: i64,
    pub flux_capacity: i64,
    pub neurogel_capacity: i64,
    /// 本帧采集 Biomass 增量
    pub biomass_harvested: i64,
    /// 本帧环境场 Nutrient 扣除（milli-unit）
    pub nutrient_consumed: i64,
    /// 本帧残骸回收
    pub carcass_reclaimed: i64,
    /// 封闭世界模式
    pub closed_world: bool,
    /// 活跃句柄是否全部指向 Alive
    pub all_handles_alive: bool,
    /// 实体局部坐标（Q32.32）
    pub local_coords: Vec<(u64, u64)>,
}

/// 坐标规范上界：`256 * 2^32`（INV-05）
pub const COORD_CANONICAL_MAX: u64 = 256u64 << 32;

#[derive(Clone, Debug, Default)]
pub struct InvariantRegistry;

impl InvariantRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, snap: &InvariantSnapshot) -> InvariantReport {
        let mut failed = Vec::new();

        // INV-01
        if snap.diagnostics_nonzero_count != 0 {
            failed.push((InvariantId::NoNaNOrInf, RynvekErrorCode::NumericNaNOrInfDetected));
        }

        // INV-02
        let resources = [
            (snap.biomass, snap.biomass_capacity),
            (snap.flux, snap.flux_capacity),
            (snap.neurogel, snap.neurogel_capacity),
        ];
        for (bal, cap) in resources {
            if bal < 0 || (cap >= 0 && bal > cap) {
                failed.push((
                    InvariantId::ResourceNonNegative,
                    RynvekErrorCode::SimulationInvariantViolated,
                ));
                break;
            }
        }

        // INV-03
        if snap.closed_world {
            let expected = snap.nutrient_consumed.saturating_add(snap.carcass_reclaimed);
            if snap.biomass_harvested != expected {
                failed.push((
                    InvariantId::MassEnergyConservation,
                    RynvekErrorCode::ConservationLawMismatch,
                ));
            }
        }

        // INV-04
        if !snap.all_handles_alive {
            failed.push((
                InvariantId::HandleReferentialIntegrity,
                RynvekErrorCode::StaleEntityHandleGeneration,
            ));
        }

        // INV-05
        for &(lx, ly) in &snap.local_coords {
            if lx >= COORD_CANONICAL_MAX || ly >= COORD_CANONICAL_MAX {
                failed.push((
                    InvariantId::CoordinateCanonicalBounds,
                    RynvekErrorCode::CoordinateOutOfCanonicalRange,
                ));
                break;
            }
        }

        InvariantReport { failed }
    }
}
