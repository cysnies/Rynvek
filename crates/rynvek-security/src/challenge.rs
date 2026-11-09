// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `/api/v1/auth/session` 的 Ed25519 质询-应答登录（规范 11.3 路由 `03`）。
//!
//! 1. 客户端提交 `principal_id`，服务端返回 32 字节随机质询（有效 [`AUTH_CHALLENGE_TTL_S`] 秒，一次性）；
//! 2. 客户端用 `principals.ed25519_pubkey` 对应私钥签名 [`challenge_message`]；
//! 3. 服务端核对质询属于该 Principal、未过期、未使用，再验签，成功后签发 `CapabilityToken`。
//!
//! 被签名消息带域分隔前缀 `rynvek-auth/1\0` 并绑定 `principal_id`，签名不能被挪用到其他协议或账号。
//! 质询由 `BLAKE3-keyed(进程密钥, 计数器)` 生成：不可预测、零碰撞，且测试可注入固定密钥。

use std::collections::BTreeMap;

use crate::error::SecurityError;
use crate::keys::verify_ed25519;

/// 质询有效期（秒）
pub const AUTH_CHALLENGE_TTL_S: u64 = 60;
/// 同时挂起的质询上限（防内存耗尽：超出时淘汰最早的）
pub const MAX_PENDING_CHALLENGES: usize = 65_536;
/// 被签名消息的域分隔前缀
pub const AUTH_DOMAIN: &[u8] = b"rynvek-auth/1\0";

/// 构造被签名的质询消息：`AUTH_DOMAIN || challenge(32) || principal_id(u64 LE)`
pub fn challenge_message(principal_id: u64, challenge: &[u8; 32]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(AUTH_DOMAIN.len() + 40);
    msg.extend_from_slice(AUTH_DOMAIN);
    msg.extend_from_slice(challenge);
    msg.extend_from_slice(&principal_id.to_le_bytes());
    msg
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    principal_id: u64,
    expires_at: u64,
    serial: u64,
}

/// 挂起质询表
#[derive(Debug)]
pub struct AuthChallenges {
    secret: [u8; 32],
    counter: u64,
    pending: BTreeMap<[u8; 32], Pending>,
}

impl AuthChallenges {
    /// 以系统随机密钥构造
    pub fn new() -> Result<Self, SecurityError> {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret)
            .map_err(|e| SecurityError::unauthorized(format!("无法获取系统随机数：{e}")))?;
        Ok(Self::with_secret(secret))
    }

    /// 以固定密钥构造（测试）
    pub fn with_secret(secret: [u8; 32]) -> Self {
        Self {
            secret,
            counter: 0,
            pending: BTreeMap::new(),
        }
    }

    /// 为 Principal 签发一个质询
    pub fn issue(&mut self, principal_id: u64, now_unix_s: u64) -> [u8; 32] {
        self.pending.retain(|_, p| p.expires_at > now_unix_s);
        if self.pending.len() >= MAX_PENDING_CHALLENGES {
            if let Some(oldest) = self.pending.iter().min_by_key(|(_, p)| p.serial).map(|(k, _)| *k) {
                self.pending.remove(&oldest);
            }
        }
        self.counter += 1;
        let mut input = [0u8; 16];
        input[..8].copy_from_slice(&self.counter.to_le_bytes());
        input[8..].copy_from_slice(&principal_id.to_le_bytes());
        let challenge = *blake3::keyed_hash(&self.secret, &input).as_bytes();
        self.pending.insert(
            challenge,
            Pending {
                principal_id,
                expires_at: now_unix_s + AUTH_CHALLENGE_TTL_S,
                serial: self.counter,
            },
        );
        challenge
    }

    /// 消费质询并验签；无论成败质询都被作废（一次性）
    pub fn redeem(
        &mut self,
        principal_id: u64,
        challenge: &[u8; 32],
        public_key: &[u8],
        signature: &[u8],
        now_unix_s: u64,
    ) -> Result<(), SecurityError> {
        let pending = self
            .pending
            .remove(challenge)
            .ok_or_else(|| SecurityError::unauthorized("质询不存在或已被使用"))?;
        if pending.principal_id != principal_id {
            return Err(SecurityError::unauthorized("质询不属于该 Principal"));
        }
        if pending.expires_at <= now_unix_s {
            return Err(SecurityError::unauthorized("质询已过期"));
        }
        verify_ed25519(public_key, &challenge_message(principal_id, challenge), signature)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::ServerSigningKey;

    #[test]
    fn full_challenge_flow_and_single_use() {
        let user = ServerSigningKey::from_seed(0, [3; 32]);
        let mut c = AuthChallenges::with_secret([1; 32]);
        let ch = c.issue(77, 100);
        let sig = user.sign(&challenge_message(77, &ch));
        c.redeem(77, &ch, &user.public_key(), &sig, 120).unwrap();
        let again = c.redeem(77, &ch, &user.public_key(), &sig, 120).unwrap_err();
        assert_eq!(again.code.code(), 0x8001, "质询只能使用一次");
    }

    #[test]
    fn rejects_wrong_principal_expired_and_bad_signature() {
        let user = ServerSigningKey::from_seed(0, [3; 32]);
        let other = ServerSigningKey::from_seed(0, [4; 32]);
        let mut c = AuthChallenges::with_secret([1; 32]);

        let ch = c.issue(77, 100);
        let sig = user.sign(&challenge_message(77, &ch));
        assert_eq!(
            c.redeem(78, &ch, &user.public_key(), &sig, 101)
                .unwrap_err()
                .code
                .code(),
            0x8001
        );

        let ch = c.issue(77, 100);
        let sig = user.sign(&challenge_message(77, &ch));
        assert_eq!(
            c.redeem(77, &ch, &user.public_key(), &sig, 160)
                .unwrap_err()
                .code
                .code(),
            0x8001
        );

        let ch = c.issue(77, 100);
        let sig = other.sign(&challenge_message(77, &ch));
        assert_eq!(
            c.redeem(77, &ch, &user.public_key(), &sig, 101)
                .unwrap_err()
                .code
                .code(),
            0x9001
        );

        // 签名绑定了 principal_id：为 78 签的消息不能用于 77
        let ch = c.issue(77, 100);
        let sig = user.sign(&challenge_message(78, &ch));
        assert_eq!(
            c.redeem(77, &ch, &user.public_key(), &sig, 101)
                .unwrap_err()
                .code
                .code(),
            0x9001
        );
    }

    #[test]
    fn challenges_are_unique_and_expired_ones_are_swept() {
        let mut c = AuthChallenges::with_secret([1; 32]);
        let a = c.issue(1, 0);
        let b = c.issue(1, 0);
        assert_ne!(a, b);
        assert_eq!(c.pending_len(), 2);
        c.issue(2, AUTH_CHALLENGE_TTL_S + 1);
        assert_eq!(c.pending_len(), 1);
    }
}
