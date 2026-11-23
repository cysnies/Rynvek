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
    agent_runs: BTreeMap<String, crate::agent_runs::AgentRun>,
    lab_jobs: BTreeMap<String, crate::lab_jobs::LabJob>,
    private_visuals: BTreeMap<(u64, String, String), super::PrivateArtifactRow>,
    private_audios: BTreeMap<(u64, String, String), super::PrivateArtifactRow>,
    private_rules: BTreeMap<(u64, String, String), super::PrivateArtifactRow>,
    private_agents: BTreeMap<(u64, String, String), super::PrivateArtifactRow>,
    ranked_deadlines: BTreeMap<WireUuid, u64>,
    ranked_tickets: Vec<crate::ranked::QueueTicket>,
    ranked_hosts: BTreeMap<WireUuid, crate::ranked::HostLease>,
    principals: BTreeMap<u64, PrincipalRow>,
    organizations: BTreeMap<u64, OrgRow>,
    worlds: BTreeMap<WireUuid, WorldRow>,
    custom_rooms: BTreeMap<WireUuid, super::CustomRoom>,
    observers: BTreeMap<(WireUuid, u64), super::ObserverGrant>,
    editors: BTreeMap<(WireUuid,u64,super::AdmissionPurpose), super::EditorGrant>,
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

impl Tables {
    fn artifacts(&self,kind:super::PrivateArtifactKind)->&BTreeMap<(u64,String,String),super::PrivateArtifactRow> {
        match kind {super::PrivateArtifactKind::Agent=>&self.private_agents,super::PrivateArtifactKind::Audio=>&self.private_audios,super::PrivateArtifactKind::Visual=>&self.private_visuals,super::PrivateArtifactKind::Rule=>&self.private_rules}
    }
    fn artifacts_mut(&mut self,kind:super::PrivateArtifactKind)->&mut BTreeMap<(u64,String,String),super::PrivateArtifactRow> {
        match kind {super::PrivateArtifactKind::Agent=>&mut self.private_agents,super::PrivateArtifactKind::Audio=>&mut self.private_audios,super::PrivateArtifactKind::Visual=>&mut self.private_visuals,super::PrivateArtifactKind::Rule=>&mut self.private_rules}
    }
}

impl MemoryStore {
    pub fn agent_run_create(&self, row: crate::agent_runs::AgentRun) -> StoreResult<crate::agent_runs::AgentRun> {
        let mut t = self.lock();
        let owner = row.owner_id.parse::<u64>().map_err(|_| conflict("非法运行owner"))?;
        if !t
            .principals
            .get(&owner)
            .is_some_and(|p| p.status == PrincipalStatus::Active)
        {
            return Err(conflict("owner失效"));
        }
        let own: Vec<_> = t.agent_runs.values().filter(|r| r.owner_id == row.owner_id).collect();
        if let Some(old) = own.iter().find(|r| r.submission_id == row.submission_id) {
            return if old.spec_digest == row.spec_digest {
                Ok((*old).clone())
            } else {
                Err(conflict("提交键已绑定其它运行规格"))
            };
        }
        if own.len() >= 64 || own.iter().any(|r| r.world_id == row.world_id && r.active()) {
            return Err(conflict("运行配额已满或此世界已有未关闭运行"));
        }
        t.agent_runs.insert(row.run_id.clone(), row.clone());
        Ok(row)
    }
    pub fn agent_run_get(&self, owner: u64, id: String) -> StoreResult<Option<crate::agent_runs::AgentRun>> {
        Ok(self
            .lock()
            .agent_runs
            .get(&id)
            .filter(|r| r.owner_id == owner.to_string())
            .cloned())
    }
    pub fn agent_run_list(&self, owner: u64) -> StoreResult<Vec<crate::agent_runs::AgentRun>> {
        Ok(self
            .lock()
            .agent_runs
            .values()
            .rev()
            .filter(|r| r.owner_id == owner.to_string())
            .cloned()
            .collect())
    }
    pub fn agent_run_change(
        &self,
        owner: u64,
        id: String,
        change: crate::agent_runs::Change,
    ) -> StoreResult<(crate::agent_runs::AgentRun, bool)> {
        let mut t = self.lock();
        if !t
            .principals
            .get(&owner)
            .is_some_and(|p| p.status == PrincipalStatus::Active)
        {
            return Err(conflict("owner失效"));
        }
        let mut row = t
            .agent_runs
            .get(&id)
            .filter(|r| r.owner_id == owner.to_string())
            .cloned()
            .ok_or_else(|| ApiError::not_found("运行不存在"))?;
        let old = row.revision;
        row.apply(change, crate::ids::unix_now_s())?;
        if row.active()
            && t.agent_runs
                .values()
                .any(|r| r.run_id != id && r.owner_id == row.owner_id && r.world_id == row.world_id && r.active())
        {
            return Err(conflict("此世界已有其它未关闭运行"));
        }
        let changed = old != row.revision;
        t.agent_runs.insert(id, row.clone());
        Ok((row, changed))
    }

