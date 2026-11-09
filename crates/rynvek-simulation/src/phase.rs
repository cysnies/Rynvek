// crates/rynvek-simulation/src/phase.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 16 阶段拓扑信封与提交清单（规范 03 §3.1）。

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanonicalPhaseId {
    ClockAndEpochAdvance = 0,        // [Kernel Immutable]
    CommandIngressAndValidate = 1,   // [Kernel Immutable]
    ScheduledEventDelivery = 2,      // [Kernel Immutable]
    SpatialAndVisibilityIndex = 3,   // [Domain Spatial]
    EnvironmentAndFieldSolve = 4,    // [Domain Field - 可与 Phase 05/06 并发]
    SensoryAndPerceptionGather = 5,  // [Domain Neural]
    NeuralDynamicsAndPlasticity = 6, // [Domain Neural]
    ControllerAndRuleEval = 7,       // [Domain Rules]
    ActionArbitration = 8,           // [Domain Arbitration]
    KinematicsAndCollision = 9,      // [Domain Physics]
    CombatAndEconomySettle = 10,     // [Domain RTS/Interaction]
    MetabolismAndLifecycle = 11,     // [Domain Bio]
    EvolutionAndMutation = 12,       // [Domain Bio - Epoch Gated]
    StructuralMutationApply = 13,    // [Kernel Immutable]
    InvariantAndNumericCheck = 14,   // [Kernel Immutable]
    CommitAndPublication = 15,       // [Kernel Immutable]
}

impl CanonicalPhaseId {
    /// 按规范顺序排列的全部 16 个阶段
    pub const ALL: [CanonicalPhaseId; 16] = [
        Self::ClockAndEpochAdvance,
        Self::CommandIngressAndValidate,
        Self::ScheduledEventDelivery,
        Self::SpatialAndVisibilityIndex,
        Self::EnvironmentAndFieldSolve,
        Self::SensoryAndPerceptionGather,
        Self::NeuralDynamicsAndPlasticity,
        Self::ControllerAndRuleEval,
        Self::ActionArbitration,
        Self::KinematicsAndCollision,
        Self::CombatAndEconomySettle,
        Self::MetabolismAndLifecycle,
        Self::EvolutionAndMutation,
        Self::StructuralMutationApply,
        Self::InvariantAndNumericCheck,
        Self::CommitAndPublication,
    ];

    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    pub const fn is_kernel_immutable(self) -> bool {
        matches!(
            self,
            Self::ClockAndEpochAdvance
                | Self::CommandIngressAndValidate
                | Self::ScheduledEventDelivery
                | Self::StructuralMutationApply
                | Self::InvariantAndNumericCheck
                | Self::CommitAndPublication
        )
    }
}

/// 不可裁剪的内核信封阶段（GAP-05）
pub const KERNEL_IMMUTABLE_PHASES: [CanonicalPhaseId; 6] = [
    CanonicalPhaseId::ClockAndEpochAdvance,
    CanonicalPhaseId::CommandIngressAndValidate,
    CanonicalPhaseId::ScheduledEventDelivery,
    CanonicalPhaseId::StructuralMutationApply,
    CanonicalPhaseId::InvariantAndNumericCheck,
    CanonicalPhaseId::CommitAndPublication,
];

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickCommitManifest {
    pub world_id: [u8; 16],
    pub tick: u64,
    pub world_revision: u64,
    pub parent_commit_hash: [u8; 32],
    pub entity_root_hash: [u8; 32],
    pub field_root_hash: [u8; 32],
    pub neural_root_hash: [u8; 32],
    pub economy_root_hash: [u8; 32],
    pub rng_root_hash: [u8; 32],
    /// 由以上 6 个 32B 子根拼接后计算的最终 32B BLAKE3 状态根
    pub state_root_hash: [u8; 32],
}

impl TickCommitManifest {
    /// 由六个子根确定性计算 `state_root_hash`
    pub fn compute_state_root(
        entity_root_hash: &[u8; 32],
        field_root_hash: &[u8; 32],
        neural_root_hash: &[u8; 32],
        economy_root_hash: &[u8; 32],
        rng_root_hash: &[u8; 32],
        parent_commit_hash: &[u8; 32],
    ) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(entity_root_hash);
        hasher.update(field_root_hash);
        hasher.update(neural_root_hash);
        hasher.update(economy_root_hash);
        hasher.update(rng_root_hash);
        hasher.update(parent_commit_hash);
        *hasher.finalize().as_bytes()
    }
}
