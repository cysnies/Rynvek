// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! SQLite 本地存储引擎：执行规范 11.2 DDL，并实现 [`StorageEngine`]。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, params};
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

use crate::blob::{BlobId, BlobStore, MemoryBlobStore};
use crate::engine::{StorageEngine, WorldCommitRecord, WorldMeta};
use crate::migrations::SQLITE_0001_LOCAL_STORAGE;

/// 基于 SQLite 的本地存储引擎。
///
/// Blob 本体仍存放于关联的 [`MemoryBlobStore`]（或未来的文件系统 CAS）；
/// SQLite 只保存世界元数据、快照索引与 LRU 资产表（与 DDL 一致）。
#[derive(Debug)]
pub struct SqliteStorageEngine {
    path: PathBuf,
    conn: Mutex<Connection>,
    blobs: MemoryBlobStore,
}

impl SqliteStorageEngine {
    /// 打开或创建数据库文件，并确保 DDL 已应用
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RynvekErrorCode> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let engine = Self {
            path,
            conn: Mutex::new(conn),
            blobs: MemoryBlobStore::new(),
        };
        engine.migrate()?;
        Ok(engine)
    }

    /// 打开内存数据库（单测）
    pub fn open_in_memory() -> Result<Self, RynvekErrorCode> {
        let conn = Connection::open_in_memory().map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let engine = Self {
            path: PathBuf::from(":memory:"),
            conn: Mutex::new(conn),
            blobs: MemoryBlobStore::new(),
        };
        engine.migrate()?;
        Ok(engine)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> Result<(), RynvekErrorCode> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        // 已迁移则跳过（规范 DDL 使用无 IF NOT EXISTS 的 CREATE TABLE）
        let migrated: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='local_worlds'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !migrated {
            // PRAGMA journal_mode 会返回结果集，不能走 execute_batch
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(|_| RynvekErrorCode::SchemaMigrationPathNotFound)?;
            conn.pragma_update(None, "synchronous", "NORMAL")
                .map_err(|_| RynvekErrorCode::SchemaMigrationPathNotFound)?;
            conn.pragma_update(None, "foreign_keys", "ON")
                .map_err(|_| RynvekErrorCode::SchemaMigrationPathNotFound)?;
            let ddl = strip_sql_comments_and_pragmas(SQLITE_0001_LOCAL_STORAGE);
            conn.execute_batch(&ddl)
                .map_err(|_| RynvekErrorCode::SchemaMigrationPathNotFound)?;
        }
        // 补充提交链表（规范 Postgres 有 world_commits；SQLite DDL 未单列，
        // 为满足 StorageEngine 与崩溃恢复，在本地库增加兼容表——见 ERRATA E-004）
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS local_world_commits (
                world_id TEXT NOT NULL,
                world_revision INTEGER NOT NULL,
                tick INTEGER NOT NULL,
                parent_revision INTEGER,
                state_root_hex TEXT NOT NULL,
                journal_segment_cid_hex TEXT NOT NULL,
                snapshot_cid_hex TEXT,
                PRIMARY KEY (world_id, world_revision),
                FOREIGN KEY (world_id) REFERENCES local_worlds(world_id) ON DELETE CASCADE
            );
            "#,
        )
        .map_err(|_| RynvekErrorCode::SchemaMigrationPathNotFound)?;
        Ok(())
    }
}