    pub fn lab_submit(&self, job: crate::lab_jobs::LabJob) -> StoreResult<crate::lab_jobs::Experiment> {
        let mut t = self.lock();
        let owner = job.experiment.owner_principal_id;
        if !t
            .principals
            .get(&owner)
            .is_some_and(|p| p.status == PrincipalStatus::Active)
        {
            return Err(conflict("作业所有者失效"));
        }
        if let Some(old) = t.lab_jobs.values().find(|j| {
            j.experiment.owner_principal_id == owner && j.experiment.submission_id == job.experiment.submission_id
        }) {
            return if old.experiment.spec_digest == job.experiment.spec_digest {
                Ok(old.experiment.clone())
            } else {
                Err(conflict("提交键已绑定不同规格"))
            };
        }
        let own: Vec<_> = t
            .lab_jobs
            .values()
            .filter(|j| j.experiment.owner_principal_id == owner)
            .collect();
        if own.len() >= crate::lab_jobs::MAX_JOBS
            || own.iter().filter(|j| j.active()).count() >= crate::lab_jobs::MAX_ACTIVE
        {
            return Err(conflict("Lab持久作业配额已满"));
        }
        let e = job.experiment.clone();
        t.lab_jobs.insert(e.exp_id.clone(), job);
        Ok(e)
    }
    pub fn lab_get(&self, owner: u64, id: String) -> StoreResult<Option<crate::lab_jobs::LabJob>> {
        Ok(self
            .lock()
            .lab_jobs
            .get(&id)
            .filter(|j| j.experiment.owner_principal_id == owner)
            .cloned())
    }
    pub fn lab_list(&self, owner: u64, limit: u32, offset: u32) -> StoreResult<Vec<crate::lab_jobs::Experiment>> {
        Ok(self
            .lock()
            .lab_jobs
            .values()
            .rev()
            .filter(|j| j.experiment.owner_principal_id == owner)
            .skip(offset as usize)
            .take(limit as usize)
            .map(|j| j.experiment.clone())
            .collect())
    }
    pub fn lab_claim(&self, node: String) -> StoreResult<Option<crate::lab_jobs::LabJob>> {
        let mut t = self.lock();
        let now = crate::ids::unix_now_s();
        let id = t
            .lab_jobs
            .iter()
            .find(|(_, j)| {
                j.eligible(now)
                    && t.principals
                        .get(&j.experiment.owner_principal_id)
                        .is_some_and(|p| p.status == PrincipalStatus::Active)
            })
            .map(|(k, _)| k.clone());
        let Some(id) = id else { return Ok(None) };
        let j = t.lab_jobs.get_mut(&id).unwrap();
        if j.claim(node, now)? {
            Ok(Some(j.clone()))
        } else {
            Ok(None)
        }
    }
    pub fn lab_control(
        &self,
        owner: u64,
        id: String,
        op: crate::lab_jobs::OwnerAction,
    ) -> StoreResult<crate::lab_jobs::Experiment> {
        let mut t = self.lock();
        let active = t
            .principals
            .get(&owner)
            .is_some_and(|p| p.status == PrincipalStatus::Active);
        let old = t
            .lab_jobs
            .get(&id)
            .filter(|j| j.experiment.owner_principal_id == owner && active)
            .ok_or_else(|| ApiError::not_found("实验不存在"))?;
        if op.action == "retry"
            && t.lab_jobs
                .values()
                .filter(|j| j.experiment.owner_principal_id == owner && j.active())
                .count()
                >= crate::lab_jobs::MAX_ACTIVE
            && !old.operations.contains_key(&op.operation_id)
        {
            return Err(conflict("Lab活动作业配额已满"));
        }
        let mut job = old.clone();
        job.owner_action(op, crate::ids::unix_now_s())?;
        let exp = job.experiment.clone();
        t.lab_jobs.insert(id, job);
        Ok(exp)
    }
    pub fn lab_worker(
        &self,
        claim: crate::lab_jobs::Claim,
        action: crate::lab_jobs::WorkerAction,
    ) -> StoreResult<crate::lab_jobs::LabJob> {
        let mut t = self.lock();
        if !t
            .principals
            .get(&claim.owner)
            .is_some_and(|p| p.status == PrincipalStatus::Active)
        {
            return Err(conflict("所有者已失效"));
        }
        let mut job = t
            .lab_jobs
            .get(&claim.id)
            .cloned()
            .ok_or_else(|| conflict("Lab租约不存在"))?;
        job.worker_action(&claim, action, crate::ids::unix_now_s())?;
        t.lab_jobs.insert(claim.id, job.clone());
        Ok(job)
    }

