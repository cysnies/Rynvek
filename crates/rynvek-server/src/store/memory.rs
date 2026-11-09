// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 进程内存储：以 `BTreeMap` 镜像规范 DDL 的主键、唯一约束与外键语义（违例返回与 Postgres 后端相同的错误码）。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use rynvek_core::error::RynvekErrorCode;
use rynvek_protocol::WireUuid;
use rynvek_security::{League, PrincipalStatus, Visibility};

use super::{
    AuditRow, CommitRow, MatchRow, NewAudit, OrgRow, PackageRow, PackageVersionRow, ParticipantRow, PrincipalRow,
    RatingRow, StoreResult, WorldRow,
};
use crate::error::ApiError;

#[derive(Debug, Default)]
struct Tables {
    principals: BTreeMap<u64, PrincipalRow>,
    organizations: BTreeMap<u64, OrgRow>,
    worlds: BTreeMap<WireUuid, WorldRow>,
    world_commits: BTreeMap<(WireUuid, u64), CommitRow>,
    packages: BTreeMap<String, PackageRow>,
    package_versions: BTreeMap<(String, String), PackageVersionRow>,
    matches: BTreeMap<WireUuid, MatchRow>,
    match_participants: BTreeMap<(WireUuid, i16), ParticipantRow>,
    glicko2_ratings: BTreeMap<(u64, League), RatingRow>,
    audit_logs: Vec<AuditRow>,
}

/// 进程内存储
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    t: Arc<Mutex<Tables>>,
}

fn conflict(message: impl Into<String>) -> ApiError {
    ApiError::new(RynvekErrorCode::StructuralMutationConflict, message)
}

