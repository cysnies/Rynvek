// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 计算能力档位（规范目录树注释「CapabilityProfile (Tier A-D)」）。
//!
//! 由 Adapter/设备 limits 在运行时判定；CPU Reference 固定报告为 [`CapabilityProfile::TierD`]。

/// 硬件计算能力档位（数值越大能力越强）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityProfile {
    /// Tier D：仅 CPU / 最低限度 WebGPU（无间接绘制、存储缓冲较小）
    TierD = 0,
    /// Tier C：基础计算着色器可用
    TierC = 1,
    /// Tier B：推荐档，支持分段 32 MiB 存储缓冲与常用扩展
    TierB = 2,
    /// Tier A：高端，支持大规模并行与高级特性
    TierA = 3,
}

impl CapabilityProfile {
    /// 是否至少达到 `required`
    pub const fn meets(self, required: Self) -> bool {
        (self as u8) >= (required as u8)
    }

    /// 人类可读名称
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TierD => "TierD",
            Self::TierC => "TierC",
            Self::TierB => "TierB",
            Self::TierA => "TierA",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_and_meets() {
        assert!(CapabilityProfile::TierA.meets(CapabilityProfile::TierB));
        assert!(!CapabilityProfile::TierD.meets(CapabilityProfile::TierC));
        assert_eq!(CapabilityProfile::TierB.as_str(), "TierB");
    }
}
