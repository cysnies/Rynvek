// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-core
//!
//! Rynvek 的 Tier 0 内核基础类型库（K0 层），为所有上层 crate 提供：
//!
//! - [`constants`]：全局冻结的时钟与空间常数；
//! - [`coord`]：统一权威坐标 `WorldPosition2D`；
//! - [`error`]：跨语言统一错误码 `RynvekErrorCode` 与恢复策略；
//! - [`fixed`]：确定性定点数 `Q32_32` / `Q8_8`；
//! - [`hash`]：BLAKE3-256 内容哈希 `Blake3Hash`；
//! - [`rng`]：无状态计数器式确定性随机数 `Philox4x32-10`；
//! - [`units`]：编译期物理量纲 `Quantity<D>`；
//!
//! 本 crate 不依赖任何内部业务 crate，且禁止 `unsafe` 代码。

#![deny(unsafe_code)]

pub mod constants;
pub mod coord;
pub mod error;
pub mod fixed;
pub mod hash;
pub mod rng;
pub mod units;
