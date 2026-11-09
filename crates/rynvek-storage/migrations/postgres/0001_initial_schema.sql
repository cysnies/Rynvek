-- crates/rynvek-storage/migrations/postgres/0001_initial_schema.sql
-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only

CREATE TABLE principals (
    principal_id BIGINT PRIMARY KEY,
    handle VARCHAR(64) NOT NULL UNIQUE,
    principal_kind SMALLINT NOT NULL CHECK (principal_kind IN (0, 1, 2)), -- 0: Human, 1: Bot/Agent, 2: Service
    ed25519_pubkey BYTEA NOT NULL CHECK (octet_length(ed25519_pubkey) = 32),
    email_hash BYTEA,
    status SMALLINT NOT NULL DEFAULT 0, -- 0: Active, 1: Suspended, 2: Deleted
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE organizations (
    org_id BIGINT PRIMARY KEY,
    slug VARCHAR(64) NOT NULL UNIQUE,
    display_name VARCHAR(128) NOT NULL,
    owner_principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    quota_max_worlds INT NOT NULL DEFAULT 10,
    quota_storage_bytes BIGINT NOT NULL DEFAULT 10737418240, -- 10 GiB
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE worlds (
    world_id UUID PRIMARY KEY,
    org_id BIGINT NOT NULL REFERENCES organizations(org_id),
    owner_principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    title VARCHAR(128) NOT NULL,
    world_profile VARCHAR(32) NOT NULL,     -- RTS_Standard, Sandbox_Creative, Research_Batch, Persistent_Ecology
    game_mode VARCHAR(32) NOT NULL,         -- PvP_Ranked, PvP_Custom, PvE_Survival, Evolution_Benchmark
    execution_profile VARCHAR(32) NOT NULL, -- D0_Strict, D1_Reproducible, D2_Bounded, D3_Statistical
    visibility SMALLINT NOT NULL DEFAULT 0, -- 0: Private, 1: Unlisted, 2: Public
    parent_world_id UUID REFERENCES worlds(world_id),
    forked_at_revision BIGINT,
    latest_tick BIGINT NOT NULL DEFAULT 0,
    latest_revision BIGINT NOT NULL DEFAULT 0,
    latest_state_root BYTEA NOT NULL CHECK (octet_length(latest_state_root) = 32),
    lease_holder_node VARCHAR(128),
    fencing_token BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_worlds_owner ON worlds(owner_principal_id);
CREATE INDEX idx_worlds_public ON worlds(visibility, updated_at DESC) WHERE visibility = 2;

CREATE TABLE world_commits (
    world_id UUID NOT NULL REFERENCES worlds(world_id) ON DELETE CASCADE,
    world_revision BIGINT NOT NULL,
    tick BIGINT NOT NULL,
    parent_revision BIGINT,
    state_root_blake3 BYTEA NOT NULL CHECK (octet_length(state_root_blake3) = 32),
    journal_segment_cid BYTEA NOT NULL CHECK (octet_length(journal_segment_cid) = 32),
    snapshot_cid BYTEA CHECK (snapshot_cid IS NULL OR octet_length(snapshot_cid) = 32),
    committed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (world_id, world_revision)
);

CREATE TABLE packages (
    package_id VARCHAR(128) PRIMARY KEY, -- e.g. "rynvek.visual.industrial-default"
    org_id BIGINT NOT NULL REFERENCES organizations(org_id),
    package_kind VARCHAR(16) NOT NULL CHECK (package_kind IN ('rworld', 'rpack', 'rview', 'audio', 'agent', 'dataset')),
    trust_level SMALLINT NOT NULL DEFAULT 3, -- 0: Builtin, 1: Official, 2: Verified, 3: Community
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE package_versions (
    package_id VARCHAR(128) NOT NULL REFERENCES packages(package_id) ON DELETE CASCADE,
    semver VARCHAR(32) NOT NULL,
    min_engine_version VARCHAR(32) NOT NULL,
    license_spdx VARCHAR(64) NOT NULL,
    artifact_cid_blake3 BYTEA NOT NULL CHECK (octet_length(artifact_cid_blake3) = 32),
    artifact_size_bytes BIGINT NOT NULL,
    ed25519_signature BYTEA NOT NULL CHECK (octet_length(ed25519_signature) = 64),
    manifest_json JSONB NOT NULL,
    lockfile_json JSONB NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (package_id, semver)
);

CREATE TABLE matches (
    match_id UUID PRIMARY KEY,
    world_id UUID NOT NULL REFERENCES worlds(world_id),
    game_mode VARCHAR(32) NOT NULL,
    ruleset_blake3 BYTEA NOT NULL CHECK (octet_length(ruleset_blake3) = 32),
    map_seed BIGINT NOT NULL,
    match_state VARCHAR(16) NOT NULL, -- Created, Lobby, Ready, Running, Completed, Aborted
    winner_faction_id SMALLINT,
    win_reason VARCHAR(32),           -- Elimination, Surrender, TieBreakerScore
    total_ticks BIGINT NOT NULL DEFAULT 0,
    replay_cid_blake3 BYTEA CHECK (replay_cid_blake3 IS NULL OR octet_length(replay_cid_blake3) = 32),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE TABLE match_participants (
    match_id UUID NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
    faction_id SMALLINT NOT NULL,
    principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    controller_league VARCHAR(16) NOT NULL CHECK (controller_league IN ('human', 'script', 'agent_hybrid')),
    final_score BIGINT NOT NULL DEFAULT 0,
    rating_delta DOUBLE PRECISION,
    PRIMARY KEY (match_id, faction_id)
);

CREATE TABLE glicko2_ratings (
    principal_id BIGINT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    league VARCHAR(16) NOT NULL CHECK (league IN ('human', 'script', 'agent_hybrid')),
    rating DOUBLE PRECISION NOT NULL DEFAULT 1500.0,
    deviation DOUBLE PRECISION NOT NULL DEFAULT 350.0,
    volatility DOUBLE PRECISION NOT NULL DEFAULT 0.06,
    matches_played INT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (principal_id, league)
);

CREATE TABLE audit_logs (
    log_id BIGSERIAL PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    principal_id BIGINT,
    action_type VARCHAR(64) NOT NULL,
    target_resource VARCHAR(128) NOT NULL,
    client_ip INET,
    details_json JSONB NOT NULL
);
