// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 离线工具：`.rreplay` 验证 / 检视与 16 字节帧解码。

use std::path::Path;

use rynvek_network::FrameDecoder;
use rynvek_protocol::WireFrameFlags;
use rynvek_security::Keyring;
use rynvek_server::client;
use rynvek_server::replay::{self, ReplayError};

use crate::{CliError, CliResult, VerifyArgs};

impl From<ReplayError> for CliError {
    fn from(e: ReplayError) -> Self {
        CliError::with_code(e.code.code(), e.message)
    }
}

fn read(path: &Path) -> CliResult<Vec<u8>> {
    std::fs::read(path).map_err(|e| CliError::new(format!("无法读取 {}：{e}", path.display())))
}

/// 解析 `KEY_ID:HEX`
pub fn parse_signer(text: &str) -> CliResult<(u16, [u8; 32])> {
    let (id, hex) = text
        .split_once(':')
        .ok_or_else(|| CliError::new(format!("--signer 应为 KEY_ID:HEX，收到 `{text}`")))?;
    let id: u16 = id
        .parse()
        .map_err(|_| CliError::new(format!("签名密钥编号 `{id}` 不是 u16")))?;
    let key: [u8; 32] = rynvek_security::from_hex(hex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| CliError::new("签名公钥必须是 32 字节十六进制"))?;
    Ok((id, key))
}

/// 从服务器下载回放与其公布的签名公钥
async fn download(server: &str, match_id: &str, token: Option<&str>) -> CliResult<(Vec<u8>, (u16, [u8; 32]))> {
    let base = server.trim_end_matches('/');
    let detail = client::get(&format!("{base}/api/v1/matches/{match_id}"), token)
        .await
        .map_err(CliError::new)?;
    let v = detail.json().map_err(CliError::new)?;
    if detail.status != 200 {
        return Err(remote_error(&v));
    }
    let signer = &v["replay_signer"];
    let key_id = signer["key_id"].as_u64().and_then(|k| u16::try_from(k).ok());
    let key = signer["ed25519_public_key"].as_str();
    let (Some(key_id), Some(key)) = (key_id, key) else {
        return Err(CliError::new("服务器未公布回放签名公钥"));
    };
    let signer = parse_signer(&format!("{key_id}:{key}"))?;
    let r = client::get(&format!("{base}/api/v1/matches/{match_id}/replay"), token)
        .await
        .map_err(CliError::new)?;
    if r.status != 200 {
        return Err(remote_error(&r.json().unwrap_or_default()));
    }
    Ok((r.body, signer))
}

/// 服务端错误体 → CliError
pub fn remote_error(v: &serde_json::Value) -> CliError {
    let message = v["message"].as_str().unwrap_or("服务器返回错误").to_owned();
    match v["code"].as_u64().and_then(|c| u16::try_from(c).ok()) {
        Some(c) => CliError::with_code(c, message),
        None => CliError::new(message),
    }
}

/// `replay verify`
pub async fn verify(a: VerifyArgs) -> CliResult {
    let mut ring = Keyring::new();
    for s in &a.signers {
        let (id, key) = parse_signer(s)?;
        ring.insert(id, key)
            .map_err(|e| CliError::with_code(e.code.code(), e.message))?;
    }
    let bytes = match (&a.file, &a.match_id, &a.server) {
        (Some(f), _, _) => read(f)?,
        (None, Some(m), Some(s)) => {
            let (bytes, (id, key)) = download(s, m, a.token.as_deref()).await?;
            ring.insert(id, key)
                .map_err(|e| CliError::with_code(e.code.code(), e.message))?;
            bytes
        }
        _ => return Err(CliError::new("需要回放文件或 --match 与 --server")),
    };
    if a.file.is_some() && a.signers.is_empty() {
        return Err(CliError::new("离线验证至少需要一个 --signer KEY_ID:HEX 受信公钥"));
    }
    let report = replay::verify(&bytes, &ring)?;
    crate::print_json(&serde_json::json!({
        "verified": true,
        "match_id": report.match_id,
        "signer_key_id": report.signer_key_id,
        "final_tick": report.final_tick,
        "final_state_root": report.final_state_root,
        "commands": report.commands,
        "checkpoints_verified": report.checkpoints_verified,
        "outcome": report.outcome,
    }));
    Ok(())
}

