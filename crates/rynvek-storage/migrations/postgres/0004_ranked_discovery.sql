-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- 单一入口协调行：固定锁序 coordination -> world，阻止跨节点跨模式重复冻结席位。
CREATE TABLE IF NOT EXISTS matchmaking_coordination (id SMALLINT PRIMARY KEY CHECK (id=1));
INSERT INTO matchmaking_coordination VALUES (1) ON CONFLICT DO NOTHING;
CREATE TABLE IF NOT EXISTS ranked_queue_tickets (
    principal_id BIGINT PRIMARY KEY REFERENCES principals(principal_id),
    ticket_json JSONB NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE INDEX IF NOT EXISTS ranked_queue_active ON ranked_queue_tickets(principal_id) WHERE active;
CREATE TABLE IF NOT EXISTS ranked_host_leases (
    world_id UUID PRIMARY KEY REFERENCES worlds(world_id),
    node_id TEXT NOT NULL,
    public_origin TEXT NOT NULL,
    fencing_token BIGINT NOT NULL CHECK (fencing_token>0),
    expires_at_s BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS ranked_match_allocations (
    match_id UUID PRIMARY KEY REFERENCES matches(match_id),
    join_deadline_s BIGINT NOT NULL,
    open BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE INDEX IF NOT EXISTS ranked_allocation_deadline ON ranked_match_allocations(join_deadline_s) WHERE open;
