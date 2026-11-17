// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 活动宿主的私有 Tick 日志与唯一提交点；回放复用同一个 ArenaRuntime，不再另建模拟器。

use crate::{
    error::{ApiError, ApiResult},
    store::{CommitRow, Store},
};
use rynvek_core::{Blake3Hash, RynvekErrorCode};
use rynvek_protocol::WireUuid;
use rynvek_rts::commands::RtsCommand;
use rynvek_rts::{ArenaRuntime, FactionId};
use rynvek_storage::BlobStore;
use rynvek_world::{PatchApplyReport, WorldPatch};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TickInput {
    Command {
        faction: u8,
        command: RtsCommand,
        principal_id: u64,
        idempotency_key: String,
    },
    Patch {
        patch: WorldPatch,
        report: PatchApplyReport,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickRecord {
    pub format: String,
    pub base_revision: u64,
    pub base_root: [u8; 32],
    pub target_tick: u64,
    pub tick: u64,
    pub revision: u64,
    pub state_root: [u8; 32],
    pub inputs: Vec<TickInput>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchApplied {
    pub revision: u64,
    pub tick: u64,
    pub effective_tick: u64,
    pub state_root: [u8; 32],
    pub report: PatchApplyReport,
    pub patch_cid: [u8; 32],
}

impl TickRecord {
    pub fn patch_receipt(&self, input: &WorldPatch) -> ApiResult<Option<PatchApplied>> {
        for event in &self.inputs {
            if let TickInput::Patch { patch, report } = event {
                if patch.patch_id == input.patch_id {
                    if patch != input {
                        return Err(conflict("同一 patch_id 不能替换载荷"));
                    }
                    return Ok(Some(PatchApplied {
                        revision: self.revision,
                        tick: self.tick,
                        effective_tick: self.target_tick,
                        state_root: self.state_root,
                        report: report.clone(),
                        patch_cid: Blake3Hash::of(&serde_json::to_vec(patch).expect("可序列化")).0,
                    }));
                }
            }
        }
        Ok(None)
    }

    pub fn replay(&self, runtime: &ArenaRuntime) -> Result<ArenaRuntime, RynvekErrorCode> {
        if self.format != "rynvek-tick/1"
            || runtime.revision() != self.base_revision
            || runtime.state_root() != self.base_root
            || runtime.tick.checked_add(1) != Some(self.target_tick)
        {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let mut candidate = runtime.candidate_tick()?;
        let mut patched = false;
        for input in &self.inputs {
            match input {
                TickInput::Command { faction, command, .. } => candidate.command(FactionId(*faction), command)?,
                TickInput::Patch { patch, report } => {
                    if patched
                        || patch.base_authoring_revision != self.base_revision
                        || candidate.outcome.is_some()
                        || report.requires_fork
                    {
                        return Err(RynvekErrorCode::StructuralMutationConflict);
                    }
                    patched = true;
                    let tick = candidate.tick;
                    let actual = candidate.transact_at(tick, |w| patch.apply_at_boundary(w, self.target_tick))?;
                    if &actual != report {
                        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
                    }
                }
            }
        }
        candidate.finish_tick()?;
        if candidate.tick != self.tick
            || candidate.revision() != self.revision
            || candidate.state_root() != self.state_root
        {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok(candidate)
    }
}

pub fn conflict(message: &str) -> ApiError {
    ApiError::new(RynvekErrorCode::StructuralMutationConflict, message)
}

/// 与 Actor 分离持久化能力，避免宿主通过 AppState 循环引用整个注册表。
#[derive(Clone)]
pub struct TickCommitter {
    pub store: Store,
    pub snapshots: Arc<dyn BlobStore>,
    pub world_id: WireUuid,
    pub fencing_token: u64,
}

impl std::fmt::Debug for TickCommitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TickCommitter")
            .field("world_id", &self.world_id)
            .field("fencing_token", &self.fencing_token)
            .finish_non_exhaustive()
    }
}

impl TickCommitter {
    pub async fn persist(&self, runtime: &ArenaRuntime, record: &TickRecord) -> ApiResult<()> {
        let cp = runtime
            .checkpoint()
            .map_err(|e| ApiError::new(e, "编码 Tick 检查点失败"))?;
        let snapshot = self.snapshots.put(&cp.bytes)?;
        let journal = self.snapshots.put(&serde_json::to_vec(record).expect("可序列化"))?;
        for input in &record.inputs {
            if let TickInput::Patch { patch, .. } = input {
                self.snapshots.put(&serde_json::to_vec(patch).expect("可序列化"))?;
            }
        }
        self.store
            .append_commit(
                CommitRow {
                    world_id: self.world_id,
                    world_revision: cp.revision,
                    tick: cp.tick,
                    parent_revision: Some(record.base_revision),
                    state_root_blake3: cp.state_root,
                    journal_segment_cid: journal.0,
                    snapshot_cid: Some(snapshot.0),
                },
                self.fencing_token,
            )
            .await
    }

    pub async fn records(&self) -> ApiResult<Vec<TickRecord>> {
        let mut records = Vec::new();
        for commit in self.store.list_commits(self.world_id).await? {
            let bytes = self.snapshots.get(&Blake3Hash(commit.journal_segment_cid))?;
            if Blake3Hash::of(&bytes).0 != commit.journal_segment_cid {
                return Err(conflict("私有日志 CID 不一致"));
            }
            if commit.world_revision == 0 {
                continue;
            }
            if let Ok(record) = serde_json::from_slice::<TickRecord>(&bytes) {
                if record.format != "rynvek-tick/1"
                    || record.revision != commit.world_revision
                    || record.state_root != commit.state_root_blake3
                    || record.tick != commit.tick
                {
                    return Err(conflict("已提交 Tick 日志与元数据不一致"));
                }
                records.push(record);
            } else if serde_json::from_slice::<WorldPatch>(&bytes).is_err() {
                return Err(conflict("未知或损坏的私有 Tick 日志"));
            }
        }
        Ok(records)
    }

    /// 幂等以已提交日志为准，重启也不会再次应用旧 patch_id。
    pub async fn patch_receipt(&self, patch: &WorldPatch) -> ApiResult<Option<PatchApplied>> {
        for record in self.records().await?.into_iter().rev() {
            if let Some(receipt) = record.patch_receipt(patch)? {
                return Ok(Some(receipt));
            }
        }
        // 旧创作提交只含补丁 JSON，没有成功回执；拒绝复用 ID 而不是重新应用。
        for commit in self.store.list_commits(self.world_id).await? {
            if commit.world_revision == 0 {
                continue;
            }
            let bytes = self.snapshots.get(&Blake3Hash(commit.journal_segment_cid))?;
            if let Ok(previous) = serde_json::from_slice::<WorldPatch>(&bytes) {
                if previous.patch_id == patch.patch_id {
                    return Err(conflict("patch_id 已用于旧创作提交，不能重复应用"));
                }
            }
        }
        Ok(None)
    }
}
