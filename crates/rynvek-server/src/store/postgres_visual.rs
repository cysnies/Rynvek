// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
impl PostgresStore {
    pub async fn put_private_visual(&self,row:super::super::PrivateArtifactRow)->StoreResult<()> {self.put_private_artifact(super::super::PrivateArtifactKind::Visual,row).await}
    pub async fn private_visual(&self,owner:u64,id:String,version:String)->StoreResult<Option<super::super::PrivateArtifactRow>> {self.private_artifact(super::super::PrivateArtifactKind::Visual,owner,id,version).await}
    pub async fn private_visuals(&self,owner:u64)->StoreResult<Vec<super::super::PrivateArtifactRow>> {self.private_artifacts(super::super::PrivateArtifactKind::Visual,owner).await}
    pub async fn revoke_private_visual(&self,owner:u64,id:String,version:String)->StoreResult<bool> {self.revoke_private_artifact(super::super::PrivateArtifactKind::Visual,owner,id,version).await}
}
