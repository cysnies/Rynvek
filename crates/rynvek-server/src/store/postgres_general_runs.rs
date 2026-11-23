// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    general_runs::{GeneralRun, RunPermit},
    lab_jobs::{Claim, MAX_ACTIVE, MAX_JOBS, OwnerAction},
};
fn decode(r: &Row) -> StoreResult<GeneralRun> {
    serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| corrupt("耐久运行"))
}
async fn time(tx: &tokio_postgres::Transaction<'_>) -> StoreResult<u64> {
    Ok(tx
        .query_one("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint", &[])
        .await
        .map_err(db_err)?
        .get::<_, i64>(0) as u64)
}
async fn world(tx: &tokio_postgres::Transaction<'_>, id: WireUuid) -> StoreResult<WorldRow> {
    world_of(
        &tx.query_one(
            &format!("SELECT {WORLD_COLS} FROM worlds WHERE world_id=$1::text::uuid FOR UPDATE"),
            &[&id.to_string()],
        )
        .await
        .map_err(db_err)?,
    )
}
async fn owner(tx: &tokio_postgres::Transaction<'_>, w: &WorldRow, pid: u64) -> StoreResult<()> {
    if !w.is_controlled_general() || w.owner_principal_id != pid {
        return Err(crate::transactions::conflict("运行所有权失效"));
    }
    PostgresStore::editor_allowed(
        tx,
        w.world_id,
        &super::super::EditorPermit {
            principal_id: pid,
            grant_id: None,
            purpose: super::super::AdmissionPurpose::GeneralRuleDrive,
        },
    )
    .await
}
async fn save(tx: &tokio_postgres::Transaction<'_>, j: &GeneralRun) -> StoreResult<()> {
    tx.execute(
        "UPDATE general_runs SET row_json=$2::text::jsonb,status=$3,lease_until_s=$4 WHERE run_id=$1::text::uuid",
        &[
            &j.id,
            &serde_json::to_string(j).expect("定型运行"),
            &j.status,
            &i(j.lease_until_s),
        ],
    )
    .await
    .map_err(db_err)?;
    Ok(())
}
pub(super) async fn commit(
    tx: &tokio_postgres::Transaction<'_>,
    w: &WorldRow,
    c: &CommitRow,
    p: Option<RunPermit>,
) -> StoreResult<()> {
    let old=tx.query_opt("SELECT row_json::text FROM general_runs WHERE world_id=$1::text::uuid AND status IN ('queued','running','paused','blocked') FOR UPDATE",&[&w.world_id.to_string()]).await.map_err(db_err)?.as_ref().map(decode).transpose()?;
    match (old, p) {
        (Some(mut j), Some(p)) => {
            owner(tx, w, j.owner).await?;
            j.commit(&p, w, c, time(tx).await?)?;
            save(tx, &j).await
        }
        (None, None) => Ok(()),
        _ => Err(crate::transactions::conflict("耐久意图保留世界，或旧运行已暂停/取消")),
    }
}
impl PostgresStore {
    pub async fn general_run_submit(&self, mut j: GeneralRun, host: u64) -> StoreResult<GeneralRun> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let w = world(&tx, j.world_id).await?;
        // 同一owner跨世界配额串行化；其余路径统一世界→主体→运行的提交锁顺序。
        tx.query_one(
            "SELECT principal_id FROM principals WHERE principal_id=$1 FOR UPDATE",
            &[&i(j.owner)],
        )
        .await
        .map_err(db_err)?;
        owner(&tx, &w, j.owner).await?;
        if let Some(r) = tx
            .query_opt(
                "SELECT row_json::text FROM general_runs WHERE owner_id=$1 AND submission_id=$2::text::uuid",
                &[&i(j.owner), &j.spec.submission_id],
            )
            .await
            .map_err(db_err)?
        {
            let old = decode(&r)?;
            return if old.world_id == j.world_id && old.spec == j.spec {
                Ok(old)
            } else {
                Err(crate::transactions::conflict("提交键绑定不同运行"))
            };
        }
        let n=tx.query_one("SELECT count(*),count(*) FILTER(WHERE status IN ('queued','running','paused','blocked')) FROM general_runs WHERE owner_id=$1",&[&i(j.owner)]).await.map_err(db_err)?;
        if n.get::<_, i64>(0) >= MAX_JOBS as i64 || n.get::<_, i64>(1) >= MAX_ACTIVE as i64 {
            return Err(crate::transactions::conflict("原作业配额已满"));
        }
        j.baseline(&w)?;
        ranked::check_lease(&tx, j.world_id, host).await?;
        if w.fencing_token != host {
            return Err(crate::transactions::conflict("启动宿主已失效"));
        }
        j.storage_backend = "postgres".into();
        j.deadline_s = time(&tx).await? + j.spec.wall_seconds as u64;
        tx.execute("INSERT INTO general_runs(run_id,world_id,owner_id,submission_id,status,row_json) VALUES($1::text::uuid,$2::text::uuid,$3,$4::text::uuid,$5,$6::text::jsonb)",&[&j.id,&j.world_id.to_string(),&i(j.owner),&j.spec.submission_id,&j.status,&serde_json::to_string(&j).expect("定型运行")]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(j)
    }
    pub async fn general_run_submission(&self, owner: u64, submission: String) -> StoreResult<Option<GeneralRun>> {
        self.client
            .lock()
            .await
            .query_opt(
                "SELECT row_json::text FROM general_runs WHERE owner_id=$1 AND submission_id=$2::text::uuid",
                &[&i(owner), &submission],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(decode)
            .transpose()
    }
    pub async fn general_run_get(&self, owner: u64, id: String) -> StoreResult<Option<GeneralRun>> {
        self.client
            .lock()
            .await
            .query_opt(
                "SELECT row_json::text FROM general_runs WHERE owner_id=$1 AND run_id=$2::text::uuid",
                &[&i(owner), &id],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(decode)
            .transpose()
    }
    pub async fn general_run_reserved(&self, world: WireUuid) -> StoreResult<bool> {
        Ok(self.client.lock().await.query_opt("SELECT run_id FROM general_runs WHERE world_id=$1::text::uuid AND status IN ('queued','running','paused','blocked')",&[&world.to_string()]).await.map_err(db_err)?.is_some())
    }
    pub async fn general_run_claim(&self, node: String) -> StoreResult<Option<GeneralRun>> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let now = time(&tx).await?;
        let r=tx.query_opt("SELECT row_json::text FROM general_runs WHERE status='queued' OR (status='running' AND lease_until_s<=$1) ORDER BY run_id FOR UPDATE SKIP LOCKED LIMIT 1",&[&i(now)]).await.map_err(db_err)?;
        let Some(r) = r else { return Ok(None) };
        let mut j = decode(&r)?;
        let claimed = j.claim(node, now)?;
        save(&tx, &j).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(claimed.then_some(j))
    }
    pub async fn general_run_control(
        &self,
        world_id: WireUuid,
        pid: u64,
        id: String,
        op: OwnerAction,
    ) -> StoreResult<GeneralRun> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let w = world(&tx, world_id).await?;
        owner(&tx, &w, pid).await?;
        let mut j=tx.query_opt("SELECT row_json::text FROM general_runs WHERE run_id=$1::text::uuid AND owner_id=$2 AND world_id=$3::text::uuid FOR UPDATE",&[&id,&i(pid),&world_id.to_string()]).await.map_err(db_err)?.as_ref().map(decode).transpose()?.ok_or_else(||ApiError::not_found("运行不存在"))?;
        if op.action == "resume" && !j.operations.contains_key(&op.operation_id) {
            j.baseline(&w)?;
        }
        j.control(op, time(&tx).await?)?;
        save(&tx, &j).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(j)
    }
    pub async fn general_run_worker(&self, claim: Claim, op: crate::lab_jobs::WorkerAction) -> StoreResult<GeneralRun> {
        let Some(old) = self.general_run_get(claim.owner, claim.id.clone()).await? else {
            return Err(crate::transactions::conflict("运行不存在"));
        };
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let w = world(&tx, old.world_id).await?;
        if matches!(op, crate::lab_jobs::WorkerAction::Heartbeat) {
            owner(&tx, &w, claim.owner).await?
        }
        let mut j = decode(
            &tx.query_one(
                "SELECT row_json::text FROM general_runs WHERE run_id=$1::text::uuid FOR UPDATE",
                &[&claim.id],
            )
            .await
            .map_err(db_err)?,
        )?;
        let now = time(&tx).await?;
        match op {
            crate::lab_jobs::WorkerAction::Fail => j.stop(&claim, now)?,
            crate::lab_jobs::WorkerAction::Release => j.release(&claim, now)?,
            crate::lab_jobs::WorkerAction::Heartbeat => j.heartbeat(&claim, now)?,
            _ => return Err(crate::transactions::conflict("该作业动作不属于General运行")),
        }
        save(&tx, &j).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(j)
    }
}
