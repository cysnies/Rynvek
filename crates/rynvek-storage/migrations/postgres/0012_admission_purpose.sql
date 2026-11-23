-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- 原记录缺省仅Studio，不删除历史或自动升级控制许可。与原世界行锁/提交事务共用同一准入表。
BEGIN;
ALTER TABLE world_editor_grants ADD COLUMN IF NOT EXISTS purpose SMALLINT NOT NULL DEFAULT 0 CHECK(purpose IN (0,1));
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='world_editor_grants'::regclass AND contype='p' AND pg_get_constraintdef(oid)='PRIMARY KEY (world_id, principal_id, purpose)') THEN
  ALTER TABLE world_editor_grants DROP CONSTRAINT IF EXISTS world_editor_grants_pkey;
  ALTER TABLE world_editor_grants ADD PRIMARY KEY(world_id,principal_id,purpose);
 END IF;
END $$;
COMMIT;
