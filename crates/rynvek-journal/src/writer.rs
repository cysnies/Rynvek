// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 追加写入器（保持文件头 `frame_count` 回填并补齐 `sync_data()` / `sync_all()` 持久化屏障）。

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rynvek_core::error::RynvekErrorCode;

use crate::frame::{JournalFrame, JournalRecord};
use crate::header::JournalFileHeader;
use crate::manifest::CommitManifest;

/// `.rjournal` 追加写
pub struct JournalWriter {
    path: PathBuf,
    file: File,
    header: JournalFileHeader,
    frames_written: u32,
}

impl std::fmt::Debug for JournalWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalWriter")
            .field("path", &self.path)
            .field("header", &self.header)
            .field("frames_written", &self.frames_written)
            .finish_non_exhaustive()
    }
}

impl JournalWriter {
    /// 创建新 journal 文件并写入文件头
    pub fn create(
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        base_tick: u64,
        base_revision: u64,
        snapshot_tick: u64,
    ) -> Result<Self, RynvekErrorCode> {
        Self::create_impl(path.as_ref(), world_id, base_tick, base_revision, snapshot_tick, false)
    }

    /// 独占创建，拒绝覆盖既有日志（本地工具默认使用此入口）。
    pub fn create_new(
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        base_tick: u64,
        base_revision: u64,
        snapshot_tick: u64,
    ) -> Result<Self, RynvekErrorCode> {
        Self::create_impl(path.as_ref(), world_id, base_tick, base_revision, snapshot_tick, true)
    }

    fn create_impl(
        path: &Path,
        world_id: [u8; 16],
        base_tick: u64,
        base_revision: u64,
        snapshot_tick: u64,
        exclusive: bool,
    ) -> Result<Self, RynvekErrorCode> {
        let path = path.to_path_buf();
        let mut file = OpenOptions::new()
            .create(!exclusive)
            .truncate(!exclusive)
            .create_new(exclusive)
            .write(true)
            .read(true)
            .open(&path)
            .map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let header = JournalFileHeader::new(world_id, base_tick, base_revision, snapshot_tick);
        file.write_all(&header.encode())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.flush().map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.sync_all()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Ok(Self {
            path,
            file,
            header,
            frames_written: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &JournalFileHeader {
        &self.header
    }

    pub const fn frames_written(&self) -> u32 {
        self.frames_written
    }

    /// 追加一条提交帧并执行 `sync_data` 持久化屏障
    pub fn append_commit(&mut self, manifest: &CommitManifest) -> Result<(), RynvekErrorCode> {
        if manifest.world_id != self.header.world_id {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let frame = JournalFrame::from_commit(manifest);
        self.write_raw_frame(&frame)
    }

    /// 追加一条类型化 WAL 记录帧并执行 `sync_data` 持久化屏障
    pub fn append_record(&mut self, record: &JournalRecord) -> Result<(), RynvekErrorCode> {
        if let JournalRecord::TickCommit(m) | JournalRecord::Checkpoint(m) = record {
            if m.world_id != self.header.world_id {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
        }
        let frame = JournalFrame::from_record(record);
        self.write_raw_frame(&frame)
    }

    fn write_raw_frame(&mut self, frame: &JournalFrame) -> Result<(), RynvekErrorCode> {
        self.file
            .write_all(&frame.encode())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.frames_written = self.frames_written.saturating_add(1);
        self.header.frame_count = self.frames_written;
        // 回填头部 frame_count（崩溃时可能未回填；读取器以校验为准）
        let encoded = self.header.encode();
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .write_all(&encoded)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .seek(SeekFrom::End(0))
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .sync_data()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Ok(())
    }

    /// 显式执行完整元数据与数据刷盘屏障
    pub fn sync_all(&mut self) -> Result<(), RynvekErrorCode> {
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .sync_all()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)
    }

    /// 故意写入截断的半帧（用于测试断电场景；生产路径不得调用）
    pub fn append_torn_bytes(&mut self, bytes: &[u8]) -> Result<(), RynvekErrorCode> {
        self.file
            .write_all(bytes)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Ok(())
    }
}
