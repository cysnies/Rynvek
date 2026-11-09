// crates/rynvek-core/src/error.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨语言统一错误码注册表（`RynvekErrorCode` `0x1000..0x9FFF`）与恢复策略（规范 11 §11.4）。
//!
//! 错误码数值是跨语言 ABI 的一部分，TypeScript 端 `@rynvek/core` 必须逐项保持一致。

use core::fmt;

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RynvekErrorCode {
    // --- 0x1000：内核、坐标与数值错误 ---
    NumericNaNOrInfDetected = 0x1001,
    FixedPointOverflow = 0x1002,
    CoordinateOutOfCanonicalRange = 0x1003,
    DimensionMismatch = 0x1004,

    // --- 0x2000：ECS、句柄与内存错误 ---
    StaleEntityHandleGeneration = 0x2001,
    EntityNotFoundOrTombstoned = 0x2002,
    ArchetypeComponentMissing = 0x2003,
    WorldMemoryBudgetExceeded = 0x2004,
    StructuralMutationConflict = 0x2005,

    // --- 0x3000：仿真、时钟与阶段 DAG 错误 ---
    PhaseDependencyCycleDetected = 0x3001,
    UnresolvedWriteConflict = 0x3002,
    SimulationInvariantViolated = 0x3003,
    TickDeadlineExceeded = 0x3004,

    // --- 0x4000：神经汤与连续场错误 ---
    NeuralTopologyCapacityExceeded = 0x4001,
    DaleLawSignViolation = 0x4002,
    FieldCflStabilityViolation = 0x4003,
    ConservationLawMismatch = 0x4004,

    // --- 0x5000：规则编译器、QuickJS 与 WASM 沙箱错误 ---
    RuleStaticTypeCheckFailed = 0x5001,
    RuleUndeclaredWriteAccess = 0x5002,
    RuleLoopBoundExceeded = 0x5003,
    SandboxGasBudgetExhausted = 0x5004,
    SandboxMemoryLimitExceeded = 0x5005,
    PluginCapabilityDenied = 0x5006,

    // --- 0x6000：WebGPU、计算与渲染错误 ---
    WebGpuAdapterUnavailable = 0x6001,
    GpuDeviceLost = 0x6002,
    GpuBufferAllocationFailed = 0x6003,
    WgslShaderValidationFailed = 0x6004,

    // --- 0x7000：存储、快照、日志与迁移错误 ---
    InvalidFileMagicOrVersion = 0x7001,
    Blake3ChecksumMismatch = 0x7002,
    JournalTornWriteTruncated = 0x7003,
    SchemaMigrationPathNotFound = 0x7004,
    StalePartitionFencingToken = 0x7005,

    // --- 0x8000：网络、命令、对局与安全错误 ---
    UnauthorizedCapabilityToken = 0x8001,
    DuplicateCommandIdempotencyKey = 0x8002,
    CommandExpiredOrFutureTooFar = 0x8003,
    CommandRateLimitExceeded = 0x8004,
    InsufficientFactionResources = 0x8005,
    InvalidBuildingPlacementGrid = 0x8006,

    // --- 0x9000：资源包、Hub 与资产管线错误 ---
    InvalidEd25519Signature = 0x9001,
    RevokedPackageBlocked = 0x9002,
    DependencyLockConflict = 0x9003,
    UnsafeRviewShaderRejected = 0x9004,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorRecoveryAction {
    /// 仅拒绝当前单条非法玩家/Agent 命令，世界继续正常推进 (适用于 0x8001..0x8006)
    RejectCommandOnly = 0,
    /// 立即隔离并禁用超限的第三方规则或 WASM 插件，回退该插件当帧写入 (适用于 0x5004..0x5006)
    IsolateFaultyExtension = 1,
    /// 丢弃当前未提交的 CandidateState[t]，回滚至 Committed[t-1] 并告警 (适用于 0x1001, 0x3003, 0x4004)
    RollbackTickCandidate = 2,
    /// 触发 GPU DeviceLost 5 步恢复机或自动降级至 CPU Reference 后端 (适用于 0x6001..0x6003)
    RecoverOrDegradeBackend = 3,
    /// 从最近 .rsnap + .rjournal 执行完整崩溃恢复状态机 (适用于 0x7002..0x7003)
    TriggerStorageRecovery = 4,
}

/// 错误码所属的九大错误域（取错误码最高 4 位）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorDomain {
    /// 0x1000：内核、坐标与数值
    CoreNumeric = 0x1,
    /// 0x2000：ECS、句柄与内存
    EcsMemory = 0x2,
    /// 0x3000：仿真、时钟与阶段 DAG
    Simulation = 0x3,
    /// 0x4000：神经汤与连续场
    NeuralField = 0x4,
    /// 0x5000：规则编译器与沙箱
    RulesSandbox = 0x5,
    /// 0x6000：WebGPU、计算与渲染
    GpuCompute = 0x6,
    /// 0x7000：存储、快照、日志与迁移
    Storage = 0x7,
    /// 0x8000：网络、命令、对局与安全
    NetworkSecurity = 0x8,
    /// 0x9000：资源包、Hub 与资产管线
    PackageHub = 0x9,
}

