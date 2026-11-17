// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 崩溃恢复状态机（D322–D327 + 审计报告 §4.3，四步）：
//!
//! 1. [`CrashRecoveryState::LocateLatestValidSnapshot`]（含 `.rsnap` 文件包络与状态根校验）
//! 2. [`CrashRecoveryState::OpenJournalAfterSnapshot`]
//! 3. [`CrashRecoveryState::ReplayFramesUntilTornWrite`]
//! 4. [`CrashRecoveryState::RebuildIndicesAndVerifyStateRoot`]（含重放回调与状态根一致性核验）

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

use crate::frame::JournalRecord;
use crate::manifest::CommitManifest;
use crate::reader::JournalReader;

/// 恢复状态机当前步骤
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashRecoveryState {
    LocateLatestValidSnapshot,
    OpenJournalAfterSnapshot,
    ReplayFramesUntilTornWrite,
    RebuildIndicesAndVerifyStateRoot,
    Completed,
    Failed,
}

/// 恢复输入：快照路径（可选）与 journal 路径
#[derive(Clone, Debug)]
pub struct RecoveryInput {
    pub snapshot_path: Option<PathBuf>,
    pub journal_path: PathBuf,
    /// 快照中记录的状态根；若无快照则为全零，仅依赖 journal 首帧的 parent
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
    pub torn_write_detected: bool,
    pub final_state_root: [u8; 32],
    pub final_tick: u64,
    pub final_revision: u64,
}

/// 四步崩溃恢复状态机
#[derive(Debug)]
pub struct CrashRecoveryStateMachine {
    state: CrashRecoveryState,
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
        }
    }

    pub fn state(&self) -> CrashRecoveryState {
        self.state
    }

    /// 执行完整恢复流程
    pub fn run(&mut self, input: &RecoveryInput) -> Result<RecoveryOutput, RynvekErrorCode> {
        self.run_with_replayer(input, |_| Ok(None))
    }

    /// 执行带确定性状态机重放回调的四步恢复流程
    pub fn run_with_replayer(
        &mut self,
        input: &RecoveryInput,
        mut replayer: impl FnMut(&JournalRecord) -> Result<Option<[u8; 32]>, RynvekErrorCode>,
    ) -> Result<RecoveryOutput, RynvekErrorCode> {
        // 步骤 1：定位并核验快照包络
        self.state = CrashRecoveryState::LocateLatestValidSnapshot;
        if let Some(path) = &input.snapshot_path {
            if !Path::new(path).exists() {
                self.state = CrashRecoveryState::Failed;
                return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
            }
            if let Err(e) = verify_snapshot_file_envelope(path, input.snapshot_tick, input.snapshot_state_root) {
                self.state = CrashRecoveryState::Failed;
                return Err(e);
            }
        }

        // 步骤 2：打开 journal
        self.state = CrashRecoveryState::OpenJournalAfterSnapshot;
        if !input.journal_path.exists() {
            self.state = CrashRecoveryState::Failed;
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }

        // 步骤 3：重放直至撕裂写
        self.state = CrashRecoveryState::ReplayFramesUntilTornWrite;
        let read = JournalReader::read_path(&input.journal_path)?;

        // 步骤 4：校验父哈希链、执行重放回调并验证最终状态根
        self.state = CrashRecoveryState::RebuildIndicesAndVerifyStateRoot;
        let mut expected_parent = input.snapshot_state_root;
        let mut commits = Vec::new();
        let mut replayed_records = Vec::new();

        for record in read.records {
            if record.tick() <= input.snapshot_tick && record.world_revision() <= input.snapshot_revision {
                continue;
            }
            let maybe_replayed_root = match replayer(&record) {
                Ok(r) => r,
                Err(e) => {
                    self.state = CrashRecoveryState::Failed;
                    return Err(e);
                }
            };
            if let JournalRecord::TickCommit(commit) | JournalRecord::Checkpoint(commit) = &record {
                if commit.parent_commit_hash != expected_parent {
                    self.state = CrashRecoveryState::Failed;
                    return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                }
                if let Some(actual_root) = maybe_replayed_root {
                    if actual_root != commit.state_root_hash {
                        self.state = CrashRecoveryState::Failed;
                        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                    }
                }
                expected_parent = commit.state_root_hash;
                commits.push(*commit);
            }
            replayed_records.push(record);
        }

        let (final_tick, final_revision, final_state_root) = if let Some(last) = commits.last() {
            (last.tick, last.world_revision, last.state_root_hash)
        } else {
            (input.snapshot_tick, input.snapshot_revision, input.snapshot_state_root)
        };

        self.state = CrashRecoveryState::Completed;
        Ok(RecoveryOutput {
            final_state: self.state,
            replayed_commits: commits,
            replayed_records,
            torn_write_detected: read.torn_write,
            final_state_root,
            final_tick,
            final_revision,
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
        .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
    if &prefix[0..4] != b"RYSS" {
        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
    }
    let version = u16::from_le_bytes([prefix[4], prefix[5]]);
    if version != 1 && version != 2 {
        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
    }
    let tick = u64::from_le_bytes(prefix[32..40].try_into().unwrap());
    let payload_bytes = u64::from_le_bytes(prefix[48..56].try_into().unwrap());
    let file_state_root: [u8; 32] = prefix[64..96].try_into().unwrap();
    if tick != expected_tick || file_state_root != expected_state_root {
        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
    }
    let meta = f.metadata().map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
    if meta.len() < 96 + payload_bytes {
        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
    }
    Ok(())
}

/// 便于测试：把状态根包装为 Blake3Hash
pub fn state_root_hash(bytes: [u8; 32]) -> Blake3Hash {
    Blake3Hash(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::CommitManifest;
    use crate::writer::JournalWriter;
    use tempfile::TempDir;

    #[test]
    fn recovers_through_torn_write_with_consistent_root() {
        let dir = TempDir::new().unwrap();
        let journal = dir.path().join("world.rjournal");
        let world = [7u8; 16];
        let mut writer = JournalWriter::create(&journal, world, 0, 0, 0).unwrap();

        let mut parent = [0u8; 32];
        let mut manifests = Vec::new();
        for rev in 1..=5u64 {
            let entity = Blake3Hash::of(&rev.to_le_bytes()).0;
            let m = CommitManifest::new(
                world,
                rev * 10,
                rev,
                parent,
                crate::manifest::CommitSubRoots {
                    entity_root_hash: entity,
                    ..Default::default()
                },
            );
            writer.append_commit(&m).unwrap();
            parent = m.state_root_hash;
            manifests.push(m);
        }
        // 模拟断电：追加半帧
        writer.append_torn_bytes(&[1, 2, 3, 4, 5]).unwrap();

        let mut machine = CrashRecoveryStateMachine::new();
        let output = machine
            .run(&RecoveryInput {
                snapshot_path: None,
                journal_path: journal,
                snapshot_state_root: [0; 32],
                snapshot_tick: 0,
                snapshot_revision: 0,
            })
            .unwrap();
        assert_eq!(output.final_state, CrashRecoveryState::Completed);
        assert!(output.torn_write_detected);
        assert_eq!(output.replayed_commits.len(), 5);
        assert_eq!(output.final_state_root, manifests[4].state_root_hash);
        assert_eq!(output.final_tick, 50);
    }
}
