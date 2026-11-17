// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::agent_runs::{AgentRun, Change};
fn run(r: &Row) -> StoreResult<AgentRun> {
    serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| corrupt("Agent运行"))
}
async fn owner(tx: &tokio_postgres::Transaction<'_>, pid: u64) -> StoreResult<()> {
    let r = tx
        .query_opt(
            "SELECT status FROM principals WHERE principal_id=$1 FOR UPDATE",
            &[&i(pid)],
        )
        .await
        .map_err(db_err)?;
    if !r.is_some_and(|r| r.get::<_, i16>(0) == 0) {
        return Err(crate::lab_jobs::conflict("owner失效"));
    }
    Ok(())
}
impl PostgresStore {
    pub async fn agent_run_create(&self, mut row: AgentRun) -> StoreResult<AgentRun> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        let pid = row.owner_id.parse::<u64>().map_err(|_| corrupt("owner"))?;
        owner(&tx, pid).await?;
        if let Some(old) = tx
            .query_opt(
                "SELECT row_json::text FROM agent_runs WHERE owner_id=$1 AND submission_id=$2::text::uuid",
                &[&i(pid), &row.submission_id],
            )
            .await
            .map_err(db_err)?
        {
            let old = run(&old)?;
            return if old.spec_digest == row.spec_digest {
                Ok(old)
            } else {
                Err(crate::lab_jobs::conflict("提交键绑定其它运行规格"))
            };
        }
        let n: i64 = tx
            .query_one("SELECT count(*) FROM agent_runs WHERE owner_id=$1", &[&i(pid)])
            .await
            .map_err(db_err)?
            .get(0);
        if n >= 64 {
            return Err(crate::lab_jobs::conflict("运行总量上限64"));
        }
        let now: i64 = tx
            .query_one("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint", &[])
            .await
            .map_err(db_err)?
            .get(0);
        row.created_at_unix_s = now as u64;
        row.deadline_unix_s = row.created_at_unix_s + u64::from(row.budget.wall_seconds);
        tx.execute("INSERT INTO agent_runs(run_id,owner_id,submission_id,world_id,active,row_json) VALUES($1::text::uuid,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::jsonb)",&[&row.run_id,&i(pid),&row.submission_id,&row.world_id,&row.active(),&serde_json::to_string(&row).unwrap()]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(row)
    }
    pub async fn agent_run_get(&self, pid: u64, id: String) -> StoreResult<Option<AgentRun>> {
        self.client
            .lock()
            .await
            .query_opt(
                "SELECT row_json::text FROM agent_runs WHERE owner_id=$1 AND run_id=$2::text::uuid",
                &[&i(pid), &id],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(run)
            .transpose()
    }
    pub async fn agent_run_list(&self, pid: u64) -> StoreResult<Vec<AgentRun>> {
        self.client
            .lock()
            .await
            .query(
                "SELECT row_json::text FROM agent_runs WHERE owner_id=$1 ORDER BY run_id DESC LIMIT 64",
                &[&i(pid)],
            )
            .await
            .map_err(db_err)?
            .iter()
            .map(run)
            .collect()
    }
    pub async fn agent_run_change(&self, pid: u64, id: String, change: Change) -> StoreResult<(AgentRun, bool)> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        owner(&tx, pid).await?;
        let mut row = tx
            .query_opt(
                "SELECT row_json::text FROM agent_runs WHERE owner_id=$1 AND run_id=$2::text::uuid FOR UPDATE",
                &[&i(pid), &id],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(run)
            .transpose()?
            .ok_or_else(|| ApiError::not_found("运行不存在"))?;
        let now: i64 = tx
            .query_one("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint", &[])
            .await
            .map_err(db_err)?
            .get(0);
        let revision = row.revision;
        row.apply(change, now as u64)?;
        let changed = revision != row.revision;
        tx.execute(
            "UPDATE agent_runs SET active=$2,row_json=$3::text::jsonb WHERE run_id=$1::text::uuid",
            &[&id, &row.active(), &serde_json::to_string(&row).unwrap()],
        )
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok((row, changed))
    }
}
