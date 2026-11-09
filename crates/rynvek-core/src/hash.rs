// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! BLAKE3-256 内容哈希（规范 01 §1.1「密码学与压缩」）。
//!
//! 用于状态根（State Root）、内容寻址存储（CAS）与资源包完整性校验。

use core::fmt;

use crate::error::RynvekErrorCode;

/// BLAKE3-256 摘要字节数
pub const BLAKE3_DIGEST_LEN: usize = 32;

/// 32 字节 BLAKE3-256 摘要
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Blake3Hash(pub [u8; BLAKE3_DIGEST_LEN]);

impl Blake3Hash {
    /// 计算一段字节的 BLAKE3-256 摘要
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// 按顺序拼接多个子根后计算摘要（例如由 6 个子根计算最终状态根）
    pub fn of_concatenated(parts: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for part in parts {
            hasher.update(part);
        }
        Self(*hasher.finalize().as_bytes())
    }

    /// 原始摘要字节
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; BLAKE3_DIGEST_LEN] {
        &self.0
    }

    /// 小写十六进制表示（64 个字符）
    pub fn to_hex(&self) -> String {
        blake3::Hash::from_bytes(self.0).to_hex().to_string()
    }

    /// 由 64 个字符的十六进制字符串解析（大小写均可）
    pub fn from_hex(hex: &str) -> Result<Self, RynvekErrorCode> {
        blake3::Hash::from_hex(hex)
            .map(|hash| Self(*hash.as_bytes()))
            .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)
    }

    /// 校验数据与期望摘要是否一致
    pub fn verify(&self, bytes: &[u8]) -> Result<(), RynvekErrorCode> {
        // blake3::Hash 的相等比较为常数时间，避免时序侧信道
        if blake3::Hash::from_bytes(self.0) == blake3::hash(bytes) {
            Ok(())
        } else {
            Err(RynvekErrorCode::Blake3ChecksumMismatch)
        }
    }
}

impl fmt::Debug for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blake3Hash({})", self.to_hex())
    }
}

impl fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BLAKE3 官方测试向量：空输入的摘要
    const EMPTY_INPUT_HEX: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    #[test]
    fn empty_input_matches_official_vector() {
        assert_eq!(Blake3Hash::of(b"").to_hex(), EMPTY_INPUT_HEX);
    }

    #[test]
    fn hex_roundtrip() {
        let hash = Blake3Hash::of(b"rynvek");
        assert_eq!(Blake3Hash::from_hex(&hash.to_hex()), Ok(hash));
        assert_eq!(Blake3Hash::from_hex("zz"), Err(RynvekErrorCode::Blake3ChecksumMismatch));
    }

    #[test]
    fn concatenation_equals_single_shot() {
        assert_eq!(
            Blake3Hash::of_concatenated(&[b"ryn", b"vek"]),
            Blake3Hash::of(b"rynvek")
        );
    }

    #[test]
    fn verify_detects_tampering() {
        let hash = Blake3Hash::of(b"payload");
        assert_eq!(hash.verify(b"payload"), Ok(()));
        assert_eq!(hash.verify(b"pay1oad"), Err(RynvekErrorCode::Blake3ChecksumMismatch));
    }
}
