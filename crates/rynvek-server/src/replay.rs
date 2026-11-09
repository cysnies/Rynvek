// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rreplay` 确定性回放归档（规范 10.1 魔数 `RYRP`、D073–D075、D197 Bit-Exact 等级、D208 裁判回放）。
//!
//! ## 布局（Little-Endian）
//!
//! | 偏移 | 长度 | 字段 |
//! | :--- | :--- | :--- |
//! | 0 | 64 | 文件头（与 `.rsnap` 的 `SnapshotFileHeader` 同构，魔数 `RYRP`） |
//! | 64 | `payload_bytes` | Zstd 压缩的 JSON [`ReplayBody`] |
//! | … | 32 | `checksum` = BLAKE3(文件头 ‖ 载荷) |
//! | … | 2 | `signer_key_id` |
//! | … | 14 | 保留（0） |
//! | … | 64 | Ed25519 签名，覆盖 `文件头 ‖ 载荷 ‖ checksum ‖ signer_key_id ‖ 保留` |
//!
//! 文件头字段：`format_version` = 1；`flags` bit0 = Zstd；`header_bytes` = 64；`chunk_count` = 1；
//! `world_id`；`tick` = 终局 Tick；`world_revision` = 回放等级（0 = Bit-Exact）；`payload_bytes`。
//!
//! ## 校验顺序与错误码
//!
//! 1. 魔数 / 版本 / 长度 → `0x7001 InvalidFileMagicOrVersion`
//! 2. BLAKE3 校验和 → `0x7002 Blake3ChecksumMismatch`
//! 3. 服务端 Ed25519 签名 → `0x9001 InvalidEd25519Signature`
//! 4. 载荷解压与解析 → `0x7001`
//! 5. 重新仿真：逐检查点比对状态根、终局状态根与胜负 → `0x3003 SimulationInvariantViolated`

use rynvek_core::error::RynvekErrorCode;
use rynvek_rts::{ArenaWorld, MatchConfig, MatchOutcome, RecordedCommand};
use rynvek_security::{Keyring, League, ServerSigningKey};
use serde::{Deserialize, Serialize};

/// 魔数 `"RYRP"`
pub const REPLAY_MAGIC: [u8; 4] = *b"RYRP";
pub const REPLAY_VERSION: u16 = 1;
pub const REPLAY_HEADER_LEN: usize = 64;
pub const REPLAY_FLAG_ZSTD: u16 = 1;
/// 尾部：checksum 32 + key_id 2 + 保留 14 + 签名 64
pub const REPLAY_TRAILER_LEN: usize = 112;
/// 载荷格式标识
pub const REPLAY_FORMAT: &str = "rynvek-replay/1";
/// 回放等级（D197）
pub const REPLAY_LEVEL_BIT_EXACT: u64 = 0;
/// 状态根检查点间隔（Tick）
pub const CHECKPOINT_INTERVAL_TICKS: u64 = 300;
/// 解压后载荷上限（64 MiB），防压缩炸弹
pub const MAX_REPLAY_BODY_BYTES: usize = 64 << 20;

/// 参赛方
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayParticipant {
    pub faction: u8,
    pub principal_id: u64,
    pub league: League,
}

/// 状态根检查点
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub tick: u64,
    /// BLAKE3 状态根（十六进制）
    pub state_root: String,
}

/// 回放载荷
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayBody {
    pub format: String,
    pub match_id: String,
    pub world_id: String,
    /// 规则集哈希（十六进制）
    pub ruleset_hash: String,
    pub config: MatchConfig,
    pub participants: Vec<ReplayParticipant>,
    /// 权威命令日志（只含被接受的命令）
    pub commands: Vec<RecordedCommand>,
    pub checkpoints: Vec<Checkpoint>,
    pub final_tick: u64,
    pub final_state_root: String,
    pub outcome: Option<MatchOutcome>,
}

/// 文件头
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayHeader {
    pub format_version: u16,
    pub flags: u16,
    pub chunk_count: u16,
    pub world_id: [u8; 16],
    pub tick: u64,
    pub level: u64,
    pub payload_bytes: u64,
}

