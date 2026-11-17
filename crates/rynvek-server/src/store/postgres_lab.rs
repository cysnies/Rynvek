// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::lab_jobs::{Claim, Experiment, LabJob, MAX_ACTIVE, MAX_JOBS, OwnerAction, WorkerAction};
fn lab_of(r: &Row) -> StoreResult<LabJob> {
    serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| corrupt("Lab作业"))
}
async fn now(tx: &tokio_postgres::Transaction<'_>) -> StoreResult<u64> {
    Ok(tx
        .query_one("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint", &[])
        .await
        .map_err(db_err)?
        .get::<_, i64>(0) as u64)
}
async fn owner_lock(tx: &tokio_postgres::Transaction<'_>, owner: u64) -> StoreResult<()> {
    let p = tx
        .query_opt(
            "SELECT status FROM principals WHERE principal_id=$1 FOR UPDATE",
            &[&i(owner)],
        )
        .await
        .map_err(db_err)?;
    if !p.is_some_and(|r| r.get::<_, i16>(0) == 0) {
        return Err(crate::lab_jobs::conflict("作业所有者失效"));
    }
    Ok(())
}
async fn save(tx: &tokio_postgres::Transaction<'_>, j: &LabJob) -> StoreResult<()> {
    tx.execute(
        "UPDATE lab_jobs SET row_json=$2::text::jsonb,status=$3,lease_until_s=$4 WHERE exp_id=$1::text::uuid",
        &[
            &j.experiment.exp_id,
            &serde_json::to_string(j).expect("作业可序列化"),
            &j.experiment.status,
            &j.experiment.lease_expires_at_unix_s.map(i),
        ],
    )
    .await
    .map_err(db_err)?;
    Ok(())
}
impl PostgresStore {
    pub async fn lab_submit(&self, job: LabJob) -> StoreResult<Experiment> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let e = &job.experiment;
        owner_lock(&tx, e.owner_principal_id).await?;
        if let Some(row) = tx
            .query_opt(
                "SELECT row_json::text FROM lab_jobs WHERE owner_id=$1 AND submission_id=$2::text::uuid",
                &[&i(e.owner_principal_id), &e.submission_id],
            )
            .await
            .map_err(db_err)?
        {
            let old = lab_of(&row)?;
            if old.experiment.spec_digest != e.spec_digest {
                return Err(crate::lab_jobs::conflict("提交键已绑定不同规格"));
            }
            return Ok(old.experiment);
        }
        let n = tx
            .query_one(
                "SELECT count(*),count(*) FILTER(WHERE status IN ('queued','running')) FROM lab_jobs WHERE owner_id=$1",
                &[&i(e.owner_principal_id)],
            )
            .await
            .map_err(db_err)?;
        if n.get::<_, i64>(0) >= MAX_JOBS as i64 || n.get::<_, i64>(1) >= MAX_ACTIVE as i64 {
            return Err(crate::lab_jobs::conflict("Lab持久作业配额已满"));
        }
        tx.execute("INSERT INTO lab_jobs(exp_id,owner_id,submission_id,status,row_json) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5::text::jsonb)",&[&e.exp_id,&i(e.owner_principal_id),&e.submission_id,&e.status,&serde_json::to_string(&job).expect("作业可序列化")]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(job.experiment)
    }
    pub async fn lab_get(&self, owner: u64, id: String) -> StoreResult<Option<LabJob>> {
        self.client
            .lock()
            .await
            .query_opt(
                "SELECT row_json::text FROM lab_jobs WHERE owner_id=$1 AND exp_id=$2::text::uuid",
                &[&i(owner), &id],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(lab_of)
            .transpose()
    }
    pub async fn lab_list(&self, owner: u64, limit: u32, offset: u32) -> StoreResult<Vec<Experiment>> {
        self.client
            .lock()
            .await
            .query(
                "SELECT row_json::text FROM lab_jobs WHERE owner_id=$1 ORDER BY exp_id DESC LIMIT $2 OFFSET $3",
                &[&i(owner), &(limit as i64), &(offset as i64)],
            )
            .await
            .map_err(db_err)?
            .iter()
            .map(|r| lab_of(r).map(|j| j.experiment))
            .collect()
    }
    pub async fn lab_claim(&self, node: String) -> StoreResult<Option<LabJob>> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let time = now(&tx).await?;
        // 只锁单个候选作业，SKIP LOCKED允许其它节点认领不同作业；最终写入另按principal→job固定顺序重验。
        let row=tx.query_opt("SELECT j.row_json::text FROM lab_jobs j JOIN principals p ON p.principal_id=j.owner_id WHERE p.status=0 AND (j.status='queued' OR (j.status='running' AND j.lease_until_s<=$1)) AND j.row_json->'experiment'->>'kernel_profile'=$2 ORDER BY j.exp_id FOR UPDATE OF j SKIP LOCKED LIMIT 1",&[&i(time),&crate::lab_jobs::KERNEL_PROFILE]).await.map_err(db_err)?;
        let Some(row) = row else { return Ok(None) };
        let mut job = lab_of(&row)?;
        let claimed = job.claim(node, time)?;
        save(&tx, &job).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(claimed.then_some(job))
    }
    pub async fn lab_control(&self, owner: u64, id: String, op: OwnerAction) -> StoreResult<Experiment> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        owner_lock(&tx, owner).await?;
        let mut job = tx
            .query_opt(
                "SELECT row_json::text FROM lab_jobs WHERE owner_id=$1 AND exp_id=$2::text::uuid FOR UPDATE",
                &[&i(owner), &id],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(lab_of)
            .transpose()?
            .ok_or_else(|| ApiError::not_found("实验不存在"))?;
        if op.action == "retry" && !job.operations.contains_key(&op.operation_id) {
            let n: i64 = tx
                .query_one(
                    "SELECT count(*) FROM lab_jobs WHERE owner_id=$1 AND status IN ('queued','running')",
                    &[&i(owner)],
                )
                .await
                .map_err(db_err)?
                .get(0);
            if n >= MAX_ACTIVE as i64 {
                return Err(crate::lab_jobs::conflict("Lab活动作业配额已满"));
            }
        }
        let time = now(&tx).await?;
        job.owner_action(op, time)?;
        save(&tx, &job).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(job.experiment)
    }
    pub async fn lab_worker(&self, claim: Claim, action: WorkerAction) -> StoreResult<LabJob> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        owner_lock(&tx, claim.owner).await?;
        let mut job = tx
            .query_opt(
                "SELECT row_json::text FROM lab_jobs WHERE exp_id=$1::text::uuid AND owner_id=$2 FOR UPDATE",
                &[&claim.id, &i(claim.owner)],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(lab_of)
            .transpose()?
            .ok_or_else(|| crate::lab_jobs::conflict("Lab租约不存在"))?;
        let time = now(&tx).await?;
        job.worker_action(&claim, action, time)?;
        save(&tx, &job).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(job)
    }
}
