// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! [`CapabilityToken`] 的签发、验签与吊销。
//!
//! 布局由 `rynvek-protocol` 冻结（32 字节声明区 + 64 字节签名），签名覆盖声明区原始字节。验签顺序：
//!
//! 1. 结构解码（长度 96、版本、保留字节、权限位）；
//! 2. 按 `signer_key_id` 查公钥并 `verify_strict`（`0x9001` 归一为 `0x8001` 对外，避免泄露密钥细节）；
//! 3. 过期检查（`now < expires_at_unix_s`）；
//! 4. 吊销检查：单个令牌（`principal_id + nonce`）或整个 Principal（账号注销 / 封禁）。
//!
//! 令牌是持有者凭证（Bearer）：传输层必须走 TLS（WebTransport 天然 TLS 1.3；WebSocket 由部署方以 `wss://` 承载）。

use std::collections::BTreeSet;

use rynvek_protocol::capability::{CAPABILITY_TOKEN_VERSION, ED25519_SIGNATURE_LEN};
use rynvek_protocol::{CapabilityScope, CapabilityScopeSet, CapabilityToken};

use crate::error::SecurityError;
use crate::keys::{Keyring, ServerSigningKey};

/// 缺省有效期：1 小时
pub const DEFAULT_TOKEN_TTL_S: u64 = 3_600;
/// 有效期上限：24 小时（长连接靠 `ResumeToken` 续接，而不是长寿令牌）
pub const MAX_TOKEN_TTL_S: u64 = 86_400;

/// 已通过验签的令牌
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedToken {
    pub token: CapabilityToken,
}

impl VerifiedToken {
    pub const fn principal_id(&self) -> u64 {
        self.token.principal_id
    }

    pub const fn scopes(&self) -> CapabilityScopeSet {
        self.token.scopes
    }

    /// 要求包含某权限，否则 `0x8001`
    pub fn require(&self, scope: CapabilityScope) -> Result<(), SecurityError> {
        if self.token.scopes.contains(scope) {
            Ok(())
        } else {
            Err(SecurityError::unauthorized(format!("令牌缺少权限 `{scope}`")))
        }
    }
}

/// 令牌签发与验证中心（服务端单例）
#[derive(Debug)]
pub struct TokenAuthority {
    signer: ServerSigningKey,
    keyring: Keyring,
    next_nonce: u32,
    revoked_tokens: BTreeSet<(u64, u32)>,
    revoked_principals: BTreeSet<u64>,
}

impl TokenAuthority {
    /// 以当前签名密钥构造；其公钥自动登记进公钥环
    pub fn new(signer: ServerSigningKey) -> Self {
        let mut keyring = Keyring::new();
        keyring
            .insert(signer.key_id(), signer.public_key())
            .expect("由合法私钥派生的公钥必然合法");
        // 起始 nonce 取公钥摘要，避免进程重启后从 0 复用
        let digest = blake3::hash(&signer.public_key());
        let next_nonce = u32::from_le_bytes(digest.as_bytes()[..4].try_into().expect("4 字节"));
        Self {
            signer,
            keyring,
            next_nonce,
            revoked_tokens: BTreeSet::new(),
            revoked_principals: BTreeSet::new(),
        }
    }

    pub fn keyring(&self) -> &Keyring {
        &self.keyring
    }

    pub fn keyring_mut(&mut self) -> &mut Keyring {
        &mut self.keyring
    }

    pub fn signer(&self) -> &ServerSigningKey {
        &self.signer
    }

    /// 轮转签名密钥：新密钥开始签发，旧公钥保留在环中直到其签发的令牌全部过期
    pub fn rotate(&mut self, signer: ServerSigningKey) {
        self.keyring
            .insert(signer.key_id(), signer.public_key())
            .expect("由合法私钥派生的公钥必然合法");
        self.signer = signer;
    }

    /// 签发令牌；`ttl_s` 被钳制到 `[1, MAX_TOKEN_TTL_S]`
    pub fn issue(
        &mut self,
        principal_id: u64,
        scopes: CapabilityScopeSet,
        now_unix_s: u64,
        ttl_s: u64,
    ) -> CapabilityToken {
        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.wrapping_add(1);
        let mut token = CapabilityToken {
            version: CAPABILITY_TOKEN_VERSION,
            signer_key_id: self.signer.key_id(),
            nonce,
            principal_id,
            scopes,
            expires_at_unix_s: now_unix_s.saturating_add(ttl_s.clamp(1, MAX_TOKEN_TTL_S)),
            signature: [0; ED25519_SIGNATURE_LEN],
        };
        token.signature = self.signer.sign(&token.claims_bytes());
        token
    }

    /// 验证线上 96 字节令牌
    pub fn verify_bytes(&self, bytes: &[u8], now_unix_s: u64) -> Result<VerifiedToken, SecurityError> {
        let token =
            CapabilityToken::decode(bytes).map_err(|e| SecurityError::unauthorized(format!("令牌格式错误：{e}")))?;
        self.verify(&token, now_unix_s)
    }

