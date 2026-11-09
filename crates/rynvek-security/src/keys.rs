// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Ed25519 密钥与验签（规范 1.1：签名算法冻结为 Ed25519）。
//!
//! - 服务端以 [`ServerSigningKey`]（`key_id` + 私钥）签发令牌与回放归档；
//! - [`Keyring`] 按 `signer_key_id` 保存历代公钥，轮转后旧令牌在过期前仍可验证（D419 密钥轮转）；
//! - 验签一律使用 `verify_strict`，拒绝非规范编码与小阶公钥，防止签名延展性攻击。

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::error::SecurityError;

/// Ed25519 公钥长度
pub const PUBLIC_KEY_LEN: usize = 32;
/// Ed25519 私钥种子长度
pub const SECRET_KEY_LEN: usize = 32;

/// 带编号的服务端签名密钥
#[derive(Clone)]
pub struct ServerSigningKey {
    key_id: u16,
    key: SigningKey,
}

impl core::fmt::Debug for ServerSigningKey {
    // 私钥绝不进入日志
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServerSigningKey")
            .field("key_id", &self.key_id)
            .field("public_key", &to_hex(&self.public_key()))
            .finish()
    }
}

impl ServerSigningKey {
    /// 由 32 字节种子构造（测试与从密钥文件加载）
    pub fn from_seed(key_id: u16, seed: [u8; SECRET_KEY_LEN]) -> Self {
        Self {
            key_id,
            key: SigningKey::from_bytes(&seed),
        }
    }

    /// 以操作系统 CSPRNG 生成新密钥
    pub fn generate(key_id: u16) -> Result<Self, SecurityError> {
        let mut seed = [0u8; SECRET_KEY_LEN];
        getrandom::getrandom(&mut seed).map_err(|e| SecurityError::unauthorized(format!("无法获取系统随机数：{e}")))?;
        Ok(Self::from_seed(key_id, seed))
    }

    pub const fn key_id(&self) -> u16 {
        self.key_id
    }

    /// 私钥种子（仅用于写入密钥文件）
    pub fn seed(&self) -> [u8; SECRET_KEY_LEN] {
        self.key.to_bytes()
    }

    pub fn public_key(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.key.verifying_key().to_bytes()
    }

    /// 对任意消息签名
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

/// 按 `key_id` 索引的公钥环
#[derive(Clone, Debug, Default)]
pub struct Keyring {
    keys: BTreeMap<u16, VerifyingKey>,
}

impl Keyring {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记公钥；公钥非法（非曲线点 / 小阶点）时报 `0x9001`
    pub fn insert(&mut self, key_id: u16, public_key: [u8; PUBLIC_KEY_LEN]) -> Result<(), SecurityError> {
        let key = parse_public_key(&public_key)?;
        self.keys.insert(key_id, key);
        Ok(())
    }

    /// 撤下某一代公钥（密钥泄露时紧急轮转）
    pub fn remove(&mut self, key_id: u16) -> bool {
        self.keys.remove(&key_id).is_some()
    }

    pub fn contains(&self, key_id: u16) -> bool {
        self.keys.contains_key(&key_id)
    }

    /// 以 `key_id` 对应公钥验证签名
    pub fn verify(&self, key_id: u16, message: &[u8], signature: &[u8; 64]) -> Result<(), SecurityError> {
        let key = self
            .keys
            .get(&key_id)
            .ok_or_else(|| SecurityError::bad_signature(format!("未知签名密钥编号 {key_id}")))?;
        key.verify_strict(message, &Signature::from_bytes(signature))
            .map_err(|_| SecurityError::bad_signature("Ed25519 签名校验失败"))
    }
}

fn parse_public_key(public_key: &[u8; PUBLIC_KEY_LEN]) -> Result<VerifyingKey, SecurityError> {
    let key =
        VerifyingKey::from_bytes(public_key).map_err(|_| SecurityError::bad_signature("Ed25519 公钥不是合法曲线点"))?;
    if key.is_weak() {
        return Err(SecurityError::bad_signature("拒绝小阶 Ed25519 公钥"));
    }
    Ok(key)
}

/// 以给定公钥验证签名（Principal 登录、包发布等一次性验签）
pub fn verify_ed25519(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), SecurityError> {
    let public_key: &[u8; PUBLIC_KEY_LEN] = public_key
        .try_into()
        .map_err(|_| SecurityError::bad_signature("Ed25519 公钥必须为 32 字节"))?;
    let signature: &[u8; 64] = signature
        .try_into()
        .map_err(|_| SecurityError::bad_signature("Ed25519 签名必须为 64 字节"))?;
    parse_public_key(public_key)?
        .verify_strict(message, &Signature::from_bytes(signature))
        .map_err(|_| SecurityError::bad_signature("Ed25519 签名校验失败"))
}

/// 小写十六进制编码
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

/// 十六进制解码（大小写不敏感）；非法输入返回 `None`
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(nibble(pair[0])? << 4 | nibble(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8032 §7.1 TEST 1：空消息
    const RFC8032_SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const RFC8032_PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    const RFC8032_SIGNATURE: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

    #[test]
    fn matches_rfc8032_test_vector_1() {
        let seed: [u8; 32] = from_hex(RFC8032_SECRET).unwrap().try_into().unwrap();
        let key = ServerSigningKey::from_seed(1, seed);
        assert_eq!(to_hex(&key.public_key()), RFC8032_PUBLIC);
        assert_eq!(to_hex(&key.sign(b"")), RFC8032_SIGNATURE);
        verify_ed25519(&key.public_key(), b"", &key.sign(b"")).unwrap();
    }

    #[test]
    fn keyring_rotation_and_tamper_detection() {
        let old = ServerSigningKey::from_seed(1, [1; 32]);
        let new = ServerSigningKey::from_seed(2, [2; 32]);
        let mut ring = Keyring::new();
        ring.insert(1, old.public_key()).unwrap();
        ring.insert(2, new.public_key()).unwrap();
        let sig = old.sign(b"claims");
        ring.verify(1, b"claims", &sig).unwrap();
        assert_eq!(ring.verify(2, b"claims", &sig).unwrap_err().code.code(), 0x9001);
        assert_eq!(ring.verify(1, b"claimz", &sig).unwrap_err().code.code(), 0x9001);
        assert!(ring.remove(1));
        assert_eq!(ring.verify(1, b"claims", &sig).unwrap_err().code.code(), 0x9001);
    }

    #[test]
    fn rejects_weak_public_keys_and_bad_lengths() {
        // 单位元（小阶点）编码
        let mut identity = [0u8; 32];
        identity[0] = 1;
        let mut ring = Keyring::new();
        assert!(ring.insert(9, identity).is_err());
        assert!(verify_ed25519(&[0u8; 31], b"", &[0u8; 64]).is_err());
        assert!(verify_ed25519(&identity, b"", &[0u8; 63]).is_err());
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(to_hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(from_hex("00ABff"), Some(vec![0x00, 0xab, 0xff]));
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn debug_never_prints_secret() {
        let key = ServerSigningKey::from_seed(3, [0x42; 32]);
        let dbg = format!("{key:?}");
        assert!(!dbg.contains(&to_hex(&[0x42; 32])));
    }
}
