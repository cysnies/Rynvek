// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
impl PostgresStore {
    pub async fn put_private_audio(&self,row:super::super::PrivateArtifactRow)->StoreResult<()> {self.put_private_artifact(super::super::PrivateArtifactKind::Audio,row).await}
    pub async fn private_audio(&self,owner:u64,id:String,version:String)->StoreResult<Option<super::super::PrivateArtifactRow>> {self.private_artifact(super::super::PrivateArtifactKind::Audio,owner,id,version).await}
    pub async fn private_audios(&self,owner:u64)->StoreResult<Vec<super::super::PrivateArtifactRow>> {self.private_artifacts(super::super::PrivateArtifactKind::Audio,owner).await}
    pub async fn revoke_private_audio(&self,owner:u64,id:String,version:String)->StoreResult<bool> {self.revoke_private_artifact(super::super::PrivateArtifactKind::Audio,owner,id,version).await}
}
