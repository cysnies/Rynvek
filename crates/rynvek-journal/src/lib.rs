// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-journal
//!
//! `.rjournal`（魔数 `RYJL`）预写日志：64 字节文件头 + 7 类变长 WAL 帧。
//!
//! - [`header`]：文件头布局；
//! - [`manifest`]：与规范 `TickCommitManifest` 对齐的提交清单；
//! - [`frame`]：单帧编解码（长度前缀 + BLAKE3 校验 + 7 种 `FrameKind` 类型化记录）；
//! - [`writer`] / [`reader`]：带 `sync_data` 持久化屏障的追加写与顺序读；
//! - [`recovery`]：四步 [`CrashRecoveryStateMachine`]（含快照包络核验与重放回调验证）。

#![deny(unsafe_code)]

pub mod frame;
pub mod header;
pub mod manifest;
pub mod reader;
pub mod recovery;
pub mod writer;

pub use frame::{
    FrameKind, JournalCommandRecord, JournalEventRecord, JournalFrame, JournalFrameHeader, JournalMutationRecord,
    JournalPatchRecord, JournalRecord, JournalSnapshotRefRecord,
};
pub use header::{JOURNAL_HEADER_LEN, JOURNAL_MAGIC, JOURNAL_VERSION, JournalFileHeader};
pub use manifest::{CanonicalDomains, CommitManifest, CommitSubRoots};
pub use reader::{JournalReadResult, JournalReader};
pub use recovery::{CrashRecoveryState, CrashRecoveryStateMachine, RecoveryInput, RecoveryOutput};
pub use writer::JournalWriter;
