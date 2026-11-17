// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! PostgreSQL 17 后端：直接执行规范 11.1 的 DDL（`rynvek-storage/migrations/postgres/0001_initial_schema.sql`），
//! 原规范DDL保留；应用扩展使用独立幂等迁移，不重写原规范附件。
//!
//! 类型映射：`BIGINT` ↔ `i64`（`u64` 标识按位重解释，规范内所有 ID 均 < 2^63）；`UUID` / `INET` / `JSONB`
//! 以 `::text` 往返（避免为 `tokio-postgres` 打开额外的类型特性）；`TIMESTAMPTZ` 读出为 Unix 秒。

use std::sync::Arc;

use rynvek_core::error::RynvekErrorCode;
use rynvek_protocol::WireUuid;
use rynvek_security::{League, PrincipalKindCode, PrincipalStatus, Visibility};
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls, Row};

use super::{
    AuditRow, CommitRow, MatchRow, MatchState, NewAudit, OrgRow, PackageRow, PackageVersionRow, ParticipantRow,
    PrincipalRow, RatingRow, StoreResult, WorldRow,
};
use crate::error::ApiError;

/// 规范 DDL 原文
pub const POSTGRES_SCHEMA_SQL: &str =
    include_str!("../../../rynvek-storage/migrations/postgres/0001_initial_schema.sql");

/// PostgreSQL 存储
#[derive(Clone)]
pub struct PostgresStore {
    client: Arc<Mutex<Client>>,
}

impl std::fmt::Debug for PostgresStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresStore").finish_non_exhaustive()
    }
}

fn db_err(e: tokio_postgres::Error) -> ApiError {
    use tokio_postgres::error::SqlState;
    match e.code() {
        Some(c) if *c == SqlState::UNIQUE_VIOLATION => ApiError::new(
            RynvekErrorCode::StructuralMutationConflict,
            format!("唯一约束冲突：{e}"),
        ),
        Some(c) if *c == SqlState::FOREIGN_KEY_VIOLATION => {
            ApiError::new(RynvekErrorCode::EntityNotFoundOrTombstoned, format!("外键约束：{e}"))
        }
        _ => ApiError::internal(format!("PostgreSQL 错误：{e}")),
    }
}

fn corrupt(what: &str) -> ApiError {
    ApiError::internal(format!("数据库中的 {what} 不符合约束"))
}

const fn i(v: u64) -> i64 {
    v as i64
}

const fn u(v: i64) -> u64 {
    v as u64
}

fn uuid_text(id: &WireUuid) -> String {
    id.to_string()
}

fn parse_uuid(text: String) -> StoreResult<WireUuid> {
    text.parse().map_err(|_| corrupt("uuid"))
}

fn arr32(v: Vec<u8>) -> StoreResult<[u8; 32]> {
    v.try_into().map_err(|_| corrupt("32 字节摘要"))
}

fn league_of(s: String) -> StoreResult<League> {
    League::parse(&s).ok_or_else(|| corrupt("league"))
}

fn json_of(s: String) -> StoreResult<serde_json::Value> {
    serde_json::from_str(&s).map_err(|_| corrupt("jsonb"))
}

const WORLD_COLS: &str = "world_id::text, org_id, owner_principal_id, title, world_profile, game_mode, execution_profile, \
     visibility, parent_world_id::text, forked_at_revision, latest_tick, latest_revision, latest_state_root, \
     lease_holder_node, fencing_token";

fn world_of(r: &Row) -> StoreResult<WorldRow> {
    Ok(WorldRow {
        world_id: parse_uuid(r.get(0))?,
        org_id: u(r.get(1)),
        owner_principal_id: u(r.get(2)),
        title: r.get(3),
        world_profile: r.get(4),
        game_mode: r.get(5),
        execution_profile: r.get(6),
        visibility: Visibility::from_i16(r.get(7)).ok_or_else(|| corrupt("visibility"))?,
        parent_world_id: r.get::<_, Option<String>>(8).map(parse_uuid).transpose()?,
        forked_at_revision: r.get::<_, Option<i64>>(9).map(u),
        latest_tick: u(r.get(10)),
        latest_revision: u(r.get(11)),
        latest_state_root: arr32(r.get(12))?,
        lease_holder_node: r.get(13),
        fencing_token: u(r.get(14)),
    })
}

