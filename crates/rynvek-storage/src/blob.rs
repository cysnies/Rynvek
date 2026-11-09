// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 内容寻址 Blob 存储（CAS，BLAKE3-256）。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

/// Blob 内容标识：BLAKE3-256 摘要
pub type BlobId = Blake3Hash;

/// CAS Blob 存储接口
pub trait BlobStore: Send + Sync {
    /// 写入不可变字节，返回内容哈希；重复写入同一内容是幂等的
    fn put(&self, bytes: &[u8]) -> Result<BlobId, RynvekErrorCode>;

    /// 按内容哈希读取；不存在时返回 `Blake3ChecksumMismatch` 的语义近似——此处用
    /// `EntityNotFoundOrTombstoned` 不合适，改用 `Blake3ChecksumMismatch` 表示「未知 CID」
    fn get(&self, id: &BlobId) -> Result<Vec<u8>, RynvekErrorCode>;

    /// 是否已存在
    fn contains(&self, id: &BlobId) -> bool;

    /// 已存 Blob 数量
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 进程内内存 CAS（测试与单机开发默认实现）
#[derive(Clone, Default, Debug)]
pub struct MemoryBlobStore {
    inner: Arc<Mutex<BTreeMap<[u8; 32], Vec<u8>>>>,
}

impl MemoryBlobStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlobStore for MemoryBlobStore {
    fn put(&self, bytes: &[u8]) -> Result<BlobId, RynvekErrorCode> {
        let id = Blake3Hash::of(bytes);
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        guard.entry(id.0).or_insert_with(|| bytes.to_vec());
        Ok(id)
    }

    fn get(&self, id: &BlobId) -> Result<Vec<u8>, RynvekErrorCode> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| RynvekErrorCode::StructuralMutationConflict)?;
        guard.get(&id.0).cloned().ok_or(RynvekErrorCode::Blake3ChecksumMismatch)
    }

    fn contains(&self, id: &BlobId) -> bool {
        self.inner.lock().map(|g| g.contains_key(&id.0)).unwrap_or(false)
    }

    fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cas_is_content_addressed_and_idempotent() {
        let store = MemoryBlobStore::new();
        let a = store.put(b"rynvek").unwrap();
        let b = store.put(b"rynvek").unwrap();
        assert_eq!(a, b);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(&a).unwrap(), b"rynvek");
        assert!(store.get(&Blake3Hash::of(b"missing")).is_err());
    }
}
