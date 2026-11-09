// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-journal
//!
//! `.rjournal`（魔数 `RYJL`）预写日志：64 字节文件头 + 变长提交帧。
//!
//! - [`header`]：文件头布局；
//! - [`manifest`]：与规范 `TickCommitManifest` 对齐的提交清单；
//! - [`frame`]：单帧编解码（长度前缀 + BLAKE3 校验）；
//! - [`writer`] / [`reader`]：追加写与顺序读；
//! - [`recovery`]：四步 [`CrashRecoveryStateMachine`]。

#![deny(unsafe_code)]

pub mod frame;
pub mod header;
pub mod manifest;
pub mod reader;
pub mod recovery;
pub mod writer;

pub use frame::{JournalFrame, JournalFrameHeader};
pub use header::{JOURNAL_HEADER_LEN, JOURNAL_MAGIC, JOURNAL_VERSION, JournalFileHeader};
pub use manifest::{CommitManifest, CommitSubRoots};
pub use reader::JournalReader;
pub use recovery::{CrashRecoveryState, CrashRecoveryStateMachine, RecoveryOutput};
pub use writer::JournalWriter;
