// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! [`StorageEngine`]：世界元数据与提交链的抽象。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

use crate::blob::{BlobId, BlobStore, MemoryBlobStore};

/// 世界元数据（与 Postgres `worlds` / SQLite `local_worlds` 对齐的最小子集）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldMeta {
    pub world_id: [u8; 16],
    pub title: String,
    pub world_profile: String,
    pub game_mode: String,
    pub execution_profile: String,
    pub latest_tick: u64,
    pub latest_revision: u64,
    pub latest_state_root: Blake3Hash,
}

/// 一次已提交的世界修订记录
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldCommitRecord {
    pub world_id: [u8; 16],
    pub world_revision: u64,
    pub tick: u64,
    pub parent_revision: Option<u64>,
    pub state_root: Blake3Hash,
    pub journal_segment_cid: BlobId,
    pub snapshot_cid: Option<BlobId>,
}

/// 存储引擎 Trait
pub trait StorageEngine: Send + Sync {
    /// 关联的 Blob CAS
    fn blobs(&self) -> &dyn BlobStore;

    /// 创建或覆盖世界元数据
    fn upsert_world(&self, meta: WorldMeta) -> Result<(), RynvekErrorCode>;

    /// 读取世界元数据
    fn get_world(&self, world_id: &[u8; 16]) -> Result<WorldMeta, RynvekErrorCode>;

    /// 追加一条提交记录，并更新世界的 latest_* 字段
    fn append_commit(&self, commit: WorldCommitRecord) -> Result<(), RynvekErrorCode>;

    /// 按修订号读取提交
    fn get_commit(&self, world_id: &[u8; 16], revision: u64) -> Result<WorldCommitRecord, RynvekErrorCode>;

    /// 列出某世界全部提交（按 revision 升序）
    fn list_commits(&self, world_id: &[u8; 16]) -> Result<Vec<WorldCommitRecord>, RynvekErrorCode>;
}

/// 纯内存存储引擎（不落盘）
type WorldCommitIndex = BTreeMap<[u8; 16], BTreeMap<u64, WorldCommitRecord>>;

#[derive(Clone, Debug, Default)]
pub struct MemoryStorageEngine {
    blobs: MemoryBlobStore,
    worlds: Arc<Mutex<BTreeMap<[u8; 16], WorldMeta>>>,
    commits: Arc<Mutex<WorldCommitIndex>>,
}

impl MemoryStorageEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StorageEngine for MemoryStorageEngine {
    fn blobs(&self) -> &dyn BlobStore {
        &self.blobs
    }

    fn upsert_world(&self, meta: WorldMeta) -> Result<(), RynvekErrorCode> {
        let mut worlds = self
            .worlds
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        worlds.insert(meta.world_id, meta);
        Ok(())
    }

    fn get_world(&self, world_id: &[u8; 16]) -> Result<WorldMeta, RynvekErrorCode> {
        let worlds = self
            .worlds
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        worlds
            .get(world_id)
            .cloned()
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)
    }

    fn append_commit(&self, commit: WorldCommitRecord) -> Result<(), RynvekErrorCode> {
        {
            let mut commits = self
                .commits
                .lock()
                .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
            let chain = commits.entry(commit.world_id).or_default();
            if chain.contains_key(&commit.world_revision) {
                return Err(RynvekErrorCode::StructuralMutationConflict);
            }
            chain.insert(commit.world_revision, commit.clone());
        }
        let mut worlds = self
            .worlds
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        let world = worlds
            .get_mut(&commit.world_id)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        world.latest_tick = commit.tick;
        world.latest_revision = commit.world_revision;
        world.latest_state_root = commit.state_root;
        Ok(())
    }

    fn get_commit(&self, world_id: &[u8; 16], revision: u64) -> Result<WorldCommitRecord, RynvekErrorCode> {
        let commits = self
            .commits
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        commits
            .get(world_id)
            .and_then(|c| c.get(&revision))
            .cloned()
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)
    }

    fn list_commits(&self, world_id: &[u8; 16]) -> Result<Vec<WorldCommitRecord>, RynvekErrorCode> {
        let commits = self
            .commits
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        Ok(commits
            .get(world_id)
            .map(|c| c.values().cloned().collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_engine_tracks_commits() {
        let engine = MemoryStorageEngine::new();
        let world_id = [1u8; 16];
        let root = Blake3Hash::of(b"root");
        engine
            .upsert_world(WorldMeta {
                world_id,
                title: "t".into(),
                world_profile: "RTS_Standard".into(),
                game_mode: "PvP_Ranked".into(),
                execution_profile: "D0_Strict".into(),
                latest_tick: 0,
                latest_revision: 0,
                latest_state_root: Blake3Hash::of(b""),
            })
            .unwrap();
        let journal = engine.blobs().put(b"journal").unwrap();
        engine
            .append_commit(WorldCommitRecord {
                world_id,
                world_revision: 1,
                tick: 30,
                parent_revision: Some(0),
                state_root: root,
                journal_segment_cid: journal,
                snapshot_cid: None,
            })
            .unwrap();
        assert_eq!(engine.get_world(&world_id).unwrap().latest_tick, 30);
        assert_eq!(engine.list_commits(&world_id).unwrap().len(), 1);
    }
}
