// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端标识生成：`world_id` / `match_id` / 实验 ID 均为 UUIDv7（RFC 9562，时间有序，利于索引局部性）。

use std::time::{SystemTime, UNIX_EPOCH};

use rynvek_protocol::WireUuid;

/// 当前 Unix 秒
pub fn unix_now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 当前 Unix 毫秒
pub fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 生成 UUIDv7：48 bit 毫秒时间戳 + 版本 7 + 74 bit 随机（RFC 变体）
pub fn new_uuid_v7() -> WireUuid {
    let mut bytes = [0u8; 16];
    // 操作系统随机源不可用时退化为时间 + 计数器派生（仍唯一，但不再不可预测）
    if getrandom::getrandom(&mut bytes[6..]).is_err() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let h = blake3::hash(&[unix_now_ms().to_le_bytes(), n.to_le_bytes()].concat());
        bytes[6..].copy_from_slice(&h.as_bytes()[..10]);
    }
    bytes[..6].copy_from_slice(&unix_now_ms().to_be_bytes()[2..]);
    bytes[6] = 0x70 | (bytes[6] & 0x0F);
    bytes[8] = 0x80 | (bytes[8] & 0x3F);
    WireUuid(bytes)
}

/// 生成 Principal ID：`[1, 2^63)` 内的 63 bit 值，高 43 bit 为毫秒时间戳，低 20 bit 随机。
///
/// 与 `BIGINT` 主键兼容且大致时间有序；碰撞由存储层唯一约束兜底（调用方重试）。
pub fn new_principal_id() -> u64 {
    let mut r = [0u8; 4];
    if getrandom::getrandom(&mut r).is_err() {
        r = (unix_now_ms() as u32).wrapping_mul(0x9E37_79B9).to_le_bytes();
    }
    let rand = u64::from(u32::from_le_bytes(r)) & 0xF_FFFF;
    (((unix_now_ms() & 0x7FF_FFFF_FFFF) << 20) | rand).max(1)
}

/// 小写十六进制
pub fn hex(bytes: &[u8]) -> String {
    rynvek_security::to_hex(bytes)
}

/// 解析固定长度十六进制
pub fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    rynvek_security::from_hex(text)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_has_version_variant_and_is_time_ordered() {
        let a = new_uuid_v7();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = new_uuid_v7();
        assert_eq!(a.version(), 7);
        assert!(a.is_rfc_variant());
        assert!(a.0[..6] < b.0[..6]);
        assert!(rynvek_protocol::IdempotencyKey::new(a).is_ok());
    }
}
