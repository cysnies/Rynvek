-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- 应用扩展：定向阵营观战，绝不将私有世界改成Public或授予WorldRead。
CREATE TABLE IF NOT EXISTS world_observer_grants (
    world_id UUID NOT NULL REFERENCES worlds(world_id),
    principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    faction SMALLINT NOT NULL CHECK (faction >= 0 AND faction < 8),
    grant_id UUID NOT NULL,
    active BOOLEAN NOT NULL,
    PRIMARY KEY (world_id, principal_id)
);