    pub fn put_private_artifact(&self, kind:super::PrivateArtifactKind, row: super::PrivateArtifactRow) -> StoreResult<()> {
        let mut t = self.lock();
        let key = (row.owner_id, row.package_id.clone(), row.version.clone());
        if !t
            .principals
            .get(&row.owner_id)
            .is_some_and(|p| p.status == PrincipalStatus::Active && p.ed25519_pubkey == row.publisher_key)
        {
            return Err(conflict("发布者已失效"));
        }
        if t.artifacts_mut(kind).contains_key(&key) {
            return Err(conflict("私有版本不可覆盖，包括已撤销版本"));
        }
        if t.artifacts_mut(kind).keys().filter(|(o, _, _)| *o == row.owner_id).count() >= 256 {
            return Err(conflict("当前私有registry每主体最多256个不可变版本"));
        }
        for d in &row.dependencies {
            if !t
                .artifacts(kind)
                .get(&(row.owner_id, d.id.clone(), d.version.clone()))
                .is_some_and(|p| !p.revoked && crate::ids::hex(&p.cid) == d.blake3_256)
            {
                return Err(conflict("私有依赖已改变/撤销"));
            }
        }
        t.artifacts_mut(kind).insert(key, row);
        Ok(())
    }
    pub fn private_artifact(
        &self,
        kind:super::PrivateArtifactKind,
        owner: u64,
        id: String,
        version: String,
    ) -> StoreResult<Option<super::PrivateArtifactRow>> {
        Ok(self.lock().artifacts(kind).get(&(owner, id, version)).cloned())
    }
    pub fn private_artifacts(&self, kind:super::PrivateArtifactKind, owner: u64) -> StoreResult<Vec<super::PrivateArtifactRow>> {
        Ok(self
            .lock()
            .artifacts(kind)
            .values()
            .filter(|p| p.owner_id == owner)
            .cloned()
            .collect())
    }
    pub fn revoke_private_artifact(&self, kind:super::PrivateArtifactKind, owner: u64, id: String, version: String) -> StoreResult<bool> {
        let mut t = self.lock();
        let Some(row) = t.artifacts_mut(kind).get_mut(&(owner, id, version)) else {
            return Ok(false);
        };
        row.revoked = true;
        Ok(true)
    }