impl ReplayHeader {
    pub fn encode(&self) -> [u8; REPLAY_HEADER_LEN] {
        let mut out = [0u8; REPLAY_HEADER_LEN];
        out[0..4].copy_from_slice(&REPLAY_MAGIC);
        out[4..6].copy_from_slice(&self.format_version.to_le_bytes());
        out[6..8].copy_from_slice(&self.flags.to_le_bytes());
        out[8..10].copy_from_slice(&(REPLAY_HEADER_LEN as u16).to_le_bytes());
        out[10..12].copy_from_slice(&self.chunk_count.to_le_bytes());
        out[16..32].copy_from_slice(&self.world_id);
        out[32..40].copy_from_slice(&self.tick.to_le_bytes());
        out[40..48].copy_from_slice(&self.level.to_le_bytes());
        out[48..56].copy_from_slice(&self.payload_bytes.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ReplayError> {
        let bad = |m: &str| ReplayError::new(RynvekErrorCode::InvalidFileMagicOrVersion, m);
        if bytes.len() < REPLAY_HEADER_LEN {
            return Err(bad("文件短于 64 字节文件头"));
        }
        if bytes[0..4] != REPLAY_MAGIC {
            return Err(bad("魔数不是 RYRP"));
        }
        let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().expect("8 字节"));
        let format_version = u16_at(4);
        if format_version != REPLAY_VERSION {
            return Err(bad("不支持的回放格式版本"));
        }
        if u16_at(8) as usize != REPLAY_HEADER_LEN {
            return Err(bad("header_bytes 必须为 64"));
        }
        let flags = u16_at(6);
        if flags & !REPLAY_FLAG_ZSTD != 0 {
            return Err(bad("未知 flags 位"));
        }
        if bytes[12..16] != [0; 4] || bytes[56..64] != [0; 8] {
            return Err(bad("保留字段必须为 0"));
        }
        Ok(Self {
            format_version,
            flags,
            chunk_count: u16_at(10),
            world_id: bytes[16..32].try_into().expect("16 字节"),
            tick: u64_at(32),
            level: u64_at(40),
            payload_bytes: u64_at(48),
        })
    }
}

/// 回放错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayError {
    pub code: RynvekErrorCode,
    pub message: String,
}

