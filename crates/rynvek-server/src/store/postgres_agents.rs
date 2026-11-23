// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
fn artifact_of(r: &Row) -> StoreResult<super::super::PrivateArtifactRow> {
    let mut row: super::super::PrivateArtifactRow =
        serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| corrupt("私有Agent版本"))?;
    row.revoked = r.get(1);
    Ok(row)
}
impl PostgresStore {
    pub async fn put_private_artifact(&self, kind:super::super::PrivateArtifactKind, row: super::super::PrivateArtifactRow) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        // 同主体行锁串行化配额与发布，固定顺序锁依赖防并发撤销覆盖已校验事实。
        let principal = tx
            .query_one(
                "SELECT status,ed25519_pubkey FROM principals WHERE principal_id=$1 FOR UPDATE",
                &[&i(row.owner_id)],
            )
            .await
            .map_err(db_err)?;
        if principal.get::<_, i16>(0) != 0 || principal.get::<_, Vec<u8>>(1) != row.publisher_key {
            return Err(crate::transactions::conflict("发布者已失效"));
        }
        let count: i64 = tx
            .query_one(
                &format!("SELECT count(*) FROM {} WHERE owner_id=$1",kind.table()),
                &[&i(row.owner_id)],
            )
            .await
            .map_err(db_err)?
            .get(0);
        if count >= 256 {
            return Err(crate::transactions::conflict("私有registry版本配额已满"));
        }
        let mut deps = row.dependencies.clone();
        deps.sort_by(|a, b| (&a.id, &a.version).cmp(&(&b.id, &b.version)));
        for d in deps {
            let dep=tx.query_opt(&format!("SELECT row_json::text,revoked FROM {} WHERE owner_id=$1 AND package_id=$2 AND semver=$3 FOR SHARE",kind.table()),&[&i(row.owner_id),&d.id,&d.version]).await.map_err(db_err)?.map(|r|artifact_of(&r)).transpose()?.ok_or_else(||crate::transactions::conflict("依赖不存在"))?;
            if dep.revoked || crate::ids::hex(&dep.cid) != d.blake3_256 {
                return Err(crate::transactions::conflict("依赖已改变/撤销"));
            }
        }
        tx.execute(
            &format!("INSERT INTO {}(owner_id,package_id,semver,row_json) VALUES($1,$2,$3,$4::text::jsonb)",kind.table()),
            &[
                &i(row.owner_id),
                &row.package_id,
                &row.version,
                &serde_json::to_string(&row).expect("版本可序列化"),
            ],
        )
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }
    pub async fn private_artifact(
        &self,
        kind:super::super::PrivateArtifactKind,
        owner: u64,
        id: String,
        version: String,
    ) -> StoreResult<Option<super::super::PrivateArtifactRow>> {
        self.client.lock().await.query_opt(&format!("SELECT row_json::text,revoked FROM {} WHERE owner_id=$1 AND package_id=$2 AND semver=$3",kind.table()),&[&i(owner),&id,&version]).await.map_err(db_err)?.as_ref().map(artifact_of).transpose()
    }
    pub async fn private_artifacts(&self, kind:super::super::PrivateArtifactKind, owner: u64) -> StoreResult<Vec<super::super::PrivateArtifactRow>> {
        self.client.lock().await.query(&format!("SELECT row_json::text,revoked FROM {} WHERE owner_id=$1 ORDER BY package_id,semver LIMIT 256",kind.table()),&[&i(owner)]).await.map_err(db_err)?.iter().map(artifact_of).collect()
    }
    pub async fn revoke_private_artifact(&self, kind:super::super::PrivateArtifactKind, owner: u64, id: String, version: String) -> StoreResult<bool> {
        Ok(self
            .client
            .lock()
            .await
            .execute(
                &format!("UPDATE {} SET revoked=TRUE WHERE owner_id=$1 AND package_id=$2 AND semver=$3",kind.table()),
                &[&i(owner), &id, &version],
            )
            .await
            .map_err(db_err)?
            == 1)
    }
    pub async fn put_private_agent(&self,row:super::super::PrivateArtifactRow)->StoreResult<()> {self.put_private_artifact(super::super::PrivateArtifactKind::Agent,row).await}
    pub async fn private_agent(&self,owner:u64,id:String,version:String)->StoreResult<Option<super::super::PrivateArtifactRow>> {self.private_artifact(super::super::PrivateArtifactKind::Agent,owner,id,version).await}
    pub async fn private_agents(&self,owner:u64)->StoreResult<Vec<super::super::PrivateArtifactRow>> {self.private_artifacts(super::super::PrivateArtifactKind::Agent,owner).await}
    pub async fn revoke_private_agent(&self,owner:u64,id:String,version:String)->StoreResult<bool> {self.revoke_private_artifact(super::super::PrivateArtifactKind::Agent,owner,id,version).await}

}