/// 去掉注释与 PRAGMA，仅保留 CREATE/其余 DDL（PRAGMA 已在 migrate 中单独执行）
fn strip_sql_comments_and_pragmas(sql: &str) -> String {
    sql.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with("--") && !trimmed.to_ascii_uppercase().starts_with("PRAGMA")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn world_id_text(id: &[u8; 16]) -> String {
    use std::fmt::Write as _;
    id.iter().fold(String::with_capacity(32), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[allow(dead_code)]
fn parse_world_id(text: &str) -> Result<[u8; 16], RynvekErrorCode> {
    if text.len() != 32 {
        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
    }
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot =
            u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
    }
    Ok(out)
}

fn parse_hash(text: &str) -> Result<Blake3Hash, RynvekErrorCode> {
    Blake3Hash::from_hex(text)
}

impl StorageEngine for SqliteStorageEngine {
    fn blobs(&self) -> &dyn BlobStore {
        &self.blobs
    }

    fn upsert_world(&self, meta: WorldMeta) -> Result<(), RynvekErrorCode> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        conn.execute(
            r#"
            INSERT INTO local_worlds (
                world_id, title, world_profile, game_mode, execution_profile,
                latest_tick, latest_revision, latest_state_root_hex, opfs_dir_path, updated_at_ms
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
            ON CONFLICT(world_id) DO UPDATE SET
                title=excluded.title,
                world_profile=excluded.world_profile,
                game_mode=excluded.game_mode,
                execution_profile=excluded.execution_profile,
                latest_tick=excluded.latest_tick,
                latest_revision=excluded.latest_revision,
                latest_state_root_hex=excluded.latest_state_root_hex,
                opfs_dir_path=excluded.opfs_dir_path,
                updated_at_ms=excluded.updated_at_ms
            "#,
            params![
                world_id_text(&meta.world_id),
                meta.title,
                meta.world_profile,
                meta.game_mode,
                meta.execution_profile,
                meta.latest_tick as i64,
                meta.latest_revision as i64,
                meta.latest_state_root.to_hex(),
                format!("opfs://{}", world_id_text(&meta.world_id)),
                0_i64,
            ],
        )
        .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        Ok(())
    }

    fn get_world(&self, world_id: &[u8; 16]) -> Result<WorldMeta, RynvekErrorCode> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        conn.query_row(
            r#"
            SELECT title, world_profile, game_mode, execution_profile,
                   latest_tick, latest_revision, latest_state_root_hex
            FROM local_worlds WHERE world_id=?1
            "#,
            params![world_id_text(world_id)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .map_err(|_| RynvekErrorCode::EntityNotFoundOrTombstoned)
        .and_then(
            |(title, world_profile, game_mode, execution_profile, tick, rev, root)| {
                Ok(WorldMeta {
                    world_id: *world_id,
                    title,
                    world_profile,
                    game_mode,
                    execution_profile,
                    latest_tick: tick as u64,
                    latest_revision: rev as u64,
                    latest_state_root: parse_hash(&root)?,
                })
            },
        )
    }

    fn append_commit(&self, commit: WorldCommitRecord) -> Result<(), RynvekErrorCode> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        conn.execute(
            r#"
            INSERT INTO local_world_commits (
                world_id, world_revision, tick, parent_revision,
                state_root_hex, journal_segment_cid_hex, snapshot_cid_hex
            ) VALUES (?1,?2,?3,?4,?5,?6,?7)
            "#,
            params![
                world_id_text(&commit.world_id),
                commit.world_revision as i64,
                commit.tick as i64,
                commit.parent_revision.map(|v| v as i64),
                commit.state_root.to_hex(),
                commit.journal_segment_cid.to_hex(),
                commit.snapshot_cid.as_ref().map(BlobId::to_hex),
            ],
        )
        .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        conn.execute(
            r#"
            UPDATE local_worlds
            SET latest_tick=?1, latest_revision=?2, latest_state_root_hex=?3, updated_at_ms=?4
            WHERE world_id=?5
            "#,
            params![
                commit.tick as i64,
                commit.world_revision as i64,
                commit.state_root.to_hex(),
                0_i64,
                world_id_text(&commit.world_id),
            ],
        )
        .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        Ok(())
    }

    fn get_commit(&self, world_id: &[u8; 16], revision: u64) -> Result<WorldCommitRecord, RynvekErrorCode> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        conn.query_row(
            r#"
            SELECT tick, parent_revision, state_root_hex, journal_segment_cid_hex, snapshot_cid_hex
            FROM local_world_commits WHERE world_id=?1 AND world_revision=?2
            "#,
            params![world_id_text(world_id), revision as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .map_err(|_| RynvekErrorCode::EntityNotFoundOrTombstoned)
        .and_then(|(tick, parent, root, journal, snap)| {
            Ok(WorldCommitRecord {
                world_id: *world_id,
                world_revision: revision,
                tick: tick as u64,
                parent_revision: parent.map(|v| v as u64),
                state_root: parse_hash(&root)?,
                journal_segment_cid: parse_hash(&journal)?,
                snapshot_cid: snap.as_deref().map(parse_hash).transpose()?,
            })
        })
    }

    fn list_commits(&self, world_id: &[u8; 16]) -> Result<Vec<WorldCommitRecord>, RynvekErrorCode> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT world_revision, tick, parent_revision, state_root_hex,
                       journal_segment_cid_hex, snapshot_cid_hex
                FROM local_world_commits WHERE world_id=?1 ORDER BY world_revision ASC
                "#,
            )
            .map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let rows = stmt
            .query_map(params![world_id_text(world_id)], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let mut out = Vec::new();
        for row in rows {
            let (rev, tick, parent, root, journal, snap) =
                row.map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
            out.push(WorldCommitRecord {
                world_id: *world_id,
                world_revision: rev as u64,
                tick: tick as u64,
                parent_revision: parent.map(|v| v as u64),
                state_root: parse_hash(&root)?,
                journal_segment_cid: parse_hash(&journal)?,
                snapshot_cid: snap.as_deref().map(parse_hash).transpose()?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sqlite_migrates_and_roundtrips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("local.db");
        let engine = SqliteStorageEngine::open(&path).unwrap();
        let world_id = [0x11; 16];
        engine
            .upsert_world(WorldMeta {
                world_id,
                title: "Arena".into(),
                world_profile: "RTS_Standard".into(),
                game_mode: "PvE_Survival".into(),
                execution_profile: "D0_Strict".into(),
                latest_tick: 0,
                latest_revision: 0,
                latest_state_root: Blake3Hash::of(b""),
            })
            .unwrap();
        let journal = engine.blobs().put(b"seg").unwrap();
        let snap = engine.blobs().put(b"snap").unwrap();
        let root = Blake3Hash::of(b"state");
        engine
            .append_commit(WorldCommitRecord {
                world_id,
                world_revision: 1,
                tick: 120,
                parent_revision: None,
                state_root: root,
                journal_segment_cid: journal,
                snapshot_cid: Some(snap),
            })
            .unwrap();
        let loaded = SqliteStorageEngine::open(&path).unwrap();
        assert_eq!(loaded.get_world(&world_id).unwrap().latest_tick, 120);
        assert_eq!(loaded.list_commits(&world_id).unwrap().len(), 1);
    }
}
