// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 16 阶段 Tick 推进引擎（规范 03 / 审计报告 §7.1）。
//!
//! 通用单 Chunk 物理 ECS 世界：完整走过 16 个阶段，不隐式创建 RTS 世界，
//! 串联 `rynvek-field`（按 10Hz 多速率门控 `clock.is_field_step_due()` 推进）与 `rynvek-spatial`，
//! 并保证 `10_000` Tick 后状态根可复现（零误差）。

use rynvek_compute::{ComputeBackend, KernelId};
use rynvek_compute_cpu::CpuReferenceBackend;
use rynvek_core::error::RynvekErrorCode;
use rynvek_field::FieldWorld;
use rynvek_spatial::SpatialCsrIndex;

use crate::clock::MultiRateClock;
use crate::invariant::{COORD_CANONICAL_MAX, InvariantRegistry, InvariantSnapshot};
use crate::phase::{CanonicalPhaseId, TickCommitManifest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimulationError {
    Invariant(RynvekErrorCode),
    Field(RynvekErrorCode),
    Spatial(RynvekErrorCode),
    Dispatch(String),
}

/// 单 Chunk 通用物理世界；神经、规则与跨 Chunk 主体仍待接入。
#[derive(Clone, Debug)]
pub struct SimulationEngine {
    pub clock: MultiRateClock,
    pub world_id: [u8; 16],
    pub field: FieldWorld,
    pub(crate) physics: crate::physics::PhysicsWorld,
    pub spatial: SpatialCsrIndex,
    pub last_manifest: Option<TickCommitManifest>,
    pub parent_commit_hash: [u8; 32],
    /// 经济账本（milli-unit）
    pub biomass: i64,
    pub flux: i64,
    pub neurogel: i64,
    pub biomass_capacity: i64,
    pub flux_capacity: i64,
    pub neurogel_capacity: i64,
    /// 本帧临时计量（Phase 10/11 写入，Phase 14 校验）
    frame_biomass_harvested: i64,
    frame_nutrient_consumed: i64,
    frame_carcass_reclaimed: i64,
    pub closed_world: bool,
    pub diagnostics_nonzero_count: u64,
    invariants: InvariantRegistry,
    backend: CpuReferenceBackend,
    /// 各阶段已执行计数（调试/验收）
    pub phase_exec_counts: [u64; 16],
    /// 已执行的 10Hz 连续环境场步数（每 3 个 30Hz Tick 递增 1）
    pub field_steps_completed: u64,
}

impl Default for SimulationEngine {
    fn default() -> Self {
        Self::new([0u8; 16])
    }
}

impl SimulationEngine {
    pub fn new(world_id: [u8; 16]) -> Self {
        let mut world = Self {
            clock: MultiRateClock::new(),
            world_id,
            field: FieldWorld::new(),
            physics: crate::physics::PhysicsWorld::new(world_id),
            spatial: SpatialCsrIndex::empty(),
            last_manifest: None,
            parent_commit_hash: [0u8; 32],
            biomass: 0,
            flux: 0,
            neurogel: 0,
            biomass_capacity: 2000,
            flux_capacity: 1000,
            neurogel_capacity: 500,
            frame_biomass_harvested: 0,
            frame_nutrient_consumed: 0,
            frame_carcass_reclaimed: 0,
            closed_world: true,
            diagnostics_nonzero_count: 0,
            invariants: InvariantRegistry::new(),
            backend: CpuReferenceBackend::new(),
            phase_exec_counts: [0; 16],
            field_steps_completed: 0,
        };
        world.publish_commit(world.authoritative_commit([0; 32]));
        world
    }

    /// 创世物资与实体必须由调用者明确提供；不注入 RTS 阵营、战斗或生物资源。
    pub fn with_bodies(world_id: [u8; 16], bodies: &[crate::physics::Body]) -> Result<Self, SimulationError> {
        if bodies.len() > crate::physics::MAX_BODIES {
            return Err(SimulationError::Invariant(RynvekErrorCode::DimensionMismatch));
        }
        let mut world = Self::new(world_id);
        for batch in bodies.chunks(crate::physics::MAX_COMMANDS) {
            let commands: Vec<_> = batch
                .iter()
                .map(|&body| crate::physics::PhysicsCommand::Spawn { body })
                .collect();
            world.physics.apply(&commands, 0).map_err(SimulationError::Invariant)?;
        }
        world.spatial =
            SpatialCsrIndex::build_from_local_q32(&world.physics.positions()).map_err(SimulationError::Spatial)?;
        world.publish_commit(world.authoritative_commit([0; 32]));
        Ok(world)
    }

    /// 推进一个完整 Tick（16 阶段）；成功则更新 `last_manifest`
    pub fn tick(&mut self) -> Result<TickCommitManifest, SimulationError> {
        self.tick_with_commands(&[])
    }

    fn tick_candidate(
        &mut self,
        commands: &[crate::physics::PhysicsCommand],
    ) -> Result<TickCommitManifest, SimulationError> {
        // 帧计量清零
        self.frame_biomass_harvested = 0;
        self.frame_nutrient_consumed = 0;
        self.frame_carcass_reclaimed = 0;

        for phase in CanonicalPhaseId::ALL {
            if phase == CanonicalPhaseId::CommandIngressAndValidate {
                self.physics
                    .apply(commands, self.clock.coupling_epoch)
                    .map_err(SimulationError::Invariant)?;
            }
            self.run_phase(phase)?;
            self.phase_exec_counts[phase.as_u16() as usize] += 1;
        }

        let manifest = self.last_manifest.clone().expect("Commit 阶段必须写入 manifest");
        Ok(manifest)
    }

    pub fn physics(&self) -> &crate::physics::PhysicsWorld {
        &self.physics
    }

    /// 命令只改变候选世界；任意后续阶段失败，时钟、场、结构、资源和提交根全部不发布。
    pub fn tick_with_commands(
        &mut self,
        commands: &[crate::physics::PhysicsCommand],
    ) -> Result<TickCommitManifest, SimulationError> {
        if self.clock.tick == u64::MAX || self.clock.world_revision == u64::MAX {
            return Err(SimulationError::Invariant(RynvekErrorCode::DimensionMismatch));
        }
        let mut candidate = self.clone();
        let manifest = candidate.tick_candidate(commands)?;
        *self = candidate;
        Ok(manifest)
    }

    pub(crate) fn run_phase(&mut self, phase: CanonicalPhaseId) -> Result<(), SimulationError> {
        match phase {
            CanonicalPhaseId::ClockAndEpochAdvance => {
                self.clock.advance();
                Ok(())
            }
            CanonicalPhaseId::CommandIngressAndValidate => {
                // M2：无外部命令；保留阶段槽位
                Ok(())
            }
            CanonicalPhaseId::ScheduledEventDelivery => Ok(()),
            CanonicalPhaseId::SpatialAndVisibilityIndex => {
                self.backend
                    .dispatch_1d(
                        KernelId("spatial.build_uniform_grid_csr"),
                        self.physics.bodies.len() as u32,
                    )
                    .map_err(|e| SimulationError::Dispatch(format!("{e:?}")))?;
                self.spatial = SpatialCsrIndex::build_from_local_q32(&self.physics.positions())
                    .map_err(SimulationError::Spatial)?;
                Ok(())
            }
            CanonicalPhaseId::EnvironmentAndFieldSolve => {
                // 10Hz 多速率门控：每 3 个 30Hz 权威 Tick 求解一次反应-扩散环境场（与 FIELD_UPDATE_INTERVAL_TICKS=3 对齐）
                if self.clock.is_field_step_due() {
                    self.backend
                        .dispatch_1d(KernelId("field.reaction_diffusion_q8"), 1)
                        .map_err(|e| SimulationError::Dispatch(format!("{e:?}")))?;
                    self.field.step_all().map_err(SimulationError::Field)?;
                    self.field_steps_completed = self.field_steps_completed.saturating_add(1);
                }
                Ok(())
            }
            CanonicalPhaseId::SensoryAndPerceptionGather
            | CanonicalPhaseId::NeuralDynamicsAndPlasticity
            | CanonicalPhaseId::ControllerAndRuleEval => {
                self.backend
                    .dispatch_1d(KernelId("sim.tick_phase_noop"), 1)
                    .map_err(|e| SimulationError::Dispatch(format!("{e:?}")))?;
                Ok(())
            }
            CanonicalPhaseId::ActionArbitration => Ok(()),
            CanonicalPhaseId::KinematicsAndCollision => {
                self.physics.step().map_err(SimulationError::Invariant)?;
                self.spatial = SpatialCsrIndex::build_from_local_q32(&self.physics.positions())
                    .map_err(SimulationError::Spatial)?;
                Ok(())
            }
            CanonicalPhaseId::CombatAndEconomySettle => {
                // 封闭世界零交互：harvest == nutrient + carcass == 0
                self.frame_biomass_harvested = 0;
                self.frame_nutrient_consumed = 0;
                self.frame_carcass_reclaimed = 0;
                Ok(())
            }
            CanonicalPhaseId::MetabolismAndLifecycle => Ok(()),
            CanonicalPhaseId::EvolutionAndMutation => {
                // Epoch 门控：非边界 Tick 跳过实质工作
                let _ = self.clock.is_evolution_gate_open();
                Ok(())
            }
            CanonicalPhaseId::StructuralMutationApply => Ok(()),
            CanonicalPhaseId::InvariantAndNumericCheck => {
                let snap = InvariantSnapshot {
                    diagnostics_nonzero_count: self.diagnostics_nonzero_count,
                    biomass: self.biomass,
                    flux: self.flux,
                    neurogel: self.neurogel,
                    biomass_capacity: self.biomass_capacity,
                    flux_capacity: self.flux_capacity,
                    neurogel_capacity: self.neurogel_capacity,
                    biomass_harvested: self.frame_biomass_harvested,
                    nutrient_consumed: self.frame_nutrient_consumed,
                    carcass_reclaimed: self.frame_carcass_reclaimed,
                    closed_world: self.closed_world,
                    all_handles_alive: true,
                    local_coords: Vec::new(), // 坐标已在 Phase 09 校验；此处避免每 Tick 分配
                };
                // INV-05 复检（无分配）
                for &(lx, ly) in &self.physics.positions() {
                    if lx >= COORD_CANONICAL_MAX || ly >= COORD_CANONICAL_MAX {
                        return Err(SimulationError::Invariant(
                            RynvekErrorCode::CoordinateOutOfCanonicalRange,
                        ));
                    }
                }
                let report = self.invariants.check(&snap);
                if let Some((_, code)) = report.failed.first() {
                    return Err(SimulationError::Invariant(*code));
                }
                Ok(())
            }
            CanonicalPhaseId::CommitAndPublication => {
                self.publish_commit(self.authoritative_commit(self.parent_commit_hash));
                Ok(())
            }
        }
    }

    pub(crate) fn publish_commit(&mut self, c: rynvek_journal::CommitManifest) {
        self.parent_commit_hash = c.state_root_hash;
        self.last_manifest = Some(TickCommitManifest {
            world_id: c.world_id,
            tick: c.tick,
            world_revision: c.world_revision,
            parent_commit_hash: c.parent_commit_hash,
            entity_root_hash: c.entity_root_hash,
            field_root_hash: c.field_root_hash,
            neural_root_hash: c.neural_root_hash,
            economy_root_hash: c.economy_root_hash,
            rng_root_hash: c.rng_root_hash,
            state_root_hash: c.state_root_hash,
        });
    }

    pub(crate) fn hash_entities(&self) -> [u8; 32] {
        self.physics.hash()
    }

    pub(crate) fn hash_economy(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.biomass.to_le_bytes());
        h.update(&self.flux.to_le_bytes());
        h.update(&self.neurogel.to_le_bytes());
        h.update(&self.biomass_capacity.to_le_bytes());
        h.update(&self.flux_capacity.to_le_bytes());
        h.update(&self.neurogel_capacity.to_le_bytes());
        *h.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_phases_run_each_tick_and_field_steps_at_10hz() {
        let mut eng = SimulationEngine::new([7u8; 16]);
        eng.tick().unwrap();
        assert!(eng.phase_exec_counts.iter().all(|&c| c == 1));
        assert_eq!(eng.clock.tick, 1);
        assert_eq!(eng.field_steps_completed, 1);

        // 再推进 29 个 Tick（共 30 个 Tick = 1.0 秒），10Hz 场步应精确执行 10 次
        for _ in 0..29 {
            eng.tick().unwrap();
        }
        assert_eq!(eng.clock.tick, 30);
        assert_eq!(eng.field_steps_completed, 10);
    }

    /// 验收：16 阶段 Tick 推进 10,000 Ticks 零误差（两次独立运行状态根位级一致）
    #[test]
    fn ten_thousand_ticks_zero_divergence() {
        const N: u64 = 10_000;
        let run = || {
            let mut eng = SimulationEngine::new([1u8; 16]);
            // 注入可扩散脉冲，使场状态根随 Tick 演化
            eng.field
                .set_interior(rynvek_field::FieldChannelId::Nutrient, 32, 32, 8000);
            eng.field
                .set_interior(rynvek_field::FieldChannelId::Energy, 40, 40, 6000);
            let mut root_at_1 = [0u8; 32];
            let mut root_at_mid = [0u8; 32];
            let mut root_final = [0u8; 32];
            for i in 1..=N {
                let m = eng.tick().unwrap();
                if i == 1 {
                    root_at_1 = m.state_root_hash;
                }
                if i == N / 2 {
                    root_at_mid = m.state_root_hash;
                }
                if i == N {
                    root_final = m.state_root_hash;
                }
            }
            (
                eng.phase_exec_counts,
                root_at_1,
                root_at_mid,
                root_final,
                eng.clock.tick,
            )
        };

        let (counts_a, r1a, rma, rfa, tick_a) = run();
        let (_counts_b, r1b, rmb, rfb, tick_b) = run();
        assert_eq!(tick_a, N);
        assert_eq!(tick_b, N);
        assert!(counts_a.iter().all(|&c| c == N));
        assert_eq!(r1a, r1b, "Tick 1 状态根必须位级一致");
        assert_eq!(rma, rmb, "Tick 5000 状态根必须位级一致");
        assert_eq!(rfa, rfb, "Tick 10000 状态根必须位级一致");
        assert_ne!(r1a, rfa, "场扩散应使状态根随时间演化");
    }
}
