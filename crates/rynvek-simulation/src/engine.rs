// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 16 阶段 Tick 推进引擎（规范 03）。
//!
//! M2 范围：在无外部命令/神经/RTS 的最小世界中，完整走过 16 个阶段，
//! 串联 `rynvek-field` 与 `rynvek-spatial`，并保证 `10_000` Tick 后状态根可复现（零误差）。

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

/// 最小可复现仿真世界状态（验收用）
#[derive(Debug)]
pub struct SimulationEngine {
    pub clock: MultiRateClock,
    pub world_id: [u8; 16],
    pub field: FieldWorld,
    /// 实体 Q32.32 局部坐标
    pub positions: Vec<(u64, u64)>,
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
}

impl Default for SimulationEngine {
    fn default() -> Self {
        Self::new([0u8; 16])
    }
}

impl SimulationEngine {
    pub fn new(world_id: [u8; 16]) -> Self {
        // 放置若干实体供空间索引阶段使用（均在规范坐标范围内）
        let positions = vec![
            (2u64 << 32, 2u64 << 32),
            (4u64 << 32, 2u64 << 32),
            (10u64 << 32, 10u64 << 32),
            (20u64 << 32, 30u64 << 32),
        ];
        Self {
            clock: MultiRateClock::new(),
            world_id,
            field: FieldWorld::new(),
            positions,
            spatial: SpatialCsrIndex::empty(),
            last_manifest: None,
            parent_commit_hash: [0u8; 32],
            biomass: 500,
            flux: 200,
            neurogel: 50,
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
        }
    }

    /// 推进一个完整 Tick（16 阶段）；成功则更新 `last_manifest`
    pub fn tick(&mut self) -> Result<TickCommitManifest, SimulationError> {
        // 帧计量清零
        self.frame_biomass_harvested = 0;
        self.frame_nutrient_consumed = 0;
        self.frame_carcass_reclaimed = 0;

        for phase in CanonicalPhaseId::ALL {
            self.run_phase(phase)?;
            self.phase_exec_counts[phase.as_u16() as usize] += 1;
        }

        let manifest = self.last_manifest.clone().expect("Commit 阶段必须写入 manifest");
        Ok(manifest)
    }

    fn run_phase(&mut self, phase: CanonicalPhaseId) -> Result<(), SimulationError> {
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
                    .dispatch_1d(KernelId("spatial.build_uniform_grid_csr"), self.positions.len() as u32)
                    .map_err(|e| SimulationError::Dispatch(format!("{e:?}")))?;
                self.spatial =
                    SpatialCsrIndex::build_from_local_q32(&self.positions).map_err(SimulationError::Spatial)?;
                Ok(())
            }
            CanonicalPhaseId::EnvironmentAndFieldSolve => {
                self.backend
                    .dispatch_1d(KernelId("field.reaction_diffusion_q8"), 1)
                    .map_err(|e| SimulationError::Dispatch(format!("{e:?}")))?;
                self.field.step_all().map_err(SimulationError::Field)?;
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
                // 静止验收世界：坐标不变，仅校验仍在规范范围内
                for &(lx, ly) in &self.positions {
                    if lx >= COORD_CANONICAL_MAX || ly >= COORD_CANONICAL_MAX {
                        return Err(SimulationError::Invariant(
                            RynvekErrorCode::CoordinateOutOfCanonicalRange,
                        ));
                    }
                }
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
                for &(lx, ly) in &self.positions {
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
                let entity_root = self.hash_entities();
                let field_root = self.field.state_root().0;
                let neural_root = [0u8; 32];
                let economy_root = self.hash_economy();
                let rng_root = [0u8; 32];
                let state_root = TickCommitManifest::compute_state_root(
                    &entity_root,
                    &field_root,
                    &neural_root,
                    &economy_root,
                    &rng_root,
                    &self.parent_commit_hash,
                );
                let manifest = TickCommitManifest {
                    world_id: self.world_id,
                    tick: self.clock.tick,
                    world_revision: self.clock.world_revision,
                    parent_commit_hash: self.parent_commit_hash,
                    entity_root_hash: entity_root,
                    field_root_hash: field_root,
                    neural_root_hash: neural_root,
                    economy_root_hash: economy_root,
                    rng_root_hash: rng_root,
                    state_root_hash: state_root,
                };
                self.parent_commit_hash = state_root;
                self.last_manifest = Some(manifest);
                Ok(())
            }
        }
    }

    fn hash_entities(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        for &(lx, ly) in &self.positions {
            h.update(&lx.to_le_bytes());
            h.update(&ly.to_le_bytes());
        }
        // CSR 紧凑索引已含完整空间划分信息；不必每 Tick 哈希 16385 个 row_offsets
        h.update(&(self.spatial.sorted_dense_indices.len() as u64).to_le_bytes());
        for d in &self.spatial.sorted_dense_indices {
            h.update(&d.to_le_bytes());
        }
        *h.finalize().as_bytes()
    }

    fn hash_economy(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.biomass.to_le_bytes());
        h.update(&self.flux.to_le_bytes());
        h.update(&self.neurogel.to_le_bytes());
        *h.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_phases_run_each_tick() {
        let mut eng = SimulationEngine::new([7u8; 16]);
        eng.tick().unwrap();
        assert!(eng.phase_exec_counts.iter().all(|&c| c == 1));
        assert_eq!(eng.clock.tick, 1);
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
