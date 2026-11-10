// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-neural
//!
//! Neural Soup 原生运行时（规范 05）：
//!
//! - [`runtime`]：规范冻结的 `NeuronModelKind` / `TopologyStorageClass` 与确定性 Q8.8 LIF + 四因子 STDP 核；
//! - [`models`]：其余三种神经元模型（`GatedMLP` / `Izhikevich` / `CTRNN`）的确定性定点实现及 `NEUR` 快照双向映射；
//! - [`storage`]：三种神经图存储布局（`DenseTiledMatrix` / `BlockSparseCSR` / `PagedDynamicAdjacency`）；
//! - [`brain`]：LIF 脑实例状态与 `NEUR` 快照分块的双向映射；
//! - [`population`]：大规模神经生态群体向量化调度、鲍德温同化与 Kuramoto 相位一致性度量。

#![deny(unsafe_code)]

pub mod brain;
pub mod models;
pub mod population;
pub mod runtime;
pub mod storage;

pub use brain::{LIF_INPUTS, LIF_OUTPUTS, LifBrain};
pub use models::{CtrnnBrain, GatedMlpBrain, IzhikevichBrain};
pub use population::{NeuralPopulationRuntime, PopulationBrainSlot, PopulationMember};
pub use runtime::{
    LIF_LEAK_BETA_Q8, LIF_REFRACTORY_TICKS, NeuronModelKind, TopologyStorageClass, lif_membrane_candidate_q8,
    lif_theta_eff_q8, step_lif_brain_deterministic,
};
pub use rynvek_snapshot::{NeurChunk, NeurNeuronRow};
pub use storage::{
    BlockSparseCsr, DaleSign, DenseTiledMatrix, NeuralGraphStorage, PagedDynamicAdjacency, SynapticMatVec,
    select_storage_class, validate_dale_law,
};
