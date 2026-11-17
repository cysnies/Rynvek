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
    private_agents: BTreeMap<(u64, String, String), super::PrivateAgentRow>,
    ranked_deadlines: BTreeMap<WireUuid, u64>,
    ranked_tickets: Vec<crate::ranked::QueueTicket>,
    ranked_hosts: BTreeMap<WireUuid, crate::ranked::HostLease>,
    principals: BTreeMap<u64, PrincipalRow>,
    organizations: BTreeMap<u64, OrgRow>,
    worlds: BTreeMap<WireUuid, WorldRow>,
    custom_rooms: BTreeMap<WireUuid, super::CustomRoom>,
    observers: BTreeMap<(WireUuid, u64), super::ObserverGrant>,
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
    pub fn put_private_agent(&self, row: super::PrivateAgentRow) -> StoreResult<()> {
        let mut t = self.lock();
        let key = (row.owner_id, row.package_id.clone(), row.version.clone());
        if !t
            .principals
            .get(&row.owner_id)
            .is_some_and(|p| p.status == PrincipalStatus::Active && p.ed25519_pubkey == row.publisher_key)
        {
            return Err(conflict("发布者已失效"));
        }
        if t.private_agents.contains_key(&key) {
            return Err(conflict("私有版本不可覆盖，包括已撤销版本"));
        }
        if t.private_agents.keys().filter(|(o, _, _)| *o == row.owner_id).count() >= 256 {
            return Err(conflict("当前私有registry每主体最多256个不可变版本"));
        }
        for d in &row.dependencies {
            if !t
                .private_agents
                .get(&(row.owner_id, d.id.clone(), d.version.clone()))
                .is_some_and(|p| !p.revoked && crate::ids::hex(&p.cid) == d.blake3_256)
            {
                return Err(conflict("私有依赖已改变/撤销"));
            }
        }
        t.private_agents.insert(key, row);
        Ok(())
    }
    pub fn private_agent(
        &self,
        owner: u64,
        id: String,
        version: String,
    ) -> StoreResult<Option<super::PrivateAgentRow>> {
        Ok(self.lock().private_agents.get(&(owner, id, version)).cloned())
    }
    pub fn private_agents(&self, owner: u64) -> StoreResult<Vec<super::PrivateAgentRow>> {
        Ok(self
            .lock()
            .private_agents
            .values()
            .filter(|p| p.owner_id == owner)
            .cloned()
            .collect())
    }
    pub fn revoke_private_agent(&self, owner: u64, id: String, version: String) -> StoreResult<bool> {
        let mut t = self.lock();
        let Some(row) = t.private_agents.get_mut(&(owner, id, version)) else {
            return Ok(false);
        };
        row.revoked = true;
        Ok(true)
    }

    pub fn settle_ranked(&self, row: MatchRow, fence: u64, mut parts: Vec<ParticipantRow>) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.worlds.get(&row.world_id).is_some_and(|w| w.fencing_token == fence)
            || t.ranked_hosts
                .get(&row.world_id)
                .is_some_and(|h| h.fence != fence || h.expires_at_s <= crate::ids::unix_now_s())
        {
            return Err(conflict("终局fence/租约失效"));
        }
        let current = t.matches.get(&row.match_id).ok_or_else(|| missing_fk("对局"))?;
        if current.match_state == super::MatchState::Completed {
            return if current.replay_cid_blake3 == row.replay_cid_blake3 {
                Ok(())
            } else {
                Err(conflict("已结算不同终局"))
            };
        }
        if current.match_state.is_terminal()
            || row.game_mode != "PvP_Ranked"
            || parts.len() != 2
            || parts.iter().any(|p| {
                !t.match_participants
                    .get(&(p.match_id, p.faction_id))
                    .is_some_and(|old| old.principal_id == p.principal_id && p.match_id == row.match_id)
            })
        {
            return Err(conflict("终局席位无效"));
        }
        let old: Vec<_> = parts
            .iter()
            .map(|p| {
                t.glicko2_ratings
                    .get(&(p.principal_id, p.controller_league))
                    .cloned()
                    .unwrap_or_else(|| RatingRow::fresh(p.principal_id, p.controller_league))
            })
            .collect();
        let ratings = crate::ranked::ratings_for(&row, &mut parts, &old);
        for r in ratings {
            t.glicko2_ratings.insert((r.principal_id, r.league), r);
        }
        for p in parts {
            t.match_participants.insert((p.match_id, p.faction_id), p);
        }
        t.matches.insert(row.match_id, row);
        Ok(())
    }

    pub fn ranked_queue(
        &self,
        req: crate::ranked::QueueCommand,
        genesis: Option<crate::ranked::RankedGenesis>,
    ) -> StoreResult<crate::ranked::QueueDecision> {
        use crate::ranked::*;
        let mut t = self.lock();
        let pid = req.ticket.entry.principal_id;
        expire_reservations(&mut t, crate::ids::unix_now_s())?;
        if !t
            .principals
            .get(&pid)
            .is_some_and(|p| p.status == PrincipalStatus::Active)
        {
            return Err(missing_fk("有效主体"));
        }
        if let Some(row) = t
            .match_participants
            .values()
            .filter(|p| p.principal_id == pid)
            .filter_map(|p| t.matches.get(&p.match_id))
            .find(|m| !m.match_state.is_terminal())
        {
            if row.game_mode != "PvP_Ranked" {
                return Err(occupied_error());
            }
            return Ok(QueueDecision::Matched(row.match_id));
        }
        let mut tickets = t.ranked_tickets.clone();
        tickets.retain(|q| {
            t.principals
                .get(&q.entry.principal_id)
                .is_some_and(|p| p.status == PrincipalStatus::Active)
                && !t.match_participants.values().any(|p| {
                    p.principal_id == q.entry.principal_id
                        && t.matches.get(&p.match_id).is_some_and(|m| !m.match_state.is_terminal())
                })
        });
        let (mut decision, pair) = decide(&mut tickets, &req, genesis.is_some());
        if let Some(pair) = pair {
            let mut g = genesis.expect("有首提交");
            g.bind(&pair);
            let org = t.organizations.get(&g.world.org_id).ok_or_else(|| missing_fk("组织"))?;
            if t.worlds.values().filter(|w| w.org_id == org.org_id).count() >= org.quota_max_worlds.max(0) as usize {
                return Err(conflict("组织世界配额已满"));
            }
            if t.worlds.contains_key(&g.world.world_id) || t.matches.contains_key(&g.row.match_id) {
                return Err(conflict("预留ID冲突"));
            }
            let parts = g.participants(&pair);
            for p in parts {
                t.match_participants.insert((p.match_id, p.faction_id), p);
            }
            t.ranked_deadlines.insert(
                g.row.match_id,
                crate::ids::unix_now_s() + crate::ranked::READY_TIMEOUT_S,
            );
            decision = QueueDecision::Matched(g.row.match_id);
            t.world_commits
                .insert((g.commit.world_id, g.commit.world_revision), g.commit);
            t.worlds.insert(g.world.world_id, g.world);
            t.matches.insert(g.row.match_id, g.row);
        }
        if req.action != QueueAction::Read {
            t.ranked_tickets = tickets;
        }
        Ok(decision)
    }
    pub fn ranked_host(&self, world: WireUuid) -> StoreResult<Option<crate::ranked::HostLease>> {
        Ok(self.lock().ranked_hosts.get(&world).cloned())
    }
    pub fn claim_ranked_host(&self, c: crate::ranked::HostClaim) -> StoreResult<crate::ranked::HostLease> {
        use crate::ranked::*;
        let mut t = self.lock();
        let now = crate::ids::unix_now_s();
        let w = t.worlds.get(&c.world).ok_or_else(|| missing_fk("世界"))?;
        if w.game_mode != "PvP_Ranked"
            || !t
                .matches
                .values()
                .any(|m| m.world_id == c.world && !m.match_state.is_terminal())
        {
            return Err(conflict("排位已终局"));
        }
        if let Some(h) = t.ranked_hosts.get(&c.world) {
            if h.expires_at_s > now && h.fence == w.fencing_token {
                return Ok(h.clone());
            }
        }
        if w.latest_revision != c.revision || w.fencing_token != c.fence || c.fence >= i64::MAX as u64 {
            return Err(conflict("恢复基线过期，请刷新"));
        }
        let lease = HostLease {
            world_id: c.world,
            node: c.node,
            origin: c.origin,
            fence: c.fence + 1,
            expires_at_s: now + HOST_TTL_S,
        };
        let w = t.worlds.get_mut(&c.world).unwrap();
        w.fencing_token = lease.fence;
        w.lease_holder_node = Some(lease.node.clone());
        t.ranked_hosts.insert(c.world, lease.clone());
        Ok(lease)
    }
    pub fn renew_ranked_host(&self, world: WireUuid, fence: u64) -> StoreResult<bool> {
        let mut t = self.lock();
        let now = crate::ids::unix_now_s();
        let valid = t.worlds.get(&world).is_some_and(|w| w.fencing_token == fence)
            && t.matches
                .values()
                .any(|m| m.world_id == world && !m.match_state.is_terminal())
            && t.ranked_hosts
                .get(&world)
                .is_some_and(|h| h.fence == fence && h.expires_at_s > now);
        if valid {
            t.ranked_hosts.get_mut(&world).unwrap().expires_at_s = now + crate::ranked::HOST_TTL_S;
        }
        Ok(valid)
    }
    pub fn release_ranked_hosts(&self, node: String) -> StoreResult<()> {
        let mut t = self.lock();
        let worlds: Vec<_> = t
            .ranked_hosts
            .values()
            .filter(|h| h.node == node)
            .map(|h| h.world_id)
            .collect();
        for id in worlds {
            let fence = t.ranked_hosts[&id].fence;
            if let Some(w) = t.worlds.get_mut(&id) {
                if w.fencing_token == fence {
                    w.fencing_token = w.fencing_token.checked_add(1).ok_or_else(|| conflict("fence溢出"))?;
                    w.lease_holder_node = None;
                }
            }
            t.ranked_hosts.get_mut(&id).unwrap().expires_at_s = 0;
        }
        Ok(())
    }

    pub fn custom_live(&self, world: WireUuid, match_id: WireUuid, fence: u64) -> StoreResult<bool> {
        let t = self.lock();
        Ok(t.custom_rooms.get(&world).is_some_and(|r| {
            r.active
                && !r.closed
                && r.match_id == match_id
                && t.worlds.get(&world).is_some_and(|w| {
                    w.fencing_token == fence
                        && t.principals
                            .get(&w.owner_principal_id)
                            .is_some_and(|p| p.status == PrincipalStatus::Active)
                })
                && t.principals
                    .get(&r.guest_id)
                    .is_some_and(|p| p.status == PrincipalStatus::Active)
        }))
    }
    pub fn custom_room(&self, world: WireUuid) -> StoreResult<Option<super::CustomRoom>> {
        Ok(self.lock().custom_rooms.get(&world).cloned())
    }
    pub fn save_custom_room(
        &self,
        room: super::CustomRoom,
        expected: Option<WireUuid>,
        activation: Option<(MatchRow, Vec<ParticipantRow>)>,
    ) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.worlds.contains_key(&room.world_id) || !t.principals.contains_key(&room.guest_id) {
            return Err(missing_fk("自创世界或主体"));
        }
        if t.custom_rooms.get(&room.world_id).map(|r| r.version) != expected {
            return Err(conflict("房间版本冲突，请刷新"));
        }
        let next_fence = t.worlds[&room.world_id]
            .fencing_token
            .checked_add(1)
            .ok_or_else(|| conflict("fence溢出"))?;
        if let Some((row, parts)) = activation {
            for p in &parts {
                ensure_free(&t, p.principal_id)?;
            }
            if t.matches.contains_key(&row.match_id)
                || row.match_id != room.match_id
                || row.world_id != room.world_id
                || parts.len() != 2
                || parts
                    .iter()
                    .any(|p| p.match_id != row.match_id || !t.principals.contains_key(&p.principal_id))
            {
                return Err(conflict("冻结完整参赛席位失败"));
            }
            for p in parts {
                t.match_participants.insert((p.match_id, p.faction_id), p);
            }
            t.matches.insert(row.match_id, row);
        }
        if room.closed {
            t.worlds.get_mut(&room.world_id).unwrap().fencing_token = next_fence;
            if let Some(row) = t.matches.get_mut(&room.match_id) {
                if !row.match_state.is_terminal() {
                    row.match_state = super::MatchState::Aborted;
                }
            }
        }
        t.custom_rooms.insert(room.world_id, room);
        Ok(())
    }
    pub fn observer_grants(&self, world: WireUuid) -> StoreResult<Vec<super::ObserverGrant>> {
        Ok(self
            .lock()
            .observers
            .values()
            .filter(|g| g.world_id == world && g.active)
            .cloned()
            .collect())
    }
    pub fn set_observer_grant(&self, grant: super::ObserverGrant) -> StoreResult<()> {
        let mut t = self.lock();
        if !t.worlds.contains_key(&grant.world_id) || !t.principals.contains_key(&grant.principal_id) {
            return Err(missing_fk("观战世界或身份"));
        }
        if grant.active
            && !t
                .observers
                .get(&(grant.world_id, grant.principal_id))
                .is_some_and(|g| g.active)
            && t.observers
                .values()
                .filter(|g| g.world_id == grant.world_id && g.active)
                .count()
                >= 32
        {
            return Err(conflict("单世界最多32个有效观战准入"));
        }
        t.observers.insert((grant.world_id, grant.principal_id), grant);
        Ok(())
    }
    pub fn revoke_observer(&self, world: WireUuid, principal: u64, grant: WireUuid) -> StoreResult<bool> {
        let mut t = self.lock();
        let Some(row) = t.observers.get_mut(&(world, principal)) else {
            return Ok(false);
        };
        if row.grant_id != grant || !row.active {
            return Ok(false);
        }
        row.active = false; // 保留原授权对象；操作事实另入审计，不删除历史。
        Ok(true)
    }
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
        t.ranked_tickets.retain(|q| q.entry.principal_id != id);
        let ids: Vec<_> = t
            .match_participants
            .values()
            .filter(|p| p.principal_id == id)
            .map(|p| p.match_id)
            .collect();
        for mid in ids {
            let world = if let Some(m) = t.matches.get_mut(&mid) {
                if m.game_mode == "PvP_Ranked" && !m.match_state.is_terminal() {
                    m.match_state = super::MatchState::Aborted;
                    Some(m.world_id)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(world) = world {
                if let Some(w) = t.worlds.get_mut(&world) {
                    w.fencing_token = w.fencing_token.checked_add(1).ok_or_else(|| conflict("fence溢出"))?;
                }
            }
        }
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

    pub fn claim_world(&self, id: WireUuid, revision: u64, token: u64, holder: String) -> StoreResult<u64> {
        let mut tables = self.lock();
        let world = tables.worlds.get_mut(&id).ok_or_else(|| missing_fk("world_id"))?;
        if world.latest_revision != revision || world.fencing_token != token || token >= i64::MAX as u64 {
            return Err(conflict("宿主认领基线已过期"));
        }
        world.fencing_token = token + 1;
        world.lease_holder_node = Some(holder);
        Ok(token + 1)
    }

    pub fn append_commit(&self, commit: CommitRow, expected_fencing_token: u64) -> StoreResult<()> {
        let mut t = self.lock();
        let world = t.worlds.get(&commit.world_id).ok_or_else(|| missing_fk("world_id"))?;
        if world.fencing_token != expected_fencing_token
            || t.ranked_hosts
                .get(&commit.world_id)
                .is_some_and(|h| h.fence != expected_fencing_token || h.expires_at_s <= crate::ids::unix_now_s())
        {
            return Err(ApiError::new(
                RynvekErrorCode::StalePartitionFencingToken,
                "fencing_token 已过期：世界租约已被其他节点接管",
            ));
        }
        if commit.parent_revision != Some(world.latest_revision)
            || world.latest_revision.checked_add(1) != Some(commit.world_revision)
            || commit.tick < world.latest_tick
        {
            return Err(conflict("提交前驱或 Tick 已过期，拒绝覆盖权威世界"));
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
        for p in &participants {
            ensure_free(&t, p.principal_id)?;
        }
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
        self.update_match_guarded(row, None)
    }
    pub fn update_match_fenced(&self, row: MatchRow, fencing_token: u64) -> StoreResult<()> {
        self.update_match_guarded(row, Some(fencing_token))
    }
    fn update_match_guarded(&self, row: MatchRow, fencing_token: Option<u64>) -> StoreResult<()> {
        let mut t = self.lock();
        let world_id = t
            .matches
            .get(&row.match_id)
            .ok_or_else(|| missing_fk("match_id"))?
            .world_id;
        if fencing_token.is_some_and(|token| {
            !t.worlds.get(&world_id).is_some_and(|w| w.fencing_token == token)
                || t.ranked_hosts
                    .get(&world_id)
                    .is_some_and(|h| h.fence != token || h.expires_at_s <= crate::ids::unix_now_s())
        }) {
            return Err(conflict("旧宿主不得覆盖恢复后的对局状态"));
        }
        let match_id = row.match_id;
        if row.match_state == super::MatchState::Running || row.match_state.is_terminal() {
            t.ranked_deadlines.remove(&match_id);
        }
        *t.matches.get_mut(&match_id).expect("已校验") = row;
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

fn ensure_free(t: &Tables, pid: u64) -> StoreResult<()> {
    if t.ranked_tickets.iter().any(|q| q.entry.principal_id == pid)
        || t.match_participants
            .values()
            .any(|p| p.principal_id == pid && t.matches.get(&p.match_id).is_some_and(|m| !m.match_state.is_terminal()))
    {
        return Err(crate::ranked::occupied_error());
    }
    Ok(())
}

fn expire_reservations(t: &mut Tables, now: u64) -> StoreResult<()> {
    let ids: Vec<_> = t
        .ranked_deadlines
        .iter()
        .filter(|(_, deadline)| **deadline <= now)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        let row = t.matches.get_mut(&id).expect("预留有完整对局");
        if !row.match_state.is_terminal()
            && row.match_state != super::MatchState::Running
            && t.worlds.get(&row.world_id).is_some_and(|w| w.latest_revision == 0)
        {
            row.match_state = super::MatchState::Aborted;
            row.completed_at_unix_s = Some(now);
            let w = t.worlds.get_mut(&row.world_id).unwrap();
            w.fencing_token = w.fencing_token.checked_add(1).ok_or_else(|| conflict("fence溢出"))?;
        }
        t.ranked_deadlines.remove(&id);
    }
    Ok(())
}

#[cfg(test)]
mod ranked_time_tests {
    use super::*;
    use crate::ranked::*;
    #[tokio::test]
    async fn expired_ranked_host_cannot_renew_commit_or_finalize_and_unseated_reservation_expires() {
        let memory = MemoryStore::new();
        for id in [1, 2] {
            memory
                .insert_principal(PrincipalRow {
                    principal_id: id,
                    handle: format!("clock-{id}"),
                    kind: rynvek_security::PrincipalKindCode::Human,
                    ed25519_pubkey: [id as u8; 32],
                    email_hash: None,
                    status: PrincipalStatus::Active,
                })
                .unwrap();
            memory
                .insert_org(OrgRow {
                    org_id: id,
                    slug: format!("clock-{id}"),
                    display_name: "test".into(),
                    owner_principal_id: id,
                    quota_max_worlds: 10,
                    quota_storage_bytes: 1000000,
                })
                .unwrap();
        }
        let app = crate::app::AppState::new(
            crate::app::AppConfig::default(),
            super::super::Store::Memory(memory.clone()),
            rynvek_security::ServerSigningKey::from_seed(1, [9; 32]),
        )
        .unwrap();
        let req = |pid, action| QueueCommand {
            action,
            ticket: QueueTicket {
                league: League::Human,
                ruleset: crate::app::arena_ruleset_hash(),
                entry: crate::matchmaking::QueueEntry {
                    principal_id: pid,
                    rating: 1500.0,
                    deviation: 100.0,
                    enqueued_at_s: crate::ids::unix_now_s(),
                },
            },
        };
        memory.ranked_queue(req(1, QueueAction::Join), None).unwrap();
        memory.ranked_queue(req(2, QueueAction::Join), None).unwrap();
        let g = app.prepare_ranked_genesis().unwrap();
        let world = g.world.world_id;
        let QueueDecision::Matched(id) = memory.ranked_queue(req(2, QueueAction::Poll), Some(g)).unwrap() else {
            panic!("配对")
        };
        let w = memory.get_world(world).unwrap().unwrap();
        let lease = memory
            .claim_ranked_host(HostClaim {
                world,
                revision: 0,
                fence: w.fencing_token,
                node: "old".into(),
                origin: "http://localhost".into(),
            })
            .unwrap();
        // 只改测试夹具中的时钟边界；不是等30秒长稳，更不是PG真实时钟试验。
        memory.lock().ranked_hosts.get_mut(&world).unwrap().expires_at_s = crate::ids::unix_now_s() - 1;
        assert!(!memory.renew_ranked_host(world, lease.fence).unwrap());
        let mut row = memory.get_match(id).unwrap().unwrap().0;
        row.match_state = super::super::MatchState::Aborted;
        assert!(memory.update_match_fenced(row, lease.fence).is_err());
        let mut commit = memory.list_commits(world).unwrap()[0].clone();
        commit.parent_revision = Some(0);
        commit.world_revision = 1;
        assert!(memory.append_commit(commit, lease.fence).is_err());
        let new = memory
            .claim_ranked_host(HostClaim {
                world,
                revision: 0,
                fence: lease.fence,
                node: "new".into(),
                origin: "http://localhost:2".into(),
            })
            .unwrap();
        assert!(new.fence > lease.fence);
        memory.lock().ranked_deadlines.insert(id, crate::ids::unix_now_s() - 1);
        assert!(matches!(
            memory.ranked_queue(req(1, QueueAction::Read), None).unwrap(),
            QueueDecision::Idle
        ));
        assert_eq!(
            memory.get_match(id).unwrap().unwrap().0.match_state,
            super::super::MatchState::Aborted
        );
        assert!(!memory.renew_ranked_host(world, new.fence).unwrap());
    }
}