const MATCH_COLS: &str = "match_id::text, world_id::text, game_mode, ruleset_blake3, map_seed, match_state, \
     winner_faction_id, win_reason, total_ticks, replay_cid_blake3, \
     extract(epoch from started_at)::bigint, extract(epoch from completed_at)::bigint";

fn match_of(r: &Row) -> StoreResult<MatchRow> {
    Ok(MatchRow {
        match_id: parse_uuid(r.get(0))?,
        world_id: parse_uuid(r.get(1))?,
        game_mode: r.get(2),
        ruleset_blake3: arr32(r.get(3))?,
        map_seed: u(r.get(4)),
        match_state: MatchState::parse(&r.get::<_, String>(5)).ok_or_else(|| corrupt("match_state"))?,
        winner_faction_id: r.get(6),
        win_reason: r.get(7),
        total_ticks: u(r.get(8)),
        replay_cid_blake3: r.get::<_, Option<Vec<u8>>>(9).map(arr32).transpose()?,
        started_at_unix_s: r.get::<_, Option<i64>>(10).map(u),
        completed_at_unix_s: r.get::<_, Option<i64>>(11).map(u),
    })
}

fn rating_of(r: &Row) -> StoreResult<RatingRow> {
    Ok(RatingRow {
        principal_id: u(r.get(0)),
        league: league_of(r.get(1))?,
        rating: r.get(2),
        deviation: r.get(3),
        volatility: r.get(4),
        matches_played: r.get(5),
    })
}

fn org_of(r: &Row) -> OrgRow {
    OrgRow {
        org_id: u(r.get(0)),
        slug: r.get(1),
        display_name: r.get(2),
        owner_principal_id: u(r.get(3)),
        quota_max_worlds: r.get(4),
        quota_storage_bytes: r.get(5),
    }
}

fn package_of(r: &Row) -> PackageRow {
    PackageRow {
        package_id: r.get(0),
        org_id: u(r.get(1)),
        package_kind: r.get(2),
        trust_level: r.get(3),
        revoked: r.get(4),
    }
}

