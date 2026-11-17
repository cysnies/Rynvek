-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS agent_runs (
 run_id UUID PRIMARY KEY,owner_id BIGINT NOT NULL REFERENCES principals(principal_id),
 submission_id UUID NOT NULL,world_id UUID NOT NULL,active BOOLEAN NOT NULL,row_json JSONB NOT NULL,
 UNIQUE(owner_id,submission_id)
);
CREATE INDEX IF NOT EXISTS agent_runs_owner ON agent_runs(owner_id,run_id DESC);
CREATE UNIQUE INDEX IF NOT EXISTS agent_runs_one_active_world ON agent_runs(owner_id,world_id) WHERE active;
