// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 崩溃恢复状态机（D322–D327，四步）。
//!
//! 1. [`CrashRecoveryState::LocateLatestValidSnapshot`]
//! 2. [`CrashRecoveryState::OpenJournalAfterSnapshot`]
//! 3. [`CrashRecoveryState::ReplayFramesUntilTornWrite`]
//! 4. [`CrashRecoveryState::RebuildIndicesAndVerifyStateRoot`]

use std::path::{Path, PathBuf};

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

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
        // 步骤 1：定位快照（M1：由调用方提供路径；此处仅验证可选文件存在）
        self.state = CrashRecoveryState::LocateLatestValidSnapshot;
        if let Some(path) = &input.snapshot_path {
            if !Path::new(path).exists() {
                self.state = CrashRecoveryState::Failed;
                return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
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
        let mut commits = read.commits;
        // 丢弃不晚于快照的帧（快照已覆盖）
        commits.retain(|c| c.tick > input.snapshot_tick || c.world_revision > input.snapshot_revision);

        // 步骤 4：校验父哈希链并得到最终状态根
        self.state = CrashRecoveryState::RebuildIndicesAndVerifyStateRoot;
        let mut expected_parent = input.snapshot_state_root;
        for commit in &commits {
            if commit.parent_commit_hash != expected_parent {
                self.state = CrashRecoveryState::Failed;
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
            expected_parent = commit.state_root_hash;
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
            torn_write_detected: read.torn_write,
            final_state_root,
            final_tick,
            final_revision,
        })
    }
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
