// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规范 11 冻结的权威 DDL 文本（嵌入二进制，供迁移执行与合规审计）。

/// PostgreSQL 17 服务端初始 schema（规范 11.1）
pub const POSTGRES_0001_INITIAL_SCHEMA: &str = include_str!("../migrations/postgres/0001_initial_schema.sql");

/// SQLite / OPFS 客户端本地存储初始 schema（规范 11.2）
pub const SQLITE_0001_LOCAL_STORAGE: &str = include_str!("../migrations/sqlite/0001_local_storage.sql");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_ddl_contains_core_tables() {
        for table in [
            "principals",
            "organizations",
            "worlds",
            "world_commits",
            "packages",
            "package_versions",
            "matches",
            "match_participants",
            "glicko2_ratings",
            "audit_logs",
        ] {
            assert!(
                POSTGRES_0001_INITIAL_SCHEMA.contains(&format!("CREATE TABLE {table}")),
                "missing {table}"
            );
        }
    }

    #[test]
    fn sqlite_ddl_contains_core_tables() {
        for table in [
            "local_worlds",
            "local_snapshots",
            "local_assets_lru",
            "user_preferences",
        ] {
            assert!(
                SQLITE_0001_LOCAL_STORAGE.contains(&format!("CREATE TABLE {table}")),
                "missing {table}"
            );
        }
    }
}