/// `replay inspect`
pub fn inspect(path: &Path) -> CliResult {
    let bytes = read(path)?;
    let opened = replay::open(&bytes, None)?;
    let h = opened.header;
    let b = &opened.body;
    crate::print_json(&serde_json::json!({
        "header": {
            "format_version": h.format_version,
            "flags": h.flags,
            "chunk_count": h.chunk_count,
            "world_id": rynvek_protocol::WireUuid(h.world_id).to_string(),
            "tick": h.tick,
            "level": h.level,
            "payload_bytes": h.payload_bytes,
        },
        "signer_key_id": opened.signer_key_id,
        "signature_checked": false,
        "body": {
            "format": b.format,
            "match_id": b.match_id,
            "world_id": b.world_id,
            "ruleset_hash": b.ruleset_hash,
            "participants": b.participants,
            "commands": b.commands.len(),
            "checkpoints": b.checkpoints.len(),
            "final_tick": b.final_tick,
            "final_state_root": b.final_state_root,
            "outcome": b.outcome,
        },
    }));
    Ok(())
}

fn flag_names(flags: WireFrameFlags) -> Vec<&'static str> {
    let mut out = Vec::new();
    if flags.contains(WireFrameFlags::COMPRESSED_LZ4) {
        out.push("COMPRESSED_LZ4");
    }
    if flags.contains(WireFrameFlags::KEYFRAME) {
        out.push("KEYFRAME");
    }
    out
}

/// 解码帧序列为 JSON
pub fn decode_frames(bytes: &[u8]) -> CliResult<serde_json::Value> {
    let mut dec = FrameDecoder::new();
    dec.push(bytes);
    let mut frames = Vec::new();
    loop {
        match dec.next_frame() {
            Ok(Some(f)) => {
                let h = f.header;
                let payload = f
                    .decoded_payload()
                    .map_err(|e| CliError::with_code(e.code().code(), e.to_string()))?;
                let text = std::str::from_utf8(&payload)
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok());
                frames.push(serde_json::json!({
                    "version": h.version,
                    "channel": format!("{:?}", h.channel),
                    "channel_id": h.channel as u8,
                    "flags": flag_names(h.flags),
                    "payload_len": h.payload_len,
                    "decoded_len": payload.len(),
                    "sequence": h.sequence,
                    "tick_low32": h.tick_low32,
                    "payload_json": text,
                }));
            }
            Ok(None) => break,
            Err(e) => return Err(CliError::with_code(e.code().code(), e.to_string())),
        }
    }
    if dec.buffered() != 0 {
        return Err(CliError::with_code(
            0x8001,
            format!("末尾有 {} 字节不足以构成完整帧", dec.buffered()),
        ));
    }
    Ok(serde_json::json!({ "frames": frames }))
}

/// `frame decode`
pub fn frame_decode(hex: Option<&str>, file: Option<&Path>) -> CliResult {
    let bytes = match (hex, file) {
        (_, Some(f)) => read(f)?,
        (Some(h), None) => {
            let clean: String = h.chars().filter(|c| !c.is_whitespace()).collect();
            rynvek_security::from_hex(&clean).ok_or_else(|| CliError::new("不是合法十六进制"))?
        }
        (None, None) => return Err(CliError::new("需要十六进制参数或 --file")),
    };
    crate::print_json(&decode_frames(&bytes)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_network::FrameEncoder;
    use rynvek_protocol::WireChannelId;

    #[test]
    fn decodes_frames_and_rejects_bad_headers() {
        let mut enc = FrameEncoder::new(true);
        let mut bytes = enc
            .encode(WireChannelId::Event, 7, br#"{"a":1}"#, false)
            .unwrap()
            .to_bytes();
        bytes.extend(
            enc.encode(WireChannelId::StateStream, 8, &[0u8; 512], true)
                .unwrap()
                .to_bytes(),
        );
        let v = decode_frames(&bytes).unwrap();
        let f = v["frames"].as_array().unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0]["channel"], "Event");
        assert_eq!(f[0]["payload_json"]["a"], 1);
        assert_eq!(f[1]["sequence"], 0);
        assert_eq!(f[1]["decoded_len"], 512);
        assert!(f[1]["flags"].as_array().unwrap().iter().any(|x| x == "KEYFRAME"));
        // 截断与坏版本
        assert_eq!(decode_frames(&bytes[..20]).unwrap_err().code, Some(0x8001));
        let mut bad = bytes.clone();
        bad[0] = 0xEE;
        assert_eq!(decode_frames(&bad).unwrap_err().code, Some(0x8001));
    }

    #[test]
    fn signer_parsing() {
        assert_eq!(parse_signer(&format!("3:{}", "ab".repeat(32))).unwrap().0, 3);
        assert!(parse_signer("x:00").is_err());
        assert!(parse_signer("1:00").is_err());
    }
}