    pub fn put_private_agent(&self,row:super::PrivateArtifactRow)->StoreResult<()> {self.put_private_artifact(super::PrivateArtifactKind::Agent,row)}
    pub fn private_agent(&self,owner:u64,id:String,version:String)->StoreResult<Option<super::PrivateArtifactRow>> {self.private_artifact(super::PrivateArtifactKind::Agent,owner,id,version)}
    pub fn private_agents(&self,owner:u64)->StoreResult<Vec<super::PrivateArtifactRow>> {self.private_artifacts(super::PrivateArtifactKind::Agent,owner)}
    pub fn revoke_private_agent(&self,owner:u64,id:String,version:String)->StoreResult<bool> {self.revoke_private_artifact(super::PrivateArtifactKind::Agent,owner,id,version)}
    pub fn put_private_audio(&self,row:super::PrivateArtifactRow)->StoreResult<()> {self.put_private_artifact(super::PrivateArtifactKind::Audio,row)}
    pub fn private_audio(&self,owner:u64,id:String,version:String)->StoreResult<Option<super::PrivateArtifactRow>> {self.private_artifact(super::PrivateArtifactKind::Audio,owner,id,version)}
    pub fn private_audios(&self,owner:u64)->StoreResult<Vec<super::PrivateArtifactRow>> {self.private_artifacts(super::PrivateArtifactKind::Audio,owner)}
    pub fn revoke_private_audio(&self,owner:u64,id:String,version:String)->StoreResult<bool> {self.revoke_private_artifact(super::PrivateArtifactKind::Audio,owner,id,version)}
    pub fn put_private_visual(&self,row:super::PrivateArtifactRow)->StoreResult<()> {self.put_private_artifact(super::PrivateArtifactKind::Visual,row)}
    pub fn private_visual(&self,owner:u64,id:String,version:String)->StoreResult<Option<super::PrivateArtifactRow>> {self.private_artifact(super::PrivateArtifactKind::Visual,owner,id,version)}
    pub fn private_visuals(&self,owner:u64)->StoreResult<Vec<super::PrivateArtifactRow>> {self.private_artifacts(super::PrivateArtifactKind::Visual,owner)}
    pub fn revoke_private_visual(&self,owner:u64,id:String,version:String)->StoreResult<bool> {self.revoke_private_artifact(super::PrivateArtifactKind::Visual,owner,id,version)}

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
        if !w.is_controlled_general() && (w.game_mode != "PvP_Ranked"
            || !t.matches.values().any(|m| m.world_id == c.world && !m.match_state.is_terminal())) {
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
            && (t.worlds.get(&world).is_some_and(|w| w.is_controlled_general()) || t.matches
                .values().any(|m| m.world_id == world && !m.match_state.is_terminal()))
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
    fn editor_allowed(t:&Tables,world:WireUuid,p:&super::EditorPermit)->StoreResult<()> {
        let w=t.worlds.get(&world).ok_or_else(||ApiError::not_found("创作准入不存在"))?;
        if p.purpose==super::AdmissionPurpose::GeneralRuleDrive && (!w.is_controlled_general() || p.principal_id!=w.owner_principal_id && !t.principals.get(&p.principal_id).is_some_and(|r|r.kind==rynvek_security::PrincipalKindCode::Human)){return Err(ApiError::forbidden("准入目的与World不符"));}
        let active=[p.principal_id,w.owner_principal_id].iter().all(|id|t.principals.get(id).is_some_and(|r|r.status==PrincipalStatus::Active));
        let allowed=if p.principal_id==w.owner_principal_id {p.grant_id.is_none()}else{
            w.visibility==rynvek_security::Visibility::Private && t.editors.get(&(world,p.principal_id,p.purpose)).is_some_and(|g|g.active&&Some(g.grant_id)==p.grant_id)
        };
        if !active||!allowed {return Err(ApiError::forbidden("创作准入已撤销或身份失效"));} Ok(())
    }
    pub fn check_editor(&self,world:WireUuid,p:super::EditorPermit)->StoreResult<()> {Self::editor_allowed(&self.lock(),world,&p)}
    pub fn editor_grants(&self,world:WireUuid)->StoreResult<Vec<super::EditorGrant>> {Ok(self.lock().editors.values().filter(|g|g.world_id==world).cloned().collect())}
    pub fn change_editor(&self,owner:u64,g:super::EditorGrant,expected:Option<WireUuid>)->StoreResult<()> {
        let mut t=self.lock();let w=t.worlds.get(&g.world_id).ok_or_else(||ApiError::not_found("世界不存在"))?;
        if w.owner_principal_id!=owner||w.visibility!=rynvek_security::Visibility::Private||g.principal_id==owner {return Err(ApiError::forbidden("仅私有世界所有者可授权他人创作"));}
        if ![owner,g.principal_id].iter().all(|id|t.principals.get(id).is_some_and(|p|p.status==PrincipalStatus::Active)){return Err(ApiError::forbidden("正式身份不可用"));}
        if g.purpose==super::AdmissionPurpose::GeneralRuleDrive && (!w.is_controlled_general() || !t.principals.get(&g.principal_id).is_some_and(|p|p.kind==rynvek_security::PrincipalKindCode::Human)){return Err(ApiError::forbidden("General驱动须受控私有世界与正式人类主体"));}
        let old=t.editors.get(&(g.world_id,g.principal_id,g.purpose));
        if old.map(|x|x.grant_id)!=expected {return Err(conflict("准入版本改变，请刷新"));}
        let all:Vec<_>=t.editors.values().filter(|x|x.world_id==g.world_id&&x.purpose==g.purpose).collect();
        if old.is_none()&&all.len()>=64 || g.active&&all.iter().filter(|x|x.active&&x.principal_id!=g.principal_id).count()>=16 {return Err(conflict("创作准入配额已满"));}
        t.editors.insert((g.world_id,g.principal_id,g.purpose),g);Ok(())
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

    pub fn append_commit(&self, commit: CommitRow, expected_fencing_token: u64) -> StoreResult<()> {self.append_commit_checked(commit,expected_fencing_token,vec![])}
    pub fn append_commit_checked(&self, commit:CommitRow,expected_fencing_token:u64,permits:Vec<super::EditorPermit>)->StoreResult<()> {
        self.append_commit_guarded(commit,expected_fencing_token,permits,vec![])
    }
    pub fn append_commit_guarded(&self,commit:CommitRow,expected_fencing_token:u64,permits:Vec<super::EditorPermit>,rules:Vec<super::RulePermit>)->StoreResult<()> {
        let mut t = self.lock();
        for permit in &permits {Self::editor_allowed(&t,commit.world_id,permit)?;}
        for rule in rules {
            let owner=t.worlds.get(&commit.world_id).ok_or_else(||missing_fk("世界"))?.owner_principal_id;
            if owner!=rule.owner_id || !t.principals.get(&owner).is_some_and(|p|p.status==PrincipalStatus::Active) {return Err(conflict("规则主体失效"));}
            for pin in rule.pins {
                if !t.private_rules.get(&(owner,pin.id,pin.version)).is_some_and(|r|!r.revoked && crate::ids::hex(&r.cid)==pin.blake3_256 && t.principals.get(&owner).is_some_and(|p|p.ed25519_pubkey==r.publisher_key)) {return Err(conflict("规则精确依赖已撤销/改变"));}
            }
        }

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

#[cfg(test)]
mod general_host_time_tests {
    use super::*;
    use crate::{
        app::{AppConfig, AppState},
        ranked::HostClaim,
        store::{EditorPermit, Store},
    };
    #[tokio::test]
    async fn general_host_keeps_original_expiry_fence_and_checked_owner_commit() {
        let memory = MemoryStore::new();
        let app = AppState::new(
            AppConfig::default(),
            Store::Memory(memory.clone()),
            rynvek_security::ServerSigningKey::from_seed(1, [9; 32]),
        )
        .unwrap();
        app.register_principal(
            1,
            "general-clock".into(),
            rynvek_security::PrincipalKindCode::Human,
            [1; 32],
        )
        .await
        .unwrap();
        let id = crate::ids::new_uuid_v7();
        let row = WorldRow {
            world_id: id,
            org_id: 1,
            owner_principal_id: 1,
            title: "时钟夹具".into(),
            world_profile: "General_Circle_v1".into(),
            game_mode: "General_Controlled".into(),
            execution_profile: "D0_Strict".into(),
            visibility: rynvek_security::Visibility::Private,
            parent_world_id: None,
            forked_at_revision: None,
            latest_tick: 0,
            latest_revision: 0,
            latest_state_root: [0; 32],
            lease_holder_node: None,
            fencing_token: 1,
        };
        let genesis = CommitRow {
            world_id: id,
            world_revision: 0,
            tick: 0,
            parent_revision: None,
            state_root_blake3: [0; 32],
            journal_segment_cid: [0; 32],
            snapshot_cid: None,
        };
        memory.insert_world(row, genesis.clone()).unwrap();
        let claim = |fence, node: &str| HostClaim {
            world: id,
            revision: 0,
            fence,
            node: node.into(),
            origin: "http://localhost".into(),
        };
        let h = memory.claim_ranked_host(claim(1, "old")).unwrap();
        assert_eq!(memory.claim_ranked_host(claim(1, "peer")).unwrap().node, "old");
        assert!(memory.renew_ranked_host(id, h.fence).unwrap());
        // 只改变内存测试时钟边界，不声称真实30秒、PG或多机故障实证。
        memory.lock().ranked_hosts.get_mut(&id).unwrap().expires_at_s = crate::ids::unix_now_s() - 1;
        assert!(!memory.renew_ranked_host(id, h.fence).unwrap());
        let mut commit = genesis;
        commit.parent_revision = Some(0);
        commit.world_revision = 1;
        commit.tick = 1;
        let owner = EditorPermit { purpose: crate::store::AdmissionPurpose::GeneralRuleDrive,
            principal_id: 1,
            grant_id: None,
        };
        assert!(
            memory
                .append_commit_checked(commit.clone(), h.fence, vec![owner.clone()])
                .is_err()
        );
        let new = memory.claim_ranked_host(claim(h.fence, "new")).unwrap();
        assert!(new.fence > h.fence);
        assert!(
            memory
                .append_commit_checked(commit.clone(), h.fence, vec![owner.clone()])
                .is_err()
        );
        memory.tombstone_principal(1).unwrap();
        assert!(memory.append_commit_checked(commit, new.fence, vec![owner]).is_err());
        assert_eq!(memory.get_world(id).unwrap().unwrap().latest_revision, 0);
        assert_eq!(memory.list_commits(id).unwrap().len(), 1);
    }
}
