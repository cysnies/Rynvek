// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
fn audio_of(r: &Row) -> StoreResult<super::super::PrivateArtifactRow> {
    let mut row: super::super::PrivateArtifactRow =
        serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| corrupt("私有Agent版本"))?;
    row.revoked = r.get(1);
    Ok(row)
}
impl PostgresStore {
    pub async fn put_private_audio(&self, row: super::super::PrivateArtifactRow) -> StoreResult<()> {
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
                "SELECT count(*) FROM private_audio_versions WHERE owner_id=$1",
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
            let dep=tx.query_opt("SELECT row_json::text,revoked FROM private_audio_versions WHERE owner_id=$1 AND package_id=$2 AND semver=$3 FOR SHARE",&[&i(row.owner_id),&d.id,&d.version]).await.map_err(db_err)?.map(|r|audio_of(&r)).transpose()?.ok_or_else(||crate::transactions::conflict("依赖不存在"))?;
            if dep.revoked || crate::ids::hex(&dep.cid) != d.blake3_256 {
                return Err(crate::transactions::conflict("依赖已改变/撤销"));
            }
        }
        tx.execute(
            "INSERT INTO private_audio_versions(owner_id,package_id,semver,row_json) VALUES($1,$2,$3,$4::text::jsonb)",
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
    pub async fn private_audio(
        &self,
        owner: u64,
        id: String,
        version: String,
    ) -> StoreResult<Option<super::super::PrivateArtifactRow>> {
        self.client.lock().await.query_opt("SELECT row_json::text,revoked FROM private_audio_versions WHERE owner_id=$1 AND package_id=$2 AND semver=$3",&[&i(owner),&id,&version]).await.map_err(db_err)?.as_ref().map(audio_of).transpose()
    }
    pub async fn private_audios(&self, owner: u64) -> StoreResult<Vec<super::super::PrivateArtifactRow>> {
        self.client.lock().await.query("SELECT row_json::text,revoked FROM private_audio_versions WHERE owner_id=$1 ORDER BY package_id,semver LIMIT 256",&[&i(owner)]).await.map_err(db_err)?.iter().map(audio_of).collect()
    }
    pub async fn revoke_private_audio(&self, owner: u64, id: String, version: String) -> StoreResult<bool> {
        Ok(self
            .client
            .lock()
            .await
            .execute(
                "UPDATE private_audio_versions SET revoked=TRUE WHERE owner_id=$1 AND package_id=$2 AND semver=$3",
                &[&i(owner), &id, &version],
            )
            .await
            .map_err(db_err)?
            == 1)
    }
}
