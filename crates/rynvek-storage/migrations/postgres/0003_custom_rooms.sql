-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- 应用扩展：结构化当前版本房间，原规范表/原材料不改写。
CREATE TABLE IF NOT EXISTS world_custom_rooms (
    world_id UUID PRIMARY KEY REFERENCES worlds(world_id),
    version UUID NOT NULL,
    guest_id BIGINT NOT NULL REFERENCES principals(principal_id),
    room_json JSONB NOT NULL
);