impl RynvekErrorCode {
    /// 注册表中的全部错误码（按数值升序），用于跨语言一致性校验与遍历
    pub const ALL: [Self; 42] = [
        Self::NumericNaNOrInfDetected,
        Self::FixedPointOverflow,
        Self::CoordinateOutOfCanonicalRange,
        Self::DimensionMismatch,
        Self::StaleEntityHandleGeneration,
        Self::EntityNotFoundOrTombstoned,
        Self::ArchetypeComponentMissing,
        Self::WorldMemoryBudgetExceeded,
        Self::StructuralMutationConflict,
        Self::PhaseDependencyCycleDetected,
        Self::UnresolvedWriteConflict,
        Self::SimulationInvariantViolated,
        Self::TickDeadlineExceeded,
        Self::NeuralTopologyCapacityExceeded,
        Self::DaleLawSignViolation,
        Self::FieldCflStabilityViolation,
        Self::ConservationLawMismatch,
        Self::RuleStaticTypeCheckFailed,
        Self::RuleUndeclaredWriteAccess,
        Self::RuleLoopBoundExceeded,
        Self::SandboxGasBudgetExhausted,
        Self::SandboxMemoryLimitExceeded,
        Self::PluginCapabilityDenied,
        Self::WebGpuAdapterUnavailable,
        Self::GpuDeviceLost,
        Self::GpuBufferAllocationFailed,
        Self::WgslShaderValidationFailed,
        Self::InvalidFileMagicOrVersion,
        Self::Blake3ChecksumMismatch,
        Self::JournalTornWriteTruncated,
        Self::SchemaMigrationPathNotFound,
        Self::StalePartitionFencingToken,
        Self::UnauthorizedCapabilityToken,
        Self::DuplicateCommandIdempotencyKey,
        Self::CommandExpiredOrFutureTooFar,
        Self::CommandRateLimitExceeded,
        Self::InsufficientFactionResources,
        Self::InvalidBuildingPlacementGrid,
        Self::InvalidEd25519Signature,
        Self::RevokedPackageBlocked,
        Self::DependencyLockConflict,
        Self::UnsafeRviewShaderRejected,
    ];

    /// 返回跨语言稳定的 16 位错误码数值
    #[inline]
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 由数值反查错误码；未注册的数值返回 `None`
    pub fn from_code(code: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|candidate| candidate.code() == code)
    }

    /// 返回错误码所属的错误域
    pub const fn domain(self) -> ErrorDomain {
        match self.code() >> 12 {
            0x1 => ErrorDomain::CoreNumeric,
            0x2 => ErrorDomain::EcsMemory,
            0x3 => ErrorDomain::Simulation,
            0x4 => ErrorDomain::NeuralField,
            0x5 => ErrorDomain::RulesSandbox,
            0x6 => ErrorDomain::GpuCompute,
            0x7 => ErrorDomain::Storage,
            0x8 => ErrorDomain::NetworkSecurity,
            _ => ErrorDomain::PackageHub,
        }
    }

    /// 返回规范强制的运行期故障恢复动作。
    ///
    /// 规范仅为部分错误码规定了恢复动作；其余错误码（例如编译期拒绝构建的规则错误）
    /// 由调用方按上下文处理，此处返回 `None`。
    pub const fn recovery_action(self) -> Option<ErrorRecoveryAction> {
        use RynvekErrorCode as E;
        match self {
            E::UnauthorizedCapabilityToken
            | E::DuplicateCommandIdempotencyKey
            | E::CommandExpiredOrFutureTooFar
            | E::CommandRateLimitExceeded
            | E::InsufficientFactionResources
            | E::InvalidBuildingPlacementGrid => Some(ErrorRecoveryAction::RejectCommandOnly),
            E::SandboxGasBudgetExhausted | E::SandboxMemoryLimitExceeded | E::PluginCapabilityDenied => {
                Some(ErrorRecoveryAction::IsolateFaultyExtension)
            }
            E::NumericNaNOrInfDetected | E::SimulationInvariantViolated | E::ConservationLawMismatch => {
                Some(ErrorRecoveryAction::RollbackTickCandidate)
            }
            E::WebGpuAdapterUnavailable | E::GpuDeviceLost | E::GpuBufferAllocationFailed => {
                Some(ErrorRecoveryAction::RecoverOrDegradeBackend)
            }
            E::Blake3ChecksumMismatch | E::JournalTornWriteTruncated => {
                Some(ErrorRecoveryAction::TriggerStorageRecovery)
            }
            _ => None,
        }
    }
}

