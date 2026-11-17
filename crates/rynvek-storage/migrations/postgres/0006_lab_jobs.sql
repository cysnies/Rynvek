-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- Lab专用作业元数据；私有CAS承载run检查点/四表，不改原规范DDL或删除历史。
CREATE TABLE IF NOT EXISTS lab_jobs (
 exp_id UUID PRIMARY KEY,
 owner_id BIGINT NOT NULL REFERENCES principals(principal_id),
 submission_id UUID NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('queued','running','completed','failed','cancelled')),
 lease_until_s BIGINT,
 row_json JSONB NOT NULL,
 UNIQUE(owner_id,submission_id),
 CHECK(row_json->'experiment'->>'status'=status)
);
CREATE INDEX IF NOT EXISTS lab_jobs_owner_page ON lab_jobs(owner_id,exp_id DESC);
CREATE INDEX IF NOT EXISTS lab_jobs_claimable ON lab_jobs(status,lease_until_s,exp_id);
