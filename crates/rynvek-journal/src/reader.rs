// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Journal 顺序读取器：遇到撕裂写即停止，同时解析全部类型化 WAL 记录与提交清单。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use rynvek_core::error::RynvekErrorCode;

use crate::frame::{
    FRAME_CHECKSUM_LEN, FRAME_HEADER_LEN, JournalFrame, JournalFrameHeader, JournalRecord, checksum_of,
};
use crate::header::{JOURNAL_HEADER_LEN, JournalFileHeader};
use crate::manifest::CommitManifest;

/// 读取结果：完整提交帧列表 + 全部类型化 WAL 记录 + 是否因撕裂写提前结束
#[derive(Clone, Debug)]
pub struct JournalReadResult {
    pub header: JournalFileHeader,
    pub commits: Vec<CommitManifest>,
    pub records: Vec<JournalRecord>,
    pub torn_write: bool,
}

/// 打开并重放 journal
#[derive(Debug, Default)]
pub struct JournalReader;

impl JournalReader {
    pub fn read_path(path: impl AsRef<Path>) -> Result<JournalReadResult, RynvekErrorCode> {
        let mut file = File::open(path.as_ref()).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        Self::read_bytes(&bytes)
    }

    /// 显式调用者上限；读满上限加一即拒绝，不把整个超大 WAL 装入内存。
    pub fn read_path_bounded(path: impl AsRef<Path>, max_bytes: usize) -> Result<JournalReadResult, RynvekErrorCode> {
        let file = File::open(path.as_ref()).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| RynvekErrorCode::JournalTornWriteTruncated)?;
        if bytes.len() > max_bytes {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        Self::read_bytes(&bytes)
    }

    pub fn read_bytes(bytes: &[u8]) -> Result<JournalReadResult, RynvekErrorCode> {
        if bytes.len() < JOURNAL_HEADER_LEN {
            return Err(RynvekErrorCode::JournalTornWriteTruncated);
        }
        let header = JournalFileHeader::decode(&bytes[..JOURNAL_HEADER_LEN])?;
        let mut offset = JOURNAL_HEADER_LEN;
        let mut commits = Vec::new();
        let mut records = Vec::new();
        let mut torn_write = false;
        while offset < bytes.len() {
            match decode_frame_at(bytes, offset) {
                Ok((frame, next)) => {
                    frame.verify_checksum()?;
                    let record = frame.to_record()?;
                    if let JournalRecord::TickCommit(m) | JournalRecord::Checkpoint(m) = &record {
                        commits.push(*m);
                    }
                    records.push(record);
                    offset = next;
                }
                Err(RynvekErrorCode::JournalTornWriteTruncated) => {
                    torn_write = true;
                    break;
                }
                Err(other) => return Err(other),
            }
        }
        Ok(JournalReadResult {
            header,
            commits,
            records,
            torn_write,
        })
    }
}

fn decode_frame_at(bytes: &[u8], offset: usize) -> Result<(JournalFrame, usize), RynvekErrorCode> {
    if bytes.len() < offset + FRAME_HEADER_LEN {
        return Err(RynvekErrorCode::JournalTornWriteTruncated);
    }
    let header = JournalFrameHeader::decode(&bytes[offset..offset + FRAME_HEADER_LEN])?;
    let payload_start = offset + FRAME_HEADER_LEN;
    let payload_end = payload_start
        .checked_add(header.payload_len as usize)
        .ok_or(RynvekErrorCode::JournalTornWriteTruncated)?;
    let checksum_end = payload_end
        .checked_add(FRAME_CHECKSUM_LEN)
        .ok_or(RynvekErrorCode::JournalTornWriteTruncated)?;
    if bytes.len() < checksum_end {
        return Err(RynvekErrorCode::JournalTornWriteTruncated);
    }
    let payload = bytes[payload_start..payload_end].to_vec();
    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&bytes[payload_end..checksum_end]);
    // 快速拒绝损坏帧
    if checksum_of(&header, &payload) != checksum {
        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
    }
    Ok((
        JournalFrame {
            header,
            payload,
            checksum,
        },
        checksum_end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::JournalCommandRecord;
    use crate::manifest::CommitManifest;
    use crate::writer::JournalWriter;
    use tempfile::TempDir;

    #[test]
    fn reads_commits_and_typed_records_until_torn_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("w.rjournal");
        let mut writer = JournalWriter::create(&path, [1; 16], 0, 0, 0).unwrap();
        let cmd = JournalRecord::CommandAccepted(JournalCommandRecord {
            tick: 1,
            world_revision: 1,
            faction_id: 0,
            command_bytes: b"surrender".to_vec(),
        });
        writer.append_record(&cmd).unwrap();
        let m1 = CommitManifest::new(
            [1; 16],
            1,
            1,
            [0; 32],
            crate::manifest::CommitSubRoots {
                entity_root_hash: [1; 32],
                ..Default::default()
            },
        );
        writer.append_commit(&m1).unwrap();
        writer.append_torn_bytes(&[0xAA, 0xBB, 0xCC]).unwrap();
        let result = JournalReader::read_path(&path).unwrap();
        assert_eq!(result.commits.len(), 1);
        assert_eq!(result.records.len(), 2);
        assert!(result.torn_write);
        assert_eq!(result.commits[0], m1);
        assert_eq!(result.records[0], cmd);
    }
}
