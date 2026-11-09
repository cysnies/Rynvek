// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

//! 96 字节二进制能力令牌 `CapabilityToken`（规范 11 §11.3 路由 `03`、GAP-23）。
//!
//! 令牌由 32 字节声明区与 64 字节 Ed25519 签名组成，签名覆盖完整声明区。
//! 本模块只定义布局与编解码；签名的生成与验证由 `rynvek-security` 负责。
//!
//! | 偏移 | 长度 | 字段 | 说明 |
//! | :--- | :--- | :--- | :--- |
//! | 0 | 1 | `version` | 令牌格式版本，当前为 [`CAPABILITY_TOKEN_VERSION`] |
//! | 1 | 1 | 保留 | 必须为 0 |
//! | 2 | 2 | `signer_key_id` | 签发密钥编号（支持密钥轮转，D419） |
//! | 4 | 4 | `nonce` | 签发方随机数，防止重放 |
//! | 8 | 8 | `principal_id` | 被授权的 `Principal` |
//! | 16 | 8 | `scopes` | [`CapabilityScopeSet`] 位集 |
//! | 24 | 8 | `expires_at_unix_s` | 过期时刻（Unix 秒，不含） |
//! | 32 | 64 | `signature` | 对字节 `0..32` 的 Ed25519 签名 |

use core::fmt;

/// 令牌总长度（字节）
pub const CAPABILITY_TOKEN_LEN: usize = 96;
/// 被签名的声明区长度（字节）
pub const CAPABILITY_CLAIMS_LEN: usize = 32;
/// Ed25519 签名长度（字节）
pub const ED25519_SIGNATURE_LEN: usize = 64;
/// 当前令牌格式版本
pub const CAPABILITY_TOKEN_VERSION: u8 = 1;

/// 服务端 API 权限范围（与规范 11.3 路由表的 `CapabilityScope` 一一对应；`Public` 路由无需权限）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityScope {
    WorldList = 0,
    WorldCreate = 1,
    WorldRead = 2,
    WorldFork = 3,
    WorldEdit = 4,
    WorldConnect = 5,
    MatchPlay = 6,
    HubPublish = 7,
    LabSubmit = 8,
    LabRead = 9,
    AccountOwner = 10,
}

impl CapabilityScope {
    /// 全部权限范围（按位序号升序）
    pub const ALL: [Self; 11] = [
        Self::WorldList,
        Self::WorldCreate,
        Self::WorldRead,
        Self::WorldFork,
        Self::WorldEdit,
        Self::WorldConnect,
        Self::MatchPlay,
        Self::HubPublish,
        Self::LabSubmit,
        Self::LabRead,
        Self::AccountOwner,
    ];

    /// 规范中的点分名称，例如 `world.read`
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorldList => "world.list",
            Self::WorldCreate => "world.create",
            Self::WorldRead => "world.read",
            Self::WorldFork => "world.fork",
            Self::WorldEdit => "world.edit",
            Self::WorldConnect => "world.connect",
            Self::MatchPlay => "match.play",
            Self::HubPublish => "hub.publish",
            Self::LabSubmit => "lab.submit",
            Self::LabRead => "lab.read",
            Self::AccountOwner => "account.owner",
        }
    }

    /// 由点分名称解析
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| scope.as_str() == name)
    }

    /// 该权限在位集中的掩码
    pub const fn bit(self) -> u64 {
        1 << (self as u8)
    }
}

impl fmt::Display for CapabilityScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 权限范围位集
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CapabilityScopeSet(u64);

impl CapabilityScopeSet {
    /// 空集
    pub const EMPTY: Self = Self(0);
    /// 全部已定义权限位的掩码
    pub const KNOWN_MASK: u64 = (1 << CapabilityScope::ALL.len()) - 1;

