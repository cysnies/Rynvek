// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

//! # rynvek-protocol
//!
//! Rynvek 的 Tier 0 公开跨语言协议底座（与 TypeScript 包 `@rynvek/protocol` 逐字节对齐）：
//!
//! - [`wire`]：16 字节实时流帧头 `WireFrameHeader` 与 `WireChannelId`；
//!
//! 所有二进制布局一律采用 Little-Endian 字节序。本 crate 不依赖任何内部 crate，且禁止 `unsafe` 代码。

#![deny(unsafe_code)]

pub mod wire;
