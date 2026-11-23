-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- 仅原General World的显式耐久规则意图；不是Lab实验或另一套权限表。
CREATE TABLE IF NOT EXISTS general_runs (
 run_id UUID PRIMARY KEY,
 world_id UUID NOT NULL REFERENCES worlds(world_id),
 owner_id BIGINT NOT NULL REFERENCES principals(principal_id),
 submission_id UUID NOT NULL,
 status TEXT NOT NULL,
 lease_until_s BIGINT NOT NULL DEFAULT 0,
 row_json JSONB NOT NULL,
 UNIQUE(owner_id,submission_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS general_run_reservation ON general_runs(world_id)
 WHERE status IN ('queued','running','paused','blocked');
CREATE INDEX IF NOT EXISTS general_run_claim ON general_runs(status,lease_until_s);
