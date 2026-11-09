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

/// 文件系统 CAS（服务端持久化后端，ERRATA E-050）。
///
/// - 布局：`<root>/<hex[0..2]>/<hex>`，文件名即 BLAKE3-256 十六进制摘要；
/// - 写入：先写 `<root>/tmp/<hex>.<pid>.<seq>` 并 `fsync`，再原子 `rename` 到最终路径，
///   崩溃后不会留下半个 Blob（残留临时文件在下次打开时清理）；
/// - 读取：重新计算 BLAKE3，磁盘位翻转或被篡改时返回 `0x7002 Blake3ChecksumMismatch`，
///   与未知 CID 同码；写入失败返回 `0x7003 JournalTornWriteTruncated`（恢复动作 TriggerStorageRecovery）。
///
/// 对象存储（S3 兼容）后端可按同一布局映射为对象键，M8 的分发闭环只需要本地文件后端。
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct FsBlobStore {
    root: std::path::PathBuf,
    seq: std::sync::atomic::AtomicU64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FsBlobStore {
    /// 打开（必要时创建）目录
    pub fn open(root: impl Into<std::path::PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        let tmp = root.join("tmp");
        std::fs::create_dir_all(&tmp)?;
        for e in std::fs::read_dir(&tmp)? {
            let _ = std::fs::remove_file(e?.path());
        }
        Ok(Self {
            root,
            seq: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    fn path_of(&self, id: &BlobId) -> std::path::PathBuf {
        let hex = id.to_hex();
        self.root.join(&hex[..2]).join(hex)
    }

    fn write_atomic(&self, id: &BlobId, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let fin = self.path_of(id);
        if let Some(dir) = fin.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let n = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = self
            .root
            .join("tmp")
            .join(format!("{}.{}.{n}", id.to_hex(), std::process::id()));
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &fin)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BlobStore for FsBlobStore {
    fn put(&self, bytes: &[u8]) -> Result<BlobId, RynvekErrorCode> {
        let id = Blake3Hash::of(bytes);
        if self.contains(&id) {
            return Ok(id);
        }
        self.write_atomic(&id, bytes)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Ok(id)
    }

    fn get(&self, id: &BlobId) -> Result<Vec<u8>, RynvekErrorCode> {
        let bytes = std::fs::read(self.path_of(id)).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        if Blake3Hash::of(&bytes) != *id {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok(bytes)
    }

    fn contains(&self, id: &BlobId) -> bool {
        self.path_of(id).is_file()
    }

    fn len(&self) -> usize {
        let Ok(dirs) = std::fs::read_dir(&self.root) else {
            return 0;
        };
        dirs.filter_map(Result::ok)
            .filter(|d| d.file_name().len() == 2 && d.path().is_dir())
            .map(|d| std::fs::read_dir(d.path()).map_or(0, |it| it.count()))
            .sum()
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

    #[test]
    fn fs_store_persists_and_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let s = FsBlobStore::open(dir.path()).unwrap();
            let id = s.put(b"rynvek-hub").unwrap();
            assert_eq!(s.put(b"rynvek-hub").unwrap(), id);
            s.put(b"other").unwrap();
            assert_eq!(s.len(), 2);
            id
        };
        // 重新打开后仍可读
        let s = FsBlobStore::open(dir.path()).unwrap();
        assert_eq!(s.get(&id).unwrap(), b"rynvek-hub");
        // 篡改磁盘内容 → 0x7002
        std::fs::write(s.path_of(&id), b"tampered").unwrap();
        assert_eq!(s.get(&id).unwrap_err(), RynvekErrorCode::Blake3ChecksumMismatch);
        assert_eq!(
            s.get(&Blake3Hash::of(b"missing")).unwrap_err(),
            RynvekErrorCode::Blake3ChecksumMismatch
        );
    }
}