impl PostgresStore {
    /// 连接（`postgres://user@host:port/db`）；连接任务在当前 Tokio 运行时后台运行
    pub async fn connect(url: &str) -> StoreResult<Self> {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await.map_err(db_err)?;
        tokio::spawn(async move {
            // 连接断开后所有查询都会返回错误，由调用方上报；这里无需额外处理
            let _ = connection.await;
        });
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
        })
    }

    /// 首装执行原规范DDL，再执行独立应用扩展的幂等迁移。
    pub async fn migrate(&self) -> StoreResult<bool> {
        let c = self.client.lock().await;
        let exists: bool = c
            .query_one("SELECT to_regclass('principals') IS NOT NULL", &[])
            .await
            .map_err(db_err)?
            .get(0);
        if !exists { c.batch_execute(POSTGRES_SCHEMA_SQL).await.map_err(db_err)?; }
        c.batch_execute(include_str!("../../../rynvek-storage/migrations/postgres/0002_observer_grants.sql")).await.map_err(db_err)?;
        Ok(!exists)
    }

    pub async fn observer_grants(&self, world: WireUuid) -> StoreResult<Vec<super::ObserverGrant>> {
        let c = self.client.lock().await;
        c.query("SELECT principal_id, faction, grant_id::text FROM world_observer_grants WHERE world_id=$1::text::uuid AND active ORDER BY principal_id", &[&uuid_text(&world)]).await.map_err(db_err)?
            .into_iter().map(|r| Ok(super::ObserverGrant { world_id:world, principal_id:u(r.get(0)), faction:u8::try_from(r.get::<_,i16>(1)).map_err(|_|corrupt("观战阵营"))?, grant_id:parse_uuid(r.get(2))?, active:true })).collect()
    }
    pub async fn set_observer_grant(&self, grant: super::ObserverGrant) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx=c.transaction().await.map_err(db_err)?;
        // 世界行锁让不同节点的并发准入也服从同一有效名额上限。
        tx.query_one("SELECT world_id::text FROM worlds WHERE world_id=$1::text::uuid FOR UPDATE", &[&uuid_text(&grant.world_id)]).await.map_err(db_err)?;
        let count:i64=tx.query_one("SELECT count(*) FROM world_observer_grants WHERE world_id=$1::text::uuid AND active AND principal_id<>$2", &[&uuid_text(&grant.world_id), &i(grant.principal_id)]).await.map_err(db_err)?.get(0);
        if grant.active && count>=32 { return Err(ApiError::bad_request("单世界最多32个有效观战准入")) }
        tx.execute("INSERT INTO world_observer_grants (world_id,principal_id,faction,grant_id,active) VALUES ($1::text::uuid,$2,$3,$4::text::uuid,$5) ON CONFLICT (world_id,principal_id) DO UPDATE SET faction=EXCLUDED.faction,grant_id=EXCLUDED.grant_id,active=EXCLUDED.active", &[&uuid_text(&grant.world_id), &i(grant.principal_id), &i16::from(grant.faction), &uuid_text(&grant.grant_id), &grant.active]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }
    pub async fn revoke_observer(&self, world: WireUuid, principal: u64, grant: WireUuid) -> StoreResult<bool> {
        let c = self.client.lock().await;
        Ok(c.execute("UPDATE world_observer_grants SET active=FALSE WHERE world_id=$1::text::uuid AND principal_id=$2 AND grant_id=$3::text::uuid AND active", &[&uuid_text(&world), &i(principal), &uuid_text(&grant)]).await.map_err(db_err)? == 1)
    }

    pub async fn insert_principal(&self, row: PrincipalRow) -> StoreResult<()> {
        let c = self.client.lock().await;
        c.execute(
            "INSERT INTO principals (principal_id, handle, principal_kind, ed25519_pubkey, email_hash, status) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &i(row.principal_id),
                &row.handle,
                &(row.kind as i16),
                &row.ed25519_pubkey.as_slice(),
                &row.email_hash,
                &(row.status as i16),
            ],
        )
        .await
        .map_err(db_err)?;
        Ok(())
    }

    pub async fn get_principal(&self, id: u64) -> StoreResult<Option<PrincipalRow>> {
        let c = self.client.lock().await;
        let row = c
            .query_opt(
                "SELECT principal_id, handle, principal_kind, ed25519_pubkey, email_hash, status \
                 FROM principals WHERE principal_id = $1",
                &[&i(id)],
            )
            .await
            .map_err(db_err)?;
        row.map(|r| {
            Ok(PrincipalRow {
                principal_id: u(r.get(0)),
                handle: r.get(1),
                kind: PrincipalKindCode::from_i16(r.get(2)).ok_or_else(|| corrupt("principal_kind"))?,
                ed25519_pubkey: arr32(r.get(3))?,
                email_hash: r.get(4),
                status: PrincipalStatus::from_i16(r.get(5)).ok_or_else(|| corrupt("status"))?,
            })
        })
        .transpose()
    }

    pub async fn tombstone_principal(&self, id: u64) -> StoreResult<()> {
        let c = self.client.lock().await;
        let n = c
            .execute(
                "UPDATE principals SET status = 2, handle = 'deleted-' || principal_id::text, email_hash = NULL, \
                 ed25519_pubkey = $2, updated_at = NOW() WHERE principal_id = $1",
                &[&i(id), &[0u8; 32].as_slice()],
            )
            .await
            .map_err(db_err)?;
        if n == 0 {
            return Err(ApiError::not_found("principal 不存在"));
        }
        Ok(())
    }

    pub async fn insert_org(&self, row: OrgRow) -> StoreResult<()> {
        let c = self.client.lock().await;
        c.execute(
            "INSERT INTO organizations (org_id, slug, display_name, owner_principal_id, quota_max_worlds, quota_storage_bytes) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &i(row.org_id),
                &row.slug,
                &row.display_name,
                &i(row.owner_principal_id),
                &row.quota_max_worlds,
                &row.quota_storage_bytes,
            ],
        )
        .await
        .map_err(db_err)?;
        Ok(())
    }

    pub async fn get_org(&self, id: u64) -> StoreResult<Option<OrgRow>> {
        let c = self.client.lock().await;
        let row = c
            .query_opt(
                "SELECT org_id, slug, display_name, owner_principal_id, quota_max_worlds, quota_storage_bytes \
                 FROM organizations WHERE org_id = $1",
                &[&i(id)],
            )
            .await
            .map_err(db_err)?;
        Ok(row.as_ref().map(org_of))
    }

    pub async fn orgs_owned_by(&self, principal_id: u64) -> StoreResult<Vec<OrgRow>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                "SELECT org_id, slug, display_name, owner_principal_id, quota_max_worlds, quota_storage_bytes \
                 FROM organizations WHERE owner_principal_id = $1 ORDER BY org_id",
                &[&i(principal_id)],
            )
            .await
            .map_err(db_err)?;
        Ok(rows.iter().map(org_of).collect())
    }

    pub async fn insert_world(&self, row: WorldRow, genesis: CommitRow) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let (owned, quota): (i64, i32) = {
            let r = tx
                .query_opt(
                    "SELECT (SELECT count(*) FROM worlds WHERE org_id = $1), quota_max_worlds \
                     FROM organizations WHERE org_id = $1 FOR UPDATE",
                    &[&i(row.org_id)],
                )
                .await
                .map_err(db_err)?
                .ok_or_else(|| ApiError::not_found("外键约束：org_id 不存在"))?;
            (r.get(0), r.get(1))
        };
        if owned >= i64::from(quota.max(0)) {
            return Err(ApiError::new(
                RynvekErrorCode::WorldMemoryBudgetExceeded,
                format!("组织世界配额已满（{quota}）"),
            ));
        }
        tx.execute(
            "INSERT INTO worlds (world_id, org_id, owner_principal_id, title, world_profile, game_mode, execution_profile, \
             visibility, parent_world_id, forked_at_revision, latest_tick, latest_revision, latest_state_root, \
             lease_holder_node, fencing_token) \
             VALUES ($1::text::uuid, $2, $3, $4, $5, $6, $7, $8, $9::text::uuid, $10, $11, $12, $13, $14, $15)",
            &[
                &uuid_text(&row.world_id),
                &i(row.org_id),
                &i(row.owner_principal_id),
                &row.title,
                &row.world_profile,
                &row.game_mode,
                &row.execution_profile,
                &(row.visibility as i16),
                &row.parent_world_id.as_ref().map(uuid_text),
                &row.forked_at_revision.map(i),
                &i(row.latest_tick),
                &i(row.latest_revision),
                &row.latest_state_root.as_slice(),
                &row.lease_holder_node,
                &i(row.fencing_token),
            ],
        )
        .await
        .map_err(db_err)?;
        insert_commit_row(&tx, &genesis).await?;
        tx.commit().await.map_err(db_err)
    }

    pub async fn get_world(&self, id: WireUuid) -> StoreResult<Option<WorldRow>> {
        let c = self.client.lock().await;
        let row = c
            .query_opt(
                &format!("SELECT {WORLD_COLS} FROM worlds WHERE world_id = $1::text::uuid"),
                &[&uuid_text(&id)],
            )
            .await
            .map_err(db_err)?;
        row.as_ref().map(world_of).transpose()
    }

    pub async fn list_worlds(&self, viewer: Option<u64>, limit: u32, offset: u32) -> StoreResult<Vec<WorldRow>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                &format!(
                    "SELECT {WORLD_COLS} FROM worlds WHERE owner_principal_id = $1 OR visibility = 2 \
                     ORDER BY world_id DESC LIMIT $2 OFFSET $3"
                ),
                &[&viewer.map(i), &i64::from(limit), &i64::from(offset)],
            )
            .await
            .map_err(db_err)?;
        rows.iter().map(world_of).collect()
    }

    pub async fn list_commits(&self, id: WireUuid) -> StoreResult<Vec<CommitRow>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                "SELECT world_id::text, world_revision, tick, parent_revision, state_root_blake3, journal_segment_cid, \
                 snapshot_cid FROM world_commits WHERE world_id = $1::text::uuid ORDER BY world_revision",
                &[&uuid_text(&id)],
            )
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                Ok(CommitRow {
                    world_id: parse_uuid(r.get(0))?,
                    world_revision: u(r.get(1)),
                    tick: u(r.get(2)),
                    parent_revision: r.get::<_, Option<i64>>(3).map(u),
                    state_root_blake3: arr32(r.get(4))?,
                    journal_segment_cid: arr32(r.get(5))?,
                    snapshot_cid: r.get::<_, Option<Vec<u8>>>(6).map(arr32).transpose()?,
                })
            })
            .collect()
    }

    pub async fn claim_world(&self, id: WireUuid, revision: u64, token: u64, holder: String) -> StoreResult<u64> {
        if token >= i64::MAX as u64 {
            return Err(ApiError::new(
                RynvekErrorCode::StructuralMutationConflict,
                "fencing 已耗尽",
            ));
        }
        let client = self.client.lock().await;
        let updated = client.execute("UPDATE worlds SET fencing_token = fencing_token + 1, lease_holder_node = $4, updated_at = NOW() WHERE world_id = $1::text::uuid AND latest_revision = $2 AND fencing_token = $3",
            &[&uuid_text(&id), &i(revision), &i(token), &holder]).await.map_err(db_err)?;
        if updated != 1 {
            return Err(ApiError::new(
                RynvekErrorCode::StructuralMutationConflict,
                "宿主认领基线已过期",
            ));
        }
        Ok(token + 1)
    }

    pub async fn append_commit(&self, commit: CommitRow, expected_fencing_token: u64) -> StoreResult<()> {
        if commit.parent_revision.and_then(|r| r.checked_add(1)) != Some(commit.world_revision) {
            return Err(ApiError::new(
                RynvekErrorCode::StructuralMutationConflict,
                "提交修订必须紧接前驱",
            ));
        }
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let n = tx
            .execute(
                "UPDATE worlds SET latest_revision = $3, latest_tick = $4, latest_state_root = $5, updated_at = NOW() \
                 WHERE world_id = $1::text::uuid AND fencing_token = $2 AND latest_revision = $6 AND latest_tick <= $4",
                &[
                    &uuid_text(&commit.world_id),
                    &i(expected_fencing_token),
                    &i(commit.world_revision),
                    &i(commit.tick),
                    &commit.state_root_blake3.as_slice(),
                    &i(commit.parent_revision.expect("已验证前驱")),
                ],
            )
            .await
            .map_err(db_err)?;
        if n == 0 {
            let exists = tx
                .query_opt(
                    "SELECT fencing_token FROM worlds WHERE world_id = $1::text::uuid",
                    &[&uuid_text(&commit.world_id)],
                )
                .await
                .map_err(db_err)?;
            return Err(if let Some(row) = exists {
                if u(row.get(0)) != expected_fencing_token {
                    ApiError::new(RynvekErrorCode::StalePartitionFencingToken, "世界租约已被接管")
                } else {
                    ApiError::new(RynvekErrorCode::StructuralMutationConflict, "提交前驱或 Tick 已过期")
                }
            } else {
                ApiError::not_found("外键约束：world_id 不存在")
            });
        }
        insert_commit_row(&tx, &commit).await?;
        tx.commit().await.map_err(db_err)
    }

    pub async fn upsert_package(&self, row: PackageRow) -> StoreResult<()> {
        let c = self.client.lock().await;
        c.execute(
            "INSERT INTO packages (package_id, org_id, package_kind, trust_level, revoked) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (package_id) DO UPDATE SET org_id = EXCLUDED.org_id, package_kind = EXCLUDED.package_kind, \
             trust_level = EXCLUDED.trust_level, revoked = EXCLUDED.revoked",
            &[
                &row.package_id,
                &i(row.org_id),
                &row.package_kind,
                &row.trust_level,
                &row.revoked,
            ],
        )
        .await
        .map_err(db_err)?;
        Ok(())
    }

    pub async fn get_package(&self, id: &str) -> StoreResult<Option<PackageRow>> {
        let c = self.client.lock().await;
        let row = c
            .query_opt(
                "SELECT package_id, org_id, package_kind, trust_level, revoked FROM packages WHERE package_id = $1",
                &[&id],
            )
            .await
            .map_err(db_err)?;
        Ok(row.as_ref().map(package_of))
    }

    pub async fn insert_package_version(&self, row: PackageVersionRow) -> StoreResult<()> {
        let c = self.client.lock().await;
        c.execute(
            "INSERT INTO package_versions (package_id, semver, min_engine_version, license_spdx, artifact_cid_blake3, \
             artifact_size_bytes, ed25519_signature, manifest_json, lockfile_json) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::text::jsonb, $9::text::jsonb)",
            &[
                &row.package_id,
                &row.semver,
                &row.min_engine_version,
                &row.license_spdx,
                &row.artifact_cid_blake3.as_slice(),
                &i(row.artifact_size_bytes),
                &row.ed25519_signature.as_slice(),
                &row.manifest_json.to_string(),
                &row.lockfile_json.to_string(),
            ],
        )
        .await
        .map_err(|e| {
            if e.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
                ApiError::new(RynvekErrorCode::DependencyLockConflict, "该版本已发布，包版本不可覆盖")
            } else {
                db_err(e)
            }
        })?;
        Ok(())
    }

    pub async fn package_versions(&self, id: &str) -> StoreResult<Vec<PackageVersionRow>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                "SELECT package_id, semver, min_engine_version, license_spdx, artifact_cid_blake3, artifact_size_bytes, \
                 ed25519_signature, manifest_json::text, lockfile_json::text FROM package_versions \
                 WHERE package_id = $1 ORDER BY semver",
                &[&id],
            )
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                Ok(PackageVersionRow {
                    package_id: r.get(0),
                    semver: r.get(1),
                    min_engine_version: r.get(2),
                    license_spdx: r.get(3),
                    artifact_cid_blake3: arr32(r.get(4))?,
                    artifact_size_bytes: u(r.get(5)),
                    ed25519_signature: r
                        .get::<_, Vec<u8>>(6)
                        .try_into()
                        .map_err(|_| corrupt("ed25519_signature"))?,
                    manifest_json: json_of(r.get(7))?,
                    lockfile_json: json_of(r.get(8))?,
                })
            })
            .collect()
    }

    pub async fn search_packages(&self, kind: Option<&str>, query: Option<&str>) -> StoreResult<Vec<PackageRow>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                "SELECT package_id, org_id, package_kind, trust_level, revoked FROM packages \
                 WHERE NOT revoked AND ($1::text IS NULL OR package_kind = $1) \
                 AND ($2::text IS NULL OR strpos(package_id, $2) > 0) ORDER BY package_id",
                &[&kind, &query],
            )
            .await
            .map_err(db_err)?;
        Ok(rows.iter().map(package_of).collect())
    }

    pub async fn insert_match(&self, row: MatchRow, participants: Vec<ParticipantRow>) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        tx.execute(
            "INSERT INTO matches (match_id, world_id, game_mode, ruleset_blake3, map_seed, match_state, \
             winner_faction_id, win_reason, total_ticks, replay_cid_blake3, started_at, completed_at) \
             VALUES ($1::text::uuid, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9, $10, to_timestamp($11::bigint), \
             to_timestamp($12::bigint))",
            &[
                &uuid_text(&row.match_id),
                &uuid_text(&row.world_id),
                &row.game_mode,
                &row.ruleset_blake3.as_slice(),
                &i(row.map_seed),
                &row.match_state.as_str(),
                &row.winner_faction_id,
                &row.win_reason,
                &i(row.total_ticks),
                &row.replay_cid_blake3.as_ref().map(|c| c.as_slice()),
                &row.started_at_unix_s.map(i),
                &row.completed_at_unix_s.map(i),
            ],
        )
        .await
        .map_err(db_err)?;
        for p in participants {
            tx.execute(
                "INSERT INTO match_participants (match_id, faction_id, principal_id, controller_league, final_score, \
                 rating_delta) VALUES ($1::text::uuid, $2, $3, $4, $5, $6)",
                &[
                    &uuid_text(&p.match_id),
                    &p.faction_id,
                    &i(p.principal_id),
                    &p.controller_league.as_str(),
                    &p.final_score,
                    &p.rating_delta,
                ],
            )
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    pub async fn get_match(&self, id: WireUuid) -> StoreResult<Option<(MatchRow, Vec<ParticipantRow>)>> {
        let c = self.client.lock().await;
        let Some(row) = c
            .query_opt(
                &format!("SELECT {MATCH_COLS} FROM matches WHERE match_id = $1::text::uuid"),
                &[&uuid_text(&id)],
            )
            .await
            .map_err(db_err)?
        else {
            return Ok(None);
        };
        let ps = c
            .query(
                "SELECT faction_id, principal_id, controller_league, final_score, rating_delta FROM match_participants \
                 WHERE match_id = $1::text::uuid ORDER BY faction_id",
                &[&uuid_text(&id)],
            )
            .await
            .map_err(db_err)?
            .iter()
            .map(|r| {
                Ok(ParticipantRow {
                    match_id: id,
                    faction_id: r.get(0),
                    principal_id: u(r.get(1)),
                    controller_league: league_of(r.get(2))?,
                    final_score: r.get(3),
                    rating_delta: r.get(4),
                })
            })
            .collect::<StoreResult<Vec<_>>>()?;
        Ok(Some((match_of(&row)?, ps)))
    }

    pub async fn update_match(&self, row: MatchRow) -> StoreResult<()> {
        self.update_match_guarded(row, None).await
    }
    pub async fn update_match_fenced(&self, row: MatchRow, fencing_token: u64) -> StoreResult<()> {
        self.update_match_guarded(row, Some(fencing_token)).await
    }
    async fn update_match_guarded(&self, row: MatchRow, fencing_token: Option<u64>) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        if let Some(expected) = fencing_token {
            // 与claim_world锁同一世界行，不能只用MVCC快照EXISTS检查后再写另一行。
            let locked = tx
                .query_opt(
                    "SELECT w.fencing_token FROM worlds w JOIN matches m ON m.world_id=w.world_id \
                 WHERE m.match_id=$1::text::uuid FOR UPDATE OF w",
                    &[&uuid_text(&row.match_id)],
                )
                .await
                .map_err(db_err)?
                .ok_or_else(|| crate::transactions::conflict("对局世界不存在"))?;
            if locked.get::<_, i64>(0) != i(expected) {
                return Err(crate::transactions::conflict("旧宿主不得覆盖恢复后的对局状态"));
            }
        }
        let n = tx
            .execute(
                "UPDATE matches SET game_mode = $2, ruleset_blake3 = $3, map_seed = $4, match_state = $5, \
                 winner_faction_id = $6, win_reason = $7, total_ticks = $8, replay_cid_blake3 = $9, \
                 started_at = to_timestamp($10::bigint), completed_at = to_timestamp($11::bigint) \
                 WHERE match_id = $1::text::uuid AND ($12::bigint IS NULL OR EXISTS \
                 (SELECT 1 FROM worlds w WHERE w.world_id = matches.world_id AND w.fencing_token = $12))",
                &[
                    &uuid_text(&row.match_id),
                    &row.game_mode,
                    &row.ruleset_blake3.as_slice(),
                    &i(row.map_seed),
                    &row.match_state.as_str(),
                    &row.winner_faction_id,
                    &row.win_reason,
                    &i(row.total_ticks),
                    &row.replay_cid_blake3.as_ref().map(|c| c.as_slice()),
                    &row.started_at_unix_s.map(i),
                    &row.completed_at_unix_s.map(i),
                    &fencing_token.map(i),
                ],
            )
            .await
            .map_err(db_err)?;
        if n == 0 {
            return Err(crate::transactions::conflict("match_id不存在或宿主fence已失效"));
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    pub async fn update_participant(&self, row: ParticipantRow) -> StoreResult<()> {
        let c = self.client.lock().await;
        let n = c
            .execute(
                "UPDATE match_participants SET principal_id = $3, controller_league = $4, final_score = $5, \
                 rating_delta = $6 WHERE match_id = $1::text::uuid AND faction_id = $2",
                &[
                    &uuid_text(&row.match_id),
                    &row.faction_id,
                    &i(row.principal_id),
                    &row.controller_league.as_str(),
                    &row.final_score,
                    &row.rating_delta,
                ],
            )
            .await
            .map_err(db_err)?;
        if n == 0 {
            return Err(ApiError::not_found("match_participants 不存在"));
        }
        Ok(())
    }

    pub async fn matches_of(&self, principal_id: u64) -> StoreResult<Vec<MatchRow>> {
        let c = self.client.lock().await;
        let cols = MATCH_COLS
            .replace("match_id::text", "m.match_id::text")
            .replace("world_id::text", "m.world_id::text");
        let rows = c
            .query(
                &format!(
                    "SELECT {cols} FROM matches m JOIN match_participants p ON p.match_id = m.match_id \
                     WHERE p.principal_id = $1 ORDER BY m.match_id"
                ),
                &[&i(principal_id)],
            )
            .await
            .map_err(db_err)?;
        rows.iter().map(match_of).collect()
    }

    pub async fn get_rating(&self, principal_id: u64, league: League) -> StoreResult<RatingRow> {
        let c = self.client.lock().await;
        let row = c
            .query_opt(
                "SELECT principal_id, league, rating, deviation, volatility, matches_played FROM glicko2_ratings \
                 WHERE principal_id = $1 AND league = $2",
                &[&i(principal_id), &league.as_str()],
            )
            .await
            .map_err(db_err)?;
        match row {
            Some(r) => rating_of(&r),
            None => Ok(RatingRow::fresh(principal_id, league)),
        }
    }

    pub async fn put_rating(&self, row: RatingRow) -> StoreResult<()> {
        let c = self.client.lock().await;
        c.execute(
            "INSERT INTO glicko2_ratings (principal_id, league, rating, deviation, volatility, matches_played) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (principal_id, league) DO UPDATE SET \
             rating = EXCLUDED.rating, deviation = EXCLUDED.deviation, volatility = EXCLUDED.volatility, \
             matches_played = EXCLUDED.matches_played, updated_at = NOW()",
            &[
                &i(row.principal_id),
                &row.league.as_str(),
                &row.rating,
                &row.deviation,
                &row.volatility,
                &row.matches_played,
            ],
        )
        .await
        .map_err(db_err)?;
        Ok(())
    }

    pub async fn leaderboard(&self, league: League, limit: u32) -> StoreResult<Vec<(RatingRow, String)>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                "SELECT r.principal_id, r.league, r.rating, r.deviation, r.volatility, r.matches_played, p.handle \
                 FROM glicko2_ratings r JOIN principals p ON p.principal_id = r.principal_id \
                 WHERE r.league = $1 AND p.status = 0 ORDER BY r.rating DESC, r.principal_id ASC LIMIT $2",
                &[&league.as_str(), &i64::from(limit)],
            )
            .await
            .map_err(db_err)?;
        rows.iter().map(|r| Ok((rating_of(r)?, r.get(6)))).collect()
    }

    pub async fn append_audit(&self, row: NewAudit) -> StoreResult<()> {
        let c = self.client.lock().await;
        c.execute(
            "INSERT INTO audit_logs (principal_id, action_type, target_resource, client_ip, details_json) \
             VALUES ($1, $2, $3, $4::text::inet, $5::text::jsonb)",
            &[
                &row.principal_id.map(i),
                &row.action_type,
                &row.target_resource,
                &row.client_ip.map(|ip| ip.to_string()),
                &row.details_json.to_string(),
            ],
        )
        .await
        .map_err(db_err)?;
        Ok(())
    }

    pub async fn audit_of(&self, principal_id: u64) -> StoreResult<Vec<AuditRow>> {
        let c = self.client.lock().await;
        let rows = c
            .query(
                "SELECT log_id, extract(epoch from occurred_at)::bigint, principal_id, action_type, target_resource, \
                 host(client_ip), details_json::text FROM audit_logs WHERE principal_id = $1 ORDER BY log_id",
                &[&i(principal_id)],
            )
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|r| {
                Ok(AuditRow {
                    log_id: r.get(0),
                    occurred_at_unix_s: u(r.get(1)),
                    principal_id: r.get::<_, Option<i64>>(2).map(u),
                    action_type: r.get(3),
                    target_resource: r.get(4),
                    client_ip: r
                        .get::<_, Option<String>>(5)
                        .map(|s| s.parse().map_err(|_| corrupt("client_ip")))
                        .transpose()?,
                    details_json: json_of(r.get(6))?,
                })
            })
            .collect()
    }
}

async fn insert_commit_row(tx: &tokio_postgres::Transaction<'_>, c: &CommitRow) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO world_commits (world_id, world_revision, tick, parent_revision, state_root_blake3, \
         journal_segment_cid, snapshot_cid) VALUES ($1::text::uuid, $2, $3, $4, $5, $6, $7)",
        &[
            &uuid_text(&c.world_id),
            &i(c.world_revision),
            &i(c.tick),
            &c.parent_revision.map(i),
            &c.state_root_blake3.as_slice(),
            &c.journal_segment_cid.as_slice(),
            &c.snapshot_cid.as_ref().map(|s| s.as_slice()),
        ],
    )
    .await
    .map_err(db_err)?;
    Ok(())
}