impl ReplayError {
    pub fn new(code: RynvekErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn divergence(message: impl Into<String>) -> Self {
        Self::new(RynvekErrorCode::SimulationInvariantViolated, message)
    }
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({:#06x}): {}", self.code, self.code.code(), self.message)
    }
}

impl std::error::Error for ReplayError {}

/// 从权威世界录制回放载荷：重新仿真一遍以生成检查点（同时自检录制完整性）
pub fn record(
    world: &ArenaWorld,
    match_id: String,
    world_id: String,
    ruleset_hash: String,
    participants: Vec<ReplayParticipant>,
) -> Result<ReplayBody, ReplayError> {
    let mut checkpoints = Vec::new();
    let replayed = ArenaWorld::replay(world.config.clone(), world.command_log(), world.tick, |w| {
        if w.tick % CHECKPOINT_INTERVAL_TICKS == 0 {
            checkpoints.push(Checkpoint {
                tick: w.tick,
                state_root: w.state_hash().to_hex(),
            });
        }
    })
    .map_err(|(i, e)| ReplayError::divergence(format!("录制自检：第 {i} 条命令重放失败：{e}")))?;
    if replayed.state_hash() != world.state_hash() {
        return Err(ReplayError::divergence("录制自检：重放终局状态根与权威世界不一致"));
    }
    Ok(ReplayBody {
        format: REPLAY_FORMAT.to_owned(),
        match_id,
        world_id,
        ruleset_hash,
        config: world.config.clone(),
        participants,
        commands: world.command_log().to_vec(),
        checkpoints,
        final_tick: world.tick,
        final_state_root: world.state_hash().to_hex(),
        outcome: world.outcome.clone(),
    })
}

fn signed_region_len(payload_len: usize) -> usize {
    REPLAY_HEADER_LEN + payload_len + 32 + 16
}

/// 编码并签名
pub fn encode(body: &ReplayBody, world_id: [u8; 16], signer: &ServerSigningKey) -> Result<Vec<u8>, ReplayError> {
    let json = serde_json::to_vec(body)
        .map_err(|e| ReplayError::new(RynvekErrorCode::InvalidFileMagicOrVersion, e.to_string()))?;
    let payload = zstd::bulk::compress(&json, 9)
        .map_err(|e| ReplayError::new(RynvekErrorCode::InvalidFileMagicOrVersion, e.to_string()))?;
    let header = ReplayHeader {
        format_version: REPLAY_VERSION,
        flags: REPLAY_FLAG_ZSTD,
        chunk_count: 1,
        world_id,
        tick: body.final_tick,
        level: REPLAY_LEVEL_BIT_EXACT,
        payload_bytes: payload.len() as u64,
    };
    let mut out = Vec::with_capacity(signed_region_len(payload.len()) + 64);
    out.extend_from_slice(&header.encode());
    out.extend_from_slice(&payload);
    let checksum = blake3::hash(&out);
    out.extend_from_slice(checksum.as_bytes());
    out.extend_from_slice(&signer.key_id().to_le_bytes());
    out.extend_from_slice(&[0u8; 14]);
    let signature = signer.sign(&out);
    out.extend_from_slice(&signature);
    Ok(out)
}

/// 结构已校验（校验和 + 签名）的回放
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedReplay {
    pub header: ReplayHeader,
    pub body: ReplayBody,
    pub signer_key_id: u16,
}

/// 解析并校验结构、校验和；`keyring` 为 `None` 时跳过签名（仅供 `inspect`）
pub fn open(bytes: &[u8], keyring: Option<&Keyring>) -> Result<OpenedReplay, ReplayError> {
    let bad = |m: String| ReplayError::new(RynvekErrorCode::InvalidFileMagicOrVersion, m);
    let header = ReplayHeader::decode(bytes)?;
    let payload_len = usize::try_from(header.payload_bytes).map_err(|_| bad("payload_bytes 溢出".into()))?;
    let expected_len = signed_region_len(payload_len)
        .checked_add(64)
        .ok_or_else(|| bad("长度溢出".into()))?;
    if bytes.len() != expected_len {
        return Err(bad(format!(
            "文件长度 {} 与文件头声明的 {expected_len} 不符",
            bytes.len()
        )));
    }
    let body_end = REPLAY_HEADER_LEN + payload_len;
    let checksum = blake3::hash(&bytes[..body_end]);
    if checksum.as_bytes() != &bytes[body_end..body_end + 32] {
        return Err(ReplayError::new(
            RynvekErrorCode::Blake3ChecksumMismatch,
            "BLAKE3 校验和不匹配：文件已损坏或被篡改",
        ));
    }
    let key_id = u16::from_le_bytes([bytes[body_end + 32], bytes[body_end + 33]]);
    if bytes[body_end + 34..body_end + 48] != [0u8; 14] {
        return Err(bad("尾部保留字段必须为 0".into()));
    }
    if let Some(ring) = keyring {
        let signed = &bytes[..body_end + 48];
        let sig: [u8; 64] = bytes[body_end + 48..].try_into().expect("64 字节");
        ring.verify(key_id, signed, &sig)
            .map_err(|e| ReplayError::new(e.code, e.message))?;
    }
    let json = zstd::bulk::decompress(&bytes[REPLAY_HEADER_LEN..body_end], MAX_REPLAY_BODY_BYTES)
        .map_err(|e| bad(format!("Zstd 解压失败：{e}")))?;
    let body: ReplayBody = serde_json::from_slice(&json).map_err(|e| bad(format!("载荷 JSON 非法：{e}")))?;
    if body.format != REPLAY_FORMAT {
        return Err(bad(format!("未知载荷格式 `{}`", body.format)));
    }
    if body.final_tick != header.tick {
        return Err(bad("文件头 tick 与载荷 final_tick 不一致".into()));
    }
    Ok(OpenedReplay {
        header,
        body,
        signer_key_id: key_id,
    })
}

/// 验证报告
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    pub match_id: String,
    pub signer_key_id: u16,
    pub final_tick: u64,
    pub final_state_root: String,
    pub commands: usize,
    pub checkpoints_verified: usize,
    pub outcome: Option<MatchOutcome>,
}

/// 完整验证：结构 → 校验和 → 签名 → 重新仿真
pub fn verify(bytes: &[u8], keyring: &Keyring) -> Result<VerifyReport, ReplayError> {
    let opened = open(bytes, Some(keyring))?;
    let body = &opened.body;
    let mut expected = body.checkpoints.iter().peekable();
    let mut verified = 0usize;
    let mut mismatch: Option<u64> = None;
    let world = ArenaWorld::replay(body.config.clone(), &body.commands, body.final_tick, |w| {
        if mismatch.is_some() {
            return;
        }
        while let Some(cp) = expected.peek() {
            if cp.tick < w.tick {
                // 检查点 Tick 早于当前：说明检查点乱序或越过了对局终点
                mismatch = Some(cp.tick);
                return;
            }
            if cp.tick > w.tick {
                break;
            }
            if cp.state_root != w.state_hash().to_hex() {
                mismatch = Some(cp.tick);
                return;
            }
            verified += 1;
            expected.next();
        }
    })
    .map_err(|(i, e)| ReplayError::divergence(format!("第 {i} 条命令在重放中被拒绝：{e}")))?;
    if let Some(t) = mismatch {
        return Err(ReplayError::divergence(format!("Tick {t} 检查点状态根不一致")));
    }
    if expected.next().is_some() {
        return Err(ReplayError::divergence("存在未被重放覆盖的检查点"));
    }
    if world.tick != body.final_tick {
        return Err(ReplayError::divergence(format!(
            "重放在 Tick {} 结束，归档声明 {}",
            world.tick, body.final_tick
        )));
    }
    if world.state_hash().to_hex() != body.final_state_root {
        return Err(ReplayError::divergence("终局状态根不一致"));
    }
    if world.outcome != body.outcome {
        return Err(ReplayError::divergence("胜负结果与重放不一致"));
    }
    Ok(VerifyReport {
        match_id: body.match_id.clone(),
        signer_key_id: opened.signer_key_id,
        final_tick: body.final_tick,
        final_state_root: body.final_state_root.clone(),
        commands: body.commands.len(),
        checkpoints_verified: verified,
        outcome: body.outcome.clone(),
    })
}