impl fmt::Display for RynvekErrorCode {
    /// 统一展示格式：`RV-0x1003 CoordinateOutOfCanonicalRange`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RV-{:#06X} {:?}", self.code(), self)
    }
}

impl std::error::Error for RynvekErrorCode {}

impl From<RynvekErrorCode> for u16 {
    fn from(code: RynvekErrorCode) -> Self {
        code.code()
    }
}

impl TryFrom<u16> for RynvekErrorCode {
    type Error = u16;

    /// 未注册的数值原样作为错误返回
    fn try_from(code: u16) -> Result<Self, Self::Error> {
        Self::from_code(code).ok_or(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_codes_are_sorted_unique_and_in_range() {
        let codes: Vec<u16> = RynvekErrorCode::ALL.iter().map(|c| c.code()).collect();
        assert!(codes.windows(2).all(|w| w[0] < w[1]), "ALL 必须严格升序且无重复");
        assert!(codes.iter().all(|c| (0x1000..=0x9FFF).contains(c)));
    }

    #[test]
    fn code_roundtrips_through_u16() {
        for code in RynvekErrorCode::ALL {
            assert_eq!(RynvekErrorCode::try_from(u16::from(code)), Ok(code));
        }
        assert_eq!(RynvekErrorCode::try_from(0x1000), Err(0x1000));
        assert_eq!(RynvekErrorCode::from_code(0xFFFF), None);
    }

    #[test]
    fn domain_follows_high_nibble() {
        assert_eq!(RynvekErrorCode::DimensionMismatch.domain(), ErrorDomain::CoreNumeric);
        assert_eq!(
            RynvekErrorCode::RuleLoopBoundExceeded.domain(),
            ErrorDomain::RulesSandbox
        );
        assert_eq!(
            RynvekErrorCode::UnsafeRviewShaderRejected.domain(),
            ErrorDomain::PackageHub
        );
        for code in RynvekErrorCode::ALL {
            assert_eq!(code.domain() as u16, code.code() >> 12);
        }
    }

    #[test]
    fn recovery_actions_follow_spec_table() {
        use ErrorRecoveryAction as A;
        use RynvekErrorCode as E;
        assert_eq!(
            E::CommandRateLimitExceeded.recovery_action(),
            Some(A::RejectCommandOnly)
        );
        assert_eq!(
            E::SandboxGasBudgetExhausted.recovery_action(),
            Some(A::IsolateFaultyExtension)
        );
        assert_eq!(
            E::SimulationInvariantViolated.recovery_action(),
            Some(A::RollbackTickCandidate)
        );
        assert_eq!(E::GpuDeviceLost.recovery_action(), Some(A::RecoverOrDegradeBackend));
        assert_eq!(
            E::JournalTornWriteTruncated.recovery_action(),
            Some(A::TriggerStorageRecovery)
        );
        assert_eq!(E::RuleUndeclaredWriteAccess.recovery_action(), None);
    }

    #[test]
    fn display_is_stable() {
        assert_eq!(
            RynvekErrorCode::CoordinateOutOfCanonicalRange.to_string(),
            "RV-0x1003 CoordinateOutOfCanonicalRange"
        );
    }
}
