-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS private_agent_versions (
    owner_id BIGINT NOT NULL REFERENCES principals(principal_id),
    package_id TEXT NOT NULL,
    semver TEXT NOT NULL,
    row_json JSONB NOT NULL,
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(owner_id,package_id,semver)
);