fn missing_fk(what: &str) -> ApiError {
    ApiError::new(
        RynvekErrorCode::EntityNotFoundOrTombstoned,
        format!("外键约束：{what} 不存在"),
    )
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Tables> {
        // 持锁期间不会 panic；即便中毒也继续使用已提交的数据
        self.t.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn insert_principal(&self, row: PrincipalRow) -> StoreResult<()> {
        let mut t = self.lock();
        if t.principals.contains_key(&row.principal_id) {
            return Err(conflict("principal_id 已存在"));
        }
        if t.principals.values().any(|p| p.handle == row.handle) {
            return Err(conflict("handle 已被占用"));
        }
        t.principals.insert(row.principal_id, row);
        Ok(())
    }

    pub fn get_principal(&self, id: u64) -> StoreResult<Option<PrincipalRow>> {
        Ok(self.lock().principals.get(&id).cloned())
    }

    pub fn tombstone_principal(&self, id: u64) -> StoreResult<()> {
        let mut t = self.lock();
        let p = t.principals.get_mut(&id).ok_or_else(|| missing_fk("principal"))?;
        p.status = PrincipalStatus::Deleted;
        p.handle = format!("deleted-{id}");
        p.email_hash = None;
        p.ed25519_pubkey = [0; 32];
        Ok(())
    }

    pub fn insert_org(&self, row: OrgRow) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.principals.contains_key(&row.owner_principal_id) {
            return Err(missing_fk("owner_principal_id"));
        }
        if t.organizations.contains_key(&row.org_id) || t.organizations.values().any(|o| o.slug == row.slug) {
            return Err(conflict("org_id 或 slug 已存在"));
        }
        t.organizations.insert(row.org_id, row);
        Ok(())
    }

    pub fn get_org(&self, id: u64) -> StoreResult<Option<OrgRow>> {
        Ok(self.lock().organizations.get(&id).cloned())
    }

    pub fn orgs_owned_by(&self, principal_id: u64) -> StoreResult<Vec<OrgRow>> {
        Ok(self
            .lock()
            .organizations
            .values()
            .filter(|o| o.owner_principal_id == principal_id)
            .cloned()
            .collect())
    }

    pub fn insert_world(&self, row: WorldRow, genesis: CommitRow) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.organizations.contains_key(&row.org_id) {
            return Err(missing_fk("org_id"));
        }
        if !t.principals.contains_key(&row.owner_principal_id) {
            return Err(missing_fk("owner_principal_id"));
        }
        if let Some(parent) = row.parent_world_id {
            if !t.worlds.contains_key(&parent) {
                return Err(missing_fk("parent_world_id"));
            }
        }
        if t.worlds.contains_key(&row.world_id) {
            return Err(conflict("world_id 已存在"));
        }
        let owned = t.worlds.values().filter(|w| w.org_id == row.org_id).count();
        let quota = t.organizations[&row.org_id].quota_max_worlds.max(0) as usize;
        if owned >= quota {
            return Err(ApiError::new(
                RynvekErrorCode::WorldMemoryBudgetExceeded,
                format!("组织世界配额已满（{quota}）"),
            ));
        }
        t.world_commits
            .insert((genesis.world_id, genesis.world_revision), genesis);
        t.worlds.insert(row.world_id, row);
        Ok(())
    }

    pub fn get_world(&self, id: WireUuid) -> StoreResult<Option<WorldRow>> {
        Ok(self.lock().worlds.get(&id).cloned())
    }

    pub fn list_worlds(&self, viewer: Option<u64>, limit: u32, offset: u32) -> StoreResult<Vec<WorldRow>> {
        Ok(self
            .lock()
            .worlds
            .values()
            .rev()
            .filter(|w| Some(w.owner_principal_id) == viewer || w.visibility == Visibility::Public)
            .skip(offset as usize)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    pub fn list_commits(&self, id: WireUuid) -> StoreResult<Vec<CommitRow>> {
        Ok(self
            .lock()
            .world_commits
            .range((id, 0)..=(id, u64::MAX))
            .map(|(_, c)| c.clone())
            .collect())
    }

    pub fn append_commit(&self, commit: CommitRow, expected_fencing_token: u64) -> StoreResult<()> {
        let mut t = self.lock();
        let world = t.worlds.get(&commit.world_id).ok_or_else(|| missing_fk("world_id"))?;
        if world.fencing_token != expected_fencing_token {
            return Err(ApiError::new(
                RynvekErrorCode::StalePartitionFencingToken,
                "fencing_token 已过期：世界租约已被其他节点接管",
            ));
        }
        if t.world_commits.contains_key(&(commit.world_id, commit.world_revision)) {
            return Err(conflict("world_revision 已存在"));
        }
        let w = t.worlds.get_mut(&commit.world_id).expect("已检查");
        w.latest_revision = commit.world_revision;
        w.latest_tick = commit.tick;
        w.latest_state_root = commit.state_root_blake3;
        t.world_commits.insert((commit.world_id, commit.world_revision), commit);
        Ok(())
    }

    pub fn upsert_package(&self, row: PackageRow) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.organizations.contains_key(&row.org_id) {
            return Err(missing_fk("org_id"));
        }
        t.packages.insert(row.package_id.clone(), row);
        Ok(())
    }

    pub fn get_package(&self, id: &str) -> StoreResult<Option<PackageRow>> {
        Ok(self.lock().packages.get(id).cloned())
    }

    pub fn insert_package_version(&self, row: PackageVersionRow) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.packages.contains_key(&row.package_id) {
            return Err(missing_fk("package_id"));
        }
        let key = (row.package_id.clone(), row.semver.clone());
        if t.package_versions.contains_key(&key) {
            return Err(ApiError::new(
                RynvekErrorCode::DependencyLockConflict,
                "该版本已发布，包版本不可覆盖",
            ));
        }
        t.package_versions.insert(key, row);
        Ok(())
    }

    pub fn package_versions(&self, id: &str) -> StoreResult<Vec<PackageVersionRow>> {
        Ok(self
            .lock()
            .package_versions
            .values()
            .filter(|v| v.package_id == id)
            .cloned()
            .collect())
    }

    pub fn search_packages(&self, kind: Option<&str>, query: Option<&str>) -> StoreResult<Vec<PackageRow>> {
        Ok(self
            .lock()
            .packages
            .values()
            .filter(|p| !p.revoked)
            .filter(|p| kind.is_none_or(|k| p.package_kind == k))
            .filter(|p| query.is_none_or(|q| p.package_id.contains(q)))
            .cloned()
            .collect())
    }

    pub fn insert_match(&self, row: MatchRow, participants: Vec<ParticipantRow>) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.worlds.contains_key(&row.world_id) {
            return Err(missing_fk("world_id"));
        }
        if t.matches.contains_key(&row.match_id) {
            return Err(conflict("match_id 已存在"));
        }
        for p in &participants {
            if !t.principals.contains_key(&p.principal_id) {
                return Err(missing_fk("participant principal_id"));
            }
        }
        for p in participants {
            t.match_participants.insert((p.match_id, p.faction_id), p);
        }
        t.matches.insert(row.match_id, row);
        Ok(())
    }

    pub fn get_match(&self, id: WireUuid) -> StoreResult<Option<(MatchRow, Vec<ParticipantRow>)>> {
        let t = self.lock();
        Ok(t.matches.get(&id).map(|m| {
            let ps = t
                .match_participants
                .range((id, i16::MIN)..=(id, i16::MAX))
                .map(|(_, p)| p.clone())
                .collect();
            (m.clone(), ps)
        }))
    }

    pub fn update_match(&self, row: MatchRow) -> StoreResult<()> {
        let mut t = self.lock();
        let slot = t.matches.get_mut(&row.match_id).ok_or_else(|| missing_fk("match_id"))?;
        *slot = row;
        Ok(())
    }

    pub fn update_participant(&self, row: ParticipantRow) -> StoreResult<()> {
        let mut t = self.lock();
        let slot = t
            .match_participants
            .get_mut(&(row.match_id, row.faction_id))
            .ok_or_else(|| missing_fk("match_participants"))?;
        *slot = row;
        Ok(())
    }

    pub fn matches_of(&self, principal_id: u64) -> StoreResult<Vec<MatchRow>> {
        let t = self.lock();
        Ok(t.match_participants
            .values()
            .filter(|p| p.principal_id == principal_id)
            .filter_map(|p| t.matches.get(&p.match_id).cloned())
            .collect())
    }

    pub fn get_rating(&self, principal_id: u64, league: League) -> StoreResult<RatingRow> {
        Ok(self
            .lock()
            .glicko2_ratings
            .get(&(principal_id, league))
            .cloned()
            .unwrap_or_else(|| RatingRow::fresh(principal_id, league)))
    }

    pub fn put_rating(&self, row: RatingRow) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.principals.contains_key(&row.principal_id) {
            return Err(missing_fk("principal_id"));
        }
        t.glicko2_ratings.insert((row.principal_id, row.league), row);
        Ok(())
    }

    pub fn leaderboard(&self, league: League, limit: u32) -> StoreResult<Vec<(RatingRow, String)>> {
        let t = self.lock();
        let mut rows: Vec<(RatingRow, String)> = t
            .glicko2_ratings
            .values()
            .filter(|r| r.league == league)
            .filter_map(|r| {
                let p = t.principals.get(&r.principal_id)?;
                (p.status == PrincipalStatus::Active).then(|| (r.clone(), p.handle.clone()))
            })
            .collect();
        rows.sort_by(|a, b| {
            b.0.rating
                .total_cmp(&a.0.rating)
                .then(a.0.principal_id.cmp(&b.0.principal_id))
        });
        rows.truncate(limit as usize);
        Ok(rows)
    }

    pub fn append_audit(&self, row: NewAudit) -> StoreResult<()> {
        let mut t = self.lock();
        let log_id = t.audit_logs.len() as i64 + 1;
        t.audit_logs.push(AuditRow {
            log_id,
            occurred_at_unix_s: crate::ids::unix_now_s(),
            principal_id: row.principal_id,
            action_type: row.action_type,
            target_resource: row.target_resource,
            client_ip: row.client_ip,
            details_json: row.details_json,
        });
        Ok(())
    }

    pub fn audit_of(&self, principal_id: u64) -> StoreResult<Vec<AuditRow>> {
        Ok(self
            .lock()
            .audit_logs
            .iter()
            .filter(|a| a.principal_id == Some(principal_id))
            .cloned()
            .collect())
    }
}
