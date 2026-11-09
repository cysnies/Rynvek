// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-snapshot
//!
//! `.rsnap`（魔数 `RYSS`）分块快照：64 字节文件头 + Zstd 压缩分块 + `NEUR` 稀疏神经块。

#![deny(unsafe_code)]

pub mod header;
pub mod neur;
pub mod reader;
pub mod writer;

pub use header::{SNAPSHOT_HEADER_LEN, SNAPSHOT_MAGIC, SNAPSHOT_VERSION, SnapshotFileHeader};
pub use neur::{NEUR_TAG, NeurChunk, NeurNeuronRow};
pub use reader::{SnapshotReadResult, SnapshotReader};
pub use writer::{SnapshotWriteOutput, SnapshotWriter};
