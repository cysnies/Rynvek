// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-field
//!
//! 连续环境场运行时（规范 06）：
//!
//! - [`solver`]：260×260 Halo 五点拉普拉斯 Q8.8 反应-扩散算子；
//! - [`cfl`]：CFL 自动子步拆分；
//! - [`channels`]：五通道标准场参数与多通道 Tile；
//! - [`lenia`]：确定性 Q8.8 Lenia 连续元胞自动机核；
//! - [`world`]：单 Chunk 五通道场世界与守恒/状态根工具。

#![deny(unsafe_code)]

pub mod cfl;
pub mod channels;
pub mod lenia;
pub mod solver;
pub mod world;

pub use cfl::{cfl_substeps_for_diffusion, solve_reaction_diffusion_tile_q8_cfl};
pub use channels::{FieldChannelId, FieldChannelParams, FieldTileQ8, STANDARD_FIELD_CHANNELS};
pub use lenia::{LeniaKernel, LeniaParams};
pub use solver::{HALO, STRIDE, TILE_DIM, solve_reaction_diffusion_tile_q8};
pub use world::FieldWorld;
