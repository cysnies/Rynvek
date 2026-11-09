// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

//! 协议层标识与摘要的跨语言文本表示。
//!
//! - [`WireUuid`]：16 字节 UUID，JSON 中以小写连字符形式 `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` 表示；
//! - [`IdempotencyKey`]：强制为 UUIDv7 的命令幂等键（D383–D394）；
//! - [`Digest256`]：32 字节摘要（BLAKE3-256 状态根等），JSON 中以 64 位小写十六进制表示。

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 标识解析错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdParseError {
    /// 文本格式不合法
    Malformed(String),
    /// UUID 版本或变体不符合要求（幂等键必须为 RFC 9562 UUIDv7）
    NotUuidV7(String),
}

impl fmt::Display for IdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(text) => write!(f, "malformed identifier: {text:?}"),
            Self::NotUuidV7(text) => write!(f, "identifier is not an RFC 9562 UUIDv7: {text:?}"),
        }
    }
}

impl std::error::Error for IdParseError {}

/// UUID 文本中连字符所在的字符下标
const UUID_HYPHEN_POSITIONS: [usize; 4] = [8, 13, 18, 23];
/// UUID 连字符文本长度
const UUID_TEXT_LEN: usize = 36;

/// 16 字节 UUID（字节序与 RFC 9562 网络字节序一致）
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WireUuid(pub [u8; 16]);

impl WireUuid {
    /// UUID 版本号（第 6 字节高 4 位）
    pub const fn version(&self) -> u8 {
        self.0[6] >> 4
    }

    /// 是否为 RFC 9562 变体（第 8 字节高 2 位为 `10`）
    pub const fn is_rfc_variant(&self) -> bool {
        self.0[8] & 0xC0 == 0x80
    }
}

impl fmt::Display for WireUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for WireUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WireUuid({self})")
    }
}

impl FromStr for WireUuid {
    type Err = IdParseError;

    /// 仅接受 36 字符的连字符形式（大小写均可）
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let malformed = || IdParseError::Malformed(text.to_owned());
        if text.len() != UUID_TEXT_LEN {
            return Err(malformed());
        }
        for (i, ch) in text.char_indices() {
            if UUID_HYPHEN_POSITIONS.contains(&i) != (ch == '-') {
                return Err(malformed());
            }
        }
        let hex: String = text.chars().filter(|ch| *ch != '-').collect();
        decode_hex::<16>(&hex).map(Self).ok_or_else(malformed)
    }
}

/// 命令幂等键：必须为 UUIDv7（前 48 位为 Unix 毫秒时间戳，天然按时间有序）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyKey(WireUuid);

impl IdempotencyKey {
    /// 校验并包装一个 UUID
    pub fn new(uuid: WireUuid) -> Result<Self, IdParseError> {
        if uuid.version() == 7 && uuid.is_rfc_variant() {
            Ok(Self(uuid))
        } else {
            Err(IdParseError::NotUuidV7(uuid.to_string()))
        }
    }

    /// 底层 UUID
    pub const fn uuid(&self) -> WireUuid {
        self.0
    }

    /// 嵌入的 Unix 毫秒时间戳（前 48 位）
    pub fn unix_ts_ms(&self) -> u64 {
        self.0.0[..6]
            .iter()
            .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte))
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for IdempotencyKey {
    type Err = IdParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text.parse()?)
    }
}

/// 32 字节摘要（BLAKE3-256 状态根、内容 CID 等）
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest256(pub [u8; 32]);

impl fmt::Display for Digest256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for Digest256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest256({self})")
    }
}

impl FromStr for Digest256 {
    type Err = IdParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        decode_hex::<32>(text)
            .map(Self)
            .ok_or_else(|| IdParseError::Malformed(text.to_owned()))
    }
}

/// 把恰好 `2 * N` 个十六进制字符解码为 `N` 字节
fn decode_hex<const N: usize>(hex: &str) -> Option<[u8; N]> {
    if hex.len() != 2 * N || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (byte, pair) in out.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let pair = core::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// 为「以文本形式序列化」的标识类型实现 serde
macro_rules! impl_serde_as_text {
    ($($ty:ty),*) => {$(
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    )*};
}

impl_serde_as_text!(WireUuid, IdempotencyKey, Digest256);

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9562 附录 A.6 的 UUIDv7 示例
    const RFC9562_V7: &str = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";

    #[test]
    fn uuid_text_roundtrip() {
        let uuid: WireUuid = RFC9562_V7.parse().unwrap();
        assert_eq!(uuid.to_string(), RFC9562_V7);
        assert_eq!(uuid.version(), 7);
        assert!(uuid.is_rfc_variant());
        assert_eq!(RFC9562_V7.to_uppercase().parse::<WireUuid>(), Ok(uuid));
    }

    #[test]
    fn uuid_rejects_malformed_text() {
        for bad in [
            "",
            "017f22e279b07cc398c4dc0c0c07398f",
            "017f22e2-79b0-7cc3-98c4-dc0c0c07398g",
            "017f22e2-79b07-cc3-98c4-dc0c0c07398f",
        ] {
            assert!(bad.parse::<WireUuid>().is_err(), "{bad}");
        }
    }

    #[test]
    fn idempotency_key_requires_v7_and_exposes_timestamp() {
        let key: IdempotencyKey = RFC9562_V7.parse().unwrap();
        assert_eq!(key.unix_ts_ms(), 0x017F_22E2_79B0); // 2022-02-22T19:22:22Z
        let v4 = "919108f7-52d1-4320-9bac-f847db4148a8";
        assert_eq!(
            v4.parse::<IdempotencyKey>(),
            Err(IdParseError::NotUuidV7(v4.to_owned()))
        );
    }

    #[test]
    fn digest_hex_roundtrip_and_serde() {
        let digest = Digest256([0xAB; 32]);
        let text = digest.to_string();
        assert_eq!(text.len(), 64);
        assert_eq!(text.parse(), Ok(digest));
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(serde_json::from_str::<Digest256>(&json).unwrap(), digest);
        assert!("ab".parse::<Digest256>().is_err());
    }
}
