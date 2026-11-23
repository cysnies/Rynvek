// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    general_runs::GeneralRun,
    lab_jobs::{Claim, MAX_ACTIVE, MAX_JOBS, OwnerAction},
};
fn owner(t: &Tables, world: WireUuid, pid: u64) -> StoreResult<()> {
    let w = t.worlds.get(&world).ok_or_else(|| missing_fk("运行世界"))?;
    if !w.is_controlled_general() || w.owner_principal_id != pid {
        return Err(conflict("运行所有权失效"));
    }
    MemoryStore::editor_allowed(
        t,
        world,
        &super::super::EditorPermit {
            principal_id: pid,
            grant_id: None,
            purpose: super::super::AdmissionPurpose::GeneralRuleDrive,
        },
    )
}
impl MemoryStore {
    pub fn general_run_submit(&self, mut j: GeneralRun, host: u64) -> StoreResult<GeneralRun> {
        let mut t = self.lock();
        owner(&t, j.world_id, j.owner)?;
        if let Some(old) = t
            .general_runs
            .values()
            .find(|o| o.owner == j.owner && o.spec.submission_id == j.spec.submission_id)
        {
            return if old.world_id == j.world_id && old.spec == j.spec {
                Ok(old.clone())
            } else {
                Err(conflict("提交键已绑定不同运行"))
            };
        }
        let own: Vec<_> = t.general_runs.values().filter(|o| o.owner == j.owner).collect();
        if own.len() >= MAX_JOBS
            || own.iter().filter(|o| o.reserved()).count() >= MAX_ACTIVE
            || t.general_runs
                .values()
                .any(|o| o.world_id == j.world_id && o.reserved())
        {
            return Err(conflict("世界已有运行或原作业配额已满"));
        }
        let w = t.worlds.get(&j.world_id).expect("已查世界");
        j.baseline(w)?;
        if w.fencing_token != host
            || !t
                .ranked_hosts
                .get(&j.world_id)
                .is_some_and(|h| h.fence == host && h.expires_at_s > crate::ids::unix_now_s())
        {
            return Err(conflict("启动宿主已失效"));
        }
        j.storage_backend = "memory".into();
        j.deadline_s = crate::ids::unix_now_s() + j.spec.wall_seconds as u64;
        t.general_runs.insert(j.id.clone(), j.clone());
        Ok(j)
    }
    pub fn general_run_submission(&self, owner: u64, submission: String) -> StoreResult<Option<GeneralRun>> {
        Ok(self
            .lock()
            .general_runs
            .values()
            .find(|j| j.owner == owner && j.spec.submission_id == submission)
            .cloned())
    }
    pub fn general_run_get(&self, owner: u64, id: String) -> StoreResult<Option<GeneralRun>> {
        Ok(self.lock().general_runs.get(&id).filter(|j| j.owner == owner).cloned())
    }
    pub fn general_run_reserved(&self, world: WireUuid) -> StoreResult<bool> {
        Ok(self
            .lock()
            .general_runs
            .values()
            .any(|j| j.world_id == world && j.reserved()))
    }
    pub fn general_run_claim(&self, node: String) -> StoreResult<Option<GeneralRun>> {
        let mut t = self.lock();
        let now = crate::ids::unix_now_s();
        let next = t
            .general_runs
            .values()
            .find(|j| j.status == "queued" || j.status == "running" && j.lease_until_s <= now)
            .map(|j| j.id.clone());
        let Some(id) = next else { return Ok(None) };
        let j = t.general_runs.get_mut(&id).expect("已选运行");
        let claimed = j.claim(node, now)?;
        Ok(claimed.then(|| j.clone()))
    }
    pub fn general_run_control(
        &self,
        world: WireUuid,
        pid: u64,
        id: String,
        op: OwnerAction,
    ) -> StoreResult<GeneralRun> {
        let mut t = self.lock();
        owner(&t, world, pid)?;
        let mut j = t
            .general_runs
            .get(&id)
            .filter(|j| j.world_id == world && j.owner == pid)
            .cloned()
            .ok_or_else(|| ApiError::not_found("运行不存在"))?;
        if op.action == "resume" && !j.operations.contains_key(&op.operation_id) {
            j.baseline(t.worlds.get(&world).expect("已查世界"))?;
        }
        j.control(op, crate::ids::unix_now_s())?;
        t.general_runs.insert(id, j.clone());
        Ok(j)
    }
    pub fn general_run_worker(&self, c: Claim, op: crate::lab_jobs::WorkerAction) -> StoreResult<GeneralRun> {
        let mut t = self.lock();
        let mut j = t
            .general_runs
            .get(&c.id)
            .cloned()
            .ok_or_else(|| conflict("运行不存在"))?;
        let now = crate::ids::unix_now_s();
        match op {
            crate::lab_jobs::WorkerAction::Fail => j.stop(&c, now)?,
            crate::lab_jobs::WorkerAction::Release => j.release(&c, now)?,
            crate::lab_jobs::WorkerAction::Heartbeat => {
                owner(&t, j.world_id, c.owner)?;
                j.heartbeat(&c, now)?
            }
            _ => return Err(conflict("该作业动作不属于General运行")),
        }
        t.general_runs.insert(j.id.clone(), j.clone());
        Ok(j)
    }
}