    /// 验证已解码令牌
    pub fn verify(&self, token: &CapabilityToken, now_unix_s: u64) -> Result<VerifiedToken, SecurityError> {
        self.keyring
            .verify(token.signer_key_id, &token.claims_bytes(), &token.signature)
            .map_err(|e| SecurityError::unauthorized(format!("令牌签名无效：{}", e.message)))?;
        if !token.is_unexpired_at(now_unix_s) {
            return Err(SecurityError::unauthorized("令牌已过期"));
        }
        if self.revoked_principals.contains(&token.principal_id) {
            return Err(SecurityError::unauthorized("该 Principal 的全部令牌已被吊销"));
        }
        if self.revoked_tokens.contains(&(token.principal_id, token.nonce)) {
            return Err(SecurityError::unauthorized("令牌已被吊销"));
        }
        Ok(VerifiedToken { token: *token })
    }

    /// 吊销单个令牌（登出）
    pub fn revoke_token(&mut self, token: &CapabilityToken) {
        self.revoked_tokens.insert((token.principal_id, token.nonce));
    }

    /// 吊销某 Principal 的全部令牌（账号注销、封禁）
    pub fn revoke_principal(&mut self, principal_id: u64) {
        self.revoked_principals.insert(principal_id);
    }

    /// 解除 Principal 级吊销（解封）
    pub fn reinstate_principal(&mut self, principal_id: u64) {
        self.revoked_principals.remove(&principal_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> TokenAuthority {
        TokenAuthority::new(ServerSigningKey::from_seed(1, [7; 32]))
    }

    fn scopes() -> CapabilityScopeSet {
        [CapabilityScope::WorldRead, CapabilityScope::MatchPlay]
            .into_iter()
            .collect()
    }

    #[test]
    fn issued_token_roundtrips_through_96_bytes() {
        let mut a = authority();
        let t = a.issue(42, scopes(), 1_000, 60);
        let bytes = t.encode();
        assert_eq!(bytes.len(), 96);
        let v = a.verify_bytes(&bytes, 1_059).unwrap();
        assert_eq!(v.principal_id(), 42);
        v.require(CapabilityScope::MatchPlay).unwrap();
        assert_eq!(v.require(CapabilityScope::HubPublish).unwrap_err().code.code(), 0x8001);
    }

    #[test]
    fn expiry_is_exclusive_and_ttl_is_clamped() {
        let mut a = authority();
        let t = a.issue(1, scopes(), 1_000, 60);
        assert!(a.verify(&t, 1_060).is_err());
        let long = a.issue(1, scopes(), 0, u64::MAX);
        assert_eq!(long.expires_at_unix_s, MAX_TOKEN_TTL_S);
    }

    #[test]
    fn every_claim_byte_is_covered_by_the_signature() {
        let mut a = authority();
        let bytes = a.issue(5, scopes(), 0, 600).encode();
        for i in 0..32 {
            let mut tampered = bytes;
            tampered[i] ^= 0x01;
            let err = a.verify_bytes(&tampered, 1).unwrap_err();
            assert_eq!(err.code.code(), 0x8001, "第 {i} 字节被篡改后必须拒绝");
        }
        for i in 32..96 {
            let mut tampered = bytes;
            tampered[i] ^= 0x80;
            assert!(a.verify_bytes(&tampered, 1).is_err(), "签名第 {} 字节", i - 32);
        }
    }

    #[test]
    fn foreign_signer_and_rotation() {
        let mut a = authority();
        let mut rogue = TokenAuthority::new(ServerSigningKey::from_seed(1, [9; 32]));
        let forged = rogue.issue(5, scopes(), 0, 600);
        assert!(a.verify(&forged, 1).is_err(), "同 key_id 不同私钥签发的令牌必须拒绝");

        let before = a.issue(5, scopes(), 0, 600);
        a.rotate(ServerSigningKey::from_seed(2, [8; 32]));
        let after = a.issue(5, scopes(), 0, 600);
        assert_eq!(after.signer_key_id, 2);
        a.verify(&before, 1).unwrap();
        a.verify(&after, 1).unwrap();
        a.keyring_mut().remove(1);
        assert!(a.verify(&before, 1).is_err());
    }

    #[test]
    fn revocation_by_token_and_by_principal() {
        let mut a = authority();
        let t1 = a.issue(5, scopes(), 0, 600);
        let t2 = a.issue(5, scopes(), 0, 600);
        assert_ne!(t1.nonce, t2.nonce);
        a.revoke_token(&t1);
        assert!(a.verify(&t1, 1).is_err());
        a.verify(&t2, 1).unwrap();
        a.revoke_principal(5);
        assert!(a.verify(&t2, 1).is_err());
        a.reinstate_principal(5);
        a.verify(&t2, 1).unwrap();
    }
}
