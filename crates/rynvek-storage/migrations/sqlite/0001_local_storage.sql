-- crates/rynvek-storage/migrations/sqlite/0001_local_storage.sql
-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE local_worlds (
    world_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    world_profile TEXT NOT NULL,
    game_mode TEXT NOT NULL,
    execution_profile TEXT NOT NULL,
    latest_tick INTEGER NOT NULL DEFAULT 0,
    latest_revision INTEGER NOT NULL DEFAULT 0,
    latest_state_root_hex TEXT NOT NULL,
    opfs_dir_path TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE local_snapshots (
    snapshot_cid_hex TEXT PRIMARY KEY,
    world_id TEXT NOT NULL REFERENCES local_worlds(world_id) ON DELETE CASCADE,
    tick INTEGER NOT NULL,
    world_revision INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    opfs_file_path TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE local_assets_lru (
    asset_cid_hex TEXT PRIMARY KEY,
    package_id TEXT NOT NULL,
    asset_kind TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    last_accessed_ms INTEGER NOT NULL,
    opfs_blob_path TEXT NOT NULL
);

CREATE TABLE user_preferences (
    pref_key TEXT PRIMARY KEY,
    pref_value_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
