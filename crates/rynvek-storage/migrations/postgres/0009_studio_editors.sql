-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS world_editor_grants (
 world_id UUID NOT NULL REFERENCES worlds(world_id),
 principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
 grant_id UUID NOT NULL,
 active BOOLEAN NOT NULL,
 PRIMARY KEY(world_id,principal_id)
);
