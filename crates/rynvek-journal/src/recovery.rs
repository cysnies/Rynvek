// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 崩溃恢复状态机（规范 10.2 `CrashRecoveryStateMachine` / D197 + 审计报告 §4.3）：
//!
//! 1. `LocateLatestValidSnapshot`：定位最新快照文件并读取文件头与 32B 状态根做真值校验；
//! 2. `VerifySnapshotChunks`：核验快照文件头的魔数、版本号、声明的载荷长度与状态根一致性；
//! 3. `ScanJournalTailAndTruncateTornWrite`：扫描 `.rjournal` 尾部，若发现撕裂写则安全截断并记录；
//! 4. `ReplayJournalFramesAndVerifyStateRoot`：逐帧重放 `tick > snapshot.tick` 的 WAL 记录（含命令、事件、结构变更与提交帧），验证 `parent_commit_hash` 链连续性及重放后世界状态根一致性。

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use rynvek_core::error::RynvekErrorCode;

use crate::frame::JournalRecord;
use crate::manifest::CommitManifest;
use crate::reader::JournalReader;

/// 状态机的 4 个规范阶段 + 完成态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashRecoveryState {
    LocateLatestValidSnapshot,
    VerifySnapshotChunks,
    ScanJournalTailAndTruncateTornWrite,
    ReplayJournalFramesAndVerifyStateRoot,
    Completed,
}

/// 恢复输入
#[derive(Clone, Debug)]
pub struct RecoveryInput {
    pub snapshot_path: Option<PathBuf>,
    pub journal_path: PathBuf,
    /// 快照已知状态根（无快照时为全 0）
    pub snapshot_state_root: [u8; 32],
    pub snapshot_tick: u64,
    pub snapshot_revision: u64,
}

/// 恢复输出
#[derive(Clone, Debug)]
pub struct RecoveryOutput {
    pub final_state: CrashRecoveryState,
    pub replayed_commits: Vec<CommitManifest>,
    pub replayed_records: Vec<JournalRecord>,
    pub final_state_root: [u8; 32],
    pub final_tick: u64,
    pub final_revision: u64,
    pub torn_write_detected: bool,
    pub state_trace: Vec<CrashRecoveryState>,
}

/// 四阶段崩溃恢复状态机
#[derive(Debug)]
pub struct CrashRecoveryStateMachine {
    state: CrashRecoveryState,
    trace: Vec<CrashRecoveryState>,
}

impl Default for CrashRecoveryStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl CrashRecoveryStateMachine {
    pub fn new() -> Self {
        Self {
            state: CrashRecoveryState::LocateLatestValidSnapshot,
            trace: vec![CrashRecoveryState::LocateLatestValidSnapshot],
        }
    }

    pub const fn state(&self) -> CrashRecoveryState {
        self.state
    }

    fn transition(&mut self, next: CrashRecoveryState) {
        self.state = next;
        self.trace.push(next);
    }

    /// 执行标准四阶段崩溃恢复（验证快照头与提交哈希链）
    pub fn run(&mut self, input: &RecoveryInput) -> Result<RecoveryOutput, RynvekErrorCode> {
        self.run_with_replayer(input, |_| Ok(None))
    }

    /// 执行带确定性状态机重放回调的四阶段崩溃恢复：
    /// `replayer` 对每条 `tick > snapshot_tick` 的 `JournalRecord` 被调用；若返回 `Some(replayed_state_root)`，
    /// 则在 `TickCommit` / `Checkpoint` 边界强制与清单中的 `state_root_hash` 比对，不一致立即报 `Blake3ChecksumMismatch`。
    pub fn run_with_replayer(
        &mut self,
        input: &RecoveryInput,
        mut replayer: impl FnMut(&JournalRecord) -> Result<Option<[u8; 32]>, RynvekErrorCode>,
    ) -> Result<RecoveryOutput, RynvekErrorCode> {
        // 阶段 1：LocateLatestValidSnapshot
        if let Some(ref snap) = input.snapshot_path {
            if !snap.exists() {
                return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
            }
        }
        // 阶段 2：VerifySnapshotChunks（校验 .rsnap 文件头魔数、版本、声明载荷长度与 32B 状态根）
        self.transition(CrashRecoveryState::VerifySnapshotChunks);
        if let Some(ref snap) = input.snapshot_path {
            verify_snapshot_file_envelope(snap, input.snapshot_tick, input.snapshot_state_root)?;
        }

        // 阶段 3：ScanJournalTailAndTruncateTornWrite
        self.transition(CrashRecoveryState::ScanJournalTailAndTruncateTornWrite);
        let read = JournalReader::read_path(&input.journal_path)?;

        // 阶段 4：ReplayJournalFramesAndVerifyStateRoot
        self.transition(CrashRecoveryState::ReplayJournalFramesAndVerifyStateRoot);
        let mut expected_parent = input.snapshot_state_root;
        let mut final_tick = input.snapshot_tick;
        let mut final_rev = input.snapshot_revision;
        let mut replayed_commits = Vec::new();
        let mut replayed_records = Vec::new();

        for record in read.records {
            if record.tick() <= input.snapshot_tick {
                continue;
            }
            let maybe_replayed_root = replayer(&record)?;
            if let JournalRecord::TickCommit(commit) | JournalRecord::Checkpoint(commit) = &record {
                if commit.parent_commit_hash != expected_parent {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                let recomputed = CommitManifest::compute_state_root(
                    commit.entity_root_hash,
                    commit.field_root_hash,
                    commit.neural_root_hash,
                    commit.economy_root_hash,
                    commit.rng_root_hash,
                    commit.parent_commit_hash,
                );
                if recomputed != commit.state_root_hash {
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                if let Some(actual_root) = maybe_replayed_root {
                    if actual_root != commit.state_root_hash {
                        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                    }
                }
                expected_parent = commit.state_root_hash;
                final_tick = commit.tick;
                final_rev = commit.world_revision;
                replayed_commits.push(*commit);
            }
            replayed_records.push(record);
        }

        self.transition(CrashRecoveryState::Completed);
        Ok(RecoveryOutput {
            final_state: self.state,
            replayed_commits,
            replayed_records,
            final_state_root: expected_parent,
            final_tick,
            final_revision: final_rev,
            torn_write_detected: read.torn_write,
            state_trace: self.trace.clone(),
        })
    }
}

/// 核验 `.rsnap` 快照文件头魔数、版本号、32B 状态根与载荷完整性
fn verify_snapshot_file_envelope(
    path: &PathBuf,
    expected_tick: u64,
    expected_state_root: [u8; 32],
) -> Result<(), RynvekErrorCode> {
    let mut f = File::open(path).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
    let mut prefix = [0u8; 96];
    f.read_exact(&mut prefix)
        .map_err(|_| RynvekErrorCode::SnapshotChunkChecksumFailed)?;
    if &prefix[0..4] != b"RYSS" {
        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
    }
    let version = u16::from_le_bytes([prefix[4], prefix[5]]);
    if version != 1 {
        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
    }
    let tick = u64::from_le_bytes(prefix[32..40].try_into().unwrap());
    let payload_bytes = u64::from_le_bytes(prefix[48..56].try_into().unwrap());
    let file_state_root: [u8; 32] = prefix[64..96].try_into().unwrap();
    if tick != expected_tick || file_state_root != expected_state_root {
        return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
    }
    let meta = f
        .metadata()
        .map_err(|_| RynvekErrorCode::SnapshotChunkChecksumFailed)?;
    if meta.len() < 96 + payload_bytes {
        return Err(RynvekErrorCode::SnapshotChunkChecksumFailed);
    }
    Ok(())
}
