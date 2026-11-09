// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-core
//!
//! Rynvek 的 Tier 0 内核基础类型库（K0 层），为所有上层 crate 提供：
//!
//! - [`fixed`]：确定性定点数 `Q32_32` / `Q8_8`；
//!
//! 本 crate 不依赖任何内部业务 crate，且禁止 `unsafe` 代码。

#![deny(unsafe_code)]

pub mod fixed;