    /// 由原始位构造；包含未定义位时返回 `None`
    pub const fn from_bits(bits: u64) -> Option<Self> {
        if bits & !Self::KNOWN_MASK == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// 原始位
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// 返回加入 `scope` 后的新位集
    pub const fn with(self, scope: CapabilityScope) -> Self {
        Self(self.0 | scope.bit())
    }

    /// 是否包含 `scope`
    pub const fn contains(self, scope: CapabilityScope) -> bool {
        self.0 & scope.bit() != 0
    }

    /// 按位序号升序遍历包含的权限
    pub fn iter(self) -> impl Iterator<Item = CapabilityScope> {
        CapabilityScope::ALL
            .into_iter()
            .filter(move |scope| self.contains(*scope))
    }
}

impl FromIterator<CapabilityScope> for CapabilityScopeSet {
    fn from_iter<I: IntoIterator<Item = CapabilityScope>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

/// 令牌解码错误
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityTokenError {
    /// 长度不是 96 字节
    InvalidLength(usize),
    /// 不支持的令牌格式版本
    UnsupportedVersion(u8),
    /// 保留字节非 0
    ReservedByteSet(u8),
    /// 权限位集包含未定义位
    UnknownScopeBits(u64),
}

impl fmt::Display for CapabilityTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength(len) => write!(f, "capability token must be 96 bytes, got {len}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported capability token version {v}"),
            Self::ReservedByteSet(b) => write!(f, "capability token reserved byte must be 0, got {b}"),
            Self::UnknownScopeBits(bits) => write!(f, "capability token contains unknown scope bits {bits:#x}"),
        }
    }
}

impl std::error::Error for CapabilityTokenError {}

/// 96 字节二进制能力令牌
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityToken {
    pub version: u8,
    pub signer_key_id: u16,
    pub nonce: u32,
    pub principal_id: u64,
    pub scopes: CapabilityScopeSet,
    pub expires_at_unix_s: u64,
    pub signature: [u8; ED25519_SIGNATURE_LEN],
}

impl CapabilityToken {
    /// 被签名的 32 字节声明区（签发与验签双方都必须对这段字节操作）
    pub fn claims_bytes(&self) -> [u8; CAPABILITY_CLAIMS_LEN] {
        let mut claims = [0u8; CAPABILITY_CLAIMS_LEN];
        claims[0] = self.version;
        claims[1] = 0; // 保留字节
        claims[2..4].copy_from_slice(&self.signer_key_id.to_le_bytes());
        claims[4..8].copy_from_slice(&self.nonce.to_le_bytes());
        claims[8..16].copy_from_slice(&self.principal_id.to_le_bytes());
        claims[16..24].copy_from_slice(&self.scopes.bits().to_le_bytes());
        claims[24..32].copy_from_slice(&self.expires_at_unix_s.to_le_bytes());
        claims
    }

    /// 编码为 96 字节
    pub fn encode(&self) -> [u8; CAPABILITY_TOKEN_LEN] {
        let mut out = [0u8; CAPABILITY_TOKEN_LEN];
        out[..CAPABILITY_CLAIMS_LEN].copy_from_slice(&self.claims_bytes());
        out[CAPABILITY_CLAIMS_LEN..].copy_from_slice(&self.signature);
        out
    }

    /// 从恰好 96 字节解码并校验结构（不验证签名）
    pub fn decode(bytes: &[u8]) -> Result<Self, CapabilityTokenError> {
        let bytes: &[u8; CAPABILITY_TOKEN_LEN] = bytes
            .try_into()
            .map_err(|_| CapabilityTokenError::InvalidLength(bytes.len()))?;
        if bytes[0] != CAPABILITY_TOKEN_VERSION {
            return Err(CapabilityTokenError::UnsupportedVersion(bytes[0]));
        }
        if bytes[1] != 0 {
            return Err(CapabilityTokenError::ReservedByteSet(bytes[1]));
        }
        let scope_bits = read_u64_le(bytes, 16);
        let scopes =
            CapabilityScopeSet::from_bits(scope_bits).ok_or(CapabilityTokenError::UnknownScopeBits(scope_bits))?;
        let mut signature = [0u8; ED25519_SIGNATURE_LEN];
        signature.copy_from_slice(&bytes[CAPABILITY_CLAIMS_LEN..]);
        Ok(Self {
            version: bytes[0],
            signer_key_id: u16::from_le_bytes([bytes[2], bytes[3]]),
            nonce: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            principal_id: read_u64_le(bytes, 8),
            scopes,
            expires_at_unix_s: read_u64_le(bytes, 24),
            signature,
        })
    }

