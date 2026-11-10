// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 追加写入器（带 `sync_data()` / `sync_all()` 持久化屏障，满足崩溃恢复契约）。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use rynvek_core::error::RynvekErrorCode;

use crate::frame::{JournalFrame, JournalRecord};
use crate::header::JournalFileHeader;
use crate::manifest::CommitManifest;

/// 追加式 `.rjournal` 写入器
#[derive(Debug)]
pub struct JournalWriter {
    path: PathBuf,
    file: File,
    header: JournalFileHeader,
    frames_written: u64,
}

impl JournalWriter {
    pub fn create(
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        base_tick: u64,
        base_world_revision: u64,
        last_snapshot_tick: u64,
    ) -> Result<Self, RynvekErrorCode> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        let header = JournalFileHeader::new(world_id, base_tick, base_world_revision, last_snapshot_tick);
        file.write_all(&header.encode())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.sync_all()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Ok(Self {
            path,
            file,
            header,
            frames_written: 0,
        })
    }

    /// 追加一条 Tick 提交清单帧并执行 `sync_data` 持久化屏障
    pub fn append_commit(&mut self, commit: &CommitManifest) -> Result<JournalFrame, RynvekErrorCode> {
        let frame = JournalFrame::from_commit(commit);
        self.write_raw_frame(&frame)?;
        Ok(frame)
    }

    /// 追加一条通用类型化 WAL 记录帧并执行 `sync_data` 持久化屏障
    pub fn append_record(&mut self, record: &JournalRecord) -> Result<JournalFrame, RynvekErrorCode> {
        let frame = JournalFrame::from_record(record);
        self.write_raw_frame(&frame)?;
        Ok(frame)
    }

    fn write_raw_frame(&mut self, frame: &JournalFrame) -> Result<(), RynvekErrorCode> {
        self.file
            .write_all(&frame.to_bytes())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .sync_data()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.frames_written += 1;
        Ok(())
    }

    /// 显式执行完整元数据与数据落盘屏障
    pub fn sync_all(&mut self) -> Result<(), RynvekErrorCode> {
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .sync_all()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)
    }

    /// 仅供断电模拟测试使用：向尾部直接写入不完整的残缺字节以模拟撕裂写
    pub fn append_torn_bytes(&mut self, partial: &[u8]) -> Result<(), RynvekErrorCode> {
        self.file
            .write_all(partial)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Ok(())
    }

    pub const fn header(&self) -> &JournalFileHeader {
        &self.header
    }

    pub const fn frames_written(&self) -> u64 {
        self.frames_written
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
