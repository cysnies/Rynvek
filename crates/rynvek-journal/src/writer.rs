// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 追加写入器。

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rynvek_core::error::RynvekErrorCode;

use crate::frame::JournalFrame;
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
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(&path)
            .map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let header = JournalFileHeader::new(world_id, base_tick, base_revision, snapshot_tick);
        file.write_all(&header.encode())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        file.flush().map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
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

    /// 追加一条提交帧并尽量刷盘
    pub fn append_commit(&mut self, manifest: &CommitManifest) -> Result<(), RynvekErrorCode> {
        if manifest.world_id != self.header.world_id {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let frame = JournalFrame::from_commit(manifest);
        self.file
            .write_all(&frame.encode())
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.file
            .flush()
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        self.frames_written += 1;
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
        Ok(())
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