    /// 在 `now_unix_s` 时刻是否仍未过期
    pub const fn is_unexpired_at(&self, now_unix_s: u64) -> bool {
        now_unix_s < self.expires_at_unix_s
    }

    /// 在 `now_unix_s` 时刻是否授予 `scope`（不含签名验证）
    pub const fn grants(&self, scope: CapabilityScope, now_unix_s: u64) -> bool {
        self.is_unexpired_at(now_unix_s) && self.scopes.contains(scope)
    }
}

/// 读取 `offset` 处的 Little-Endian u64（调用方保证不越界）
fn read_u64_le(bytes: &[u8; CAPABILITY_TOKEN_LEN], offset: usize) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CapabilityToken {
        CapabilityToken {
            version: CAPABILITY_TOKEN_VERSION,
            signer_key_id: 7,
            nonce: 0xDEAD_BEEF,
            principal_id: 1001,
            scopes: [CapabilityScope::WorldRead, CapabilityScope::WorldConnect]
                .into_iter()
                .collect(),
            expires_at_unix_s: 1_800_000_000,
            signature: [0x5A; ED25519_SIGNATURE_LEN],
        }
    }

    /// 跨语言字节向量：TypeScript `@rynvek/protocol` 的测试使用完全相同的期望声明区
    const SAMPLE_CLAIMS_HEX: &str = "01000700efbeaddee903000000000000240000000000000000d2496b00000000";

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
    }

    #[test]
    fn claims_match_reference_bytes() {
        assert_eq!(hex(&sample().claims_bytes()), SAMPLE_CLAIMS_HEX);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let encoded = sample().encode();
        assert_eq!(encoded.len(), CAPABILITY_TOKEN_LEN);
        assert_eq!(&encoded[32..], &[0x5A; 64]);
        assert_eq!(CapabilityToken::decode(&encoded), Ok(sample()));
    }

    #[test]
    fn decode_rejects_malformed_tokens() {
        let good = sample().encode();
        assert_eq!(
            CapabilityToken::decode(&good[..95]),
            Err(CapabilityTokenError::InvalidLength(95))
        );

        let mut bad = good;
        bad[0] = 9;
        assert_eq!(
            CapabilityToken::decode(&bad),
            Err(CapabilityTokenError::UnsupportedVersion(9))
        );

        let mut bad = good;
        bad[1] = 1;
        assert_eq!(
            CapabilityToken::decode(&bad),
            Err(CapabilityTokenError::ReservedByteSet(1))
        );

        let mut bad = good;
        bad[17] = 0x80; // 第 15 位：未定义权限
        assert_eq!(
            CapabilityToken::decode(&bad),
            Err(CapabilityTokenError::UnknownScopeBits(0x8024))
        );
    }

    #[test]
    fn grants_respects_scope_and_expiry() {
        let token = sample();
        assert!(token.grants(CapabilityScope::WorldRead, 1_799_999_999));
        assert!(!token.grants(CapabilityScope::WorldRead, 1_800_000_000));
        assert!(!token.grants(CapabilityScope::HubPublish, 0));
    }

    #[test]
    fn scope_names_roundtrip() {
        for scope in CapabilityScope::ALL {
            assert_eq!(CapabilityScope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(CapabilityScope::parse("public"), None);
        assert_eq!(CapabilityScopeSet::KNOWN_MASK, 0x7FF);
        let set: CapabilityScopeSet = [CapabilityScope::LabRead, CapabilityScope::WorldList]
            .into_iter()
            .collect();
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![CapabilityScope::WorldList, CapabilityScope::LabRead]
        );
    }
}
