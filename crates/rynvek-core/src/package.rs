// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 资源包统一容器（ERRATA E-019 布局；本实现与 `packages/assets/src/rview.ts` 逐字节兼容）：
//!
//! | 偏移 | 长度 | 字段 |
//! |---|---|---|
//! | 0 | 4 | 魔数：`RYWL` / `RYPK` / `RYVW`（规范 8.1） |
//! | 4 | 2 | `format_version` = 1 |
//! | 6 | 2 | `flags` = 0 |
//! | 8 | 2 | `header_bytes` = 64 |
//! | 10 | 2 | `entry_count` |
//! | 12 | 4 | 保留 = 0 |
//! | 16 | 16 | `package_key` = BLAKE3(id)[..16] |
//! | 32 | 8 | `manifest_bytes` |
//! | 40 | 8 | `payload_bytes` |
//! | 48 | 8 | `total_bytes` |
//! | 56 | 8 | 保留 = 0 |
//! | 64 | 32 | `content_hash` = BLAKE3(manifest ‖ payload) |
//! | 96 | — | 清单 JSON（补零至 8 字节对齐），随后为载荷区（条目按路径升序、8 字节对齐紧密排布） |
//!
//! 清单 JSON 字段顺序固定为 `schema, id, version, name, license, engine_compat, entries`，
//! 条目字段顺序为 `path, kind, media_type, offset, length, blake3_256`（与 `JSON.stringify` 一致），
//! 因此同一输入在 Rust 与 TypeScript 中写出逐字节相同的文件。
//! 结构错误 → `0x7001 InvalidFileMagicOrVersion`；哈希不符 → `0x7002 Blake3ChecksumMismatch`。

use std::collections::BTreeMap;

use crate::error::RynvekErrorCode;
use serde::{Deserialize, Serialize};

/// 文件头长度
pub const HEADER_BYTES: usize = 64;
/// 整包内容哈希长度
pub const CONTENT_HASH_BYTES: usize = 32;
/// 区段对齐
pub const ALIGN: usize = 8;
/// 格式版本
pub const FORMAT_VERSION: u16 = 1;
/// 清单上限
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// 整包上限
pub const MAX_TOTAL_BYTES: usize = 256 << 20;

/// 资源包类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Rworld,
    Rpack,
    Rview,
}

impl PackageKind {
    pub const ALL: [Self; 3] = [Self::Rworld, Self::Rpack, Self::Rview];

    /// 规范 8.1 冻结的魔数
    pub const fn magic(self) -> [u8; 4] {
        match self {
            Self::Rworld => *b"RYWL",
            Self::Rpack => *b"RYPK",
            Self::Rview => *b"RYVW",
        }
    }

    /// 清单 `schema` 字段
    pub const fn schema(self) -> &'static str {
        match self {
            Self::Rworld => "rynvek.rworld.v1",
            Self::Rpack => "rynvek.rpack.v1",
            Self::Rview => "rynvek.rview.v1",
        }
    }

    /// `hub_packages.package_kind` 取值
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rworld => "rworld",
            Self::Rpack => "rpack",
            Self::Rview => "rview",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// 由魔数识别
    pub fn from_magic(bytes: &[u8]) -> Option<Self> {
        let m = bytes.get(..4)?;
        Self::ALL.into_iter().find(|k| k.magic() == m)
    }

    /// 各条目类型允许的媒体类型
    pub fn media_types(self, entry_kind: &str) -> Option<&'static [&'static str]> {
        const JSON: &[&str] = &["application/json"];
        match (self, entry_kind) {
            (Self::Rview, "palette" | "theme") => Some(JSON),
            (Self::Rview, "shader") => Some(&["text/wgsl"]),
            (Self::Rview, "sprite_atlas") => Some(&["image/png", "application/json"]),
            (Self::Rpack, "rule_module") => Some(&["application/vnd.rynvek.rir"]),
            (Self::Rpack, "rule_source") => Some(&["text/javascript", "text/typescript"]),
            (Self::Rpack, "plugin") => Some(&["application/wasm"]),
            (Self::Rpack | Self::Rworld, "genome") => Some(JSON),
            (Self::Rpack | Self::Rworld, "data") => Some(&["application/json", "application/octet-stream"]),
            (Self::Rworld, "world_config" | "lockfile") => Some(JSON),
            _ => None,
        }
    }

    /// 允许的许可证：视觉包沿用 E-019（美术 CC0 / CC-BY 或 AGPL）；规则 / 世界包额外允许
    /// 与项目文档 7.1 兼容的宽松代码许可证
    pub const fn licenses(self) -> &'static [&'static str] {
        match self {
            Self::Rview => &["CC0-1.0", "CC-BY-4.0", "AGPL-3.0-only"],
            Self::Rpack | Self::Rworld => &[
                "CC0-1.0",
                "CC-BY-4.0",
                "AGPL-3.0-only",
                "Apache-2.0",
                "Apache-2.0 WITH LLVM-exception",
                "MIT",
            ],
        }
    }
}

/// 容器错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageError {
    pub code: RynvekErrorCode,
    pub detail: String,
}

impl PackageError {
    fn invalid(detail: impl Into<String>) -> Self {
        Self {
            code: RynvekErrorCode::InvalidFileMagicOrVersion,
            detail: detail.into(),
        }
    }

    fn checksum(detail: impl Into<String>) -> Self {
        Self {
            code: RynvekErrorCode::Blake3ChecksumMismatch,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.code, self.detail)
    }
}

impl std::error::Error for PackageError {}

/// 清单条目
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageEntry {
    pub path: String,
    pub kind: String,
    pub media_type: String,
    pub offset: u64,
    pub length: u64,
    pub blake3_256: String,
}

/// 清单（字段顺序即序列化顺序，勿调整）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub schema: String,
    pub id: String,
    pub version: String,
    pub name: String,
    pub license: String,
    pub engine_compat: String,
    pub entries: Vec<PackageEntry>,
}

/// 写入输入条目
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageInputEntry {
    pub path: String,
    pub kind: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

/// 写入输入
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageInput {
    pub kind: PackageKind,
    pub id: String,
    pub version: String,
    pub name: String,
    pub license: String,
    pub engine_compat: String,
    pub entries: Vec<PackageInputEntry>,
}

/// 已校验的资源包
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub kind: PackageKind,
    pub content_hash: [u8; 32],
    pub manifest: PackageManifest,
    /// 路径 → 条目字节
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// 取条目字节
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// 按条目类型筛选（路径升序）
    pub fn entries_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = (&'a PackageEntry, &'a [u8])> + 'a {
        self.manifest
            .entries
            .iter()
            .filter(move |e| e.kind == kind)
            .filter_map(|e| self.files.get(&e.path).map(|d| (e, d.as_slice())))
    }
}

const fn align(n: usize) -> usize {
    n.div_ceil(ALIGN) * ALIGN
}

/// 包 ID 的 16 字节键
pub fn package_key(id: &str) -> [u8; 16] {
    let h = blake3::hash(id.as_bytes());
    let mut k = [0u8; 16];
    k.copy_from_slice(&h.as_bytes()[..16]);
    k
}

/// 包 ID：小写点分命名空间（至少两段），如 `rynvek.visual.industrial-default`
pub fn is_valid_package_id(id: &str) -> bool {
    let seg_ok = |s: &str| {
        !s.is_empty()
            && s.split('-')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    };
    id.split('.').count() >= 2 && id.split('.').all(seg_ok)
}

/// 语义化版本（不含构建元数据）
pub fn is_valid_version(v: &str) -> bool {
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let nums: Vec<&str> = core.split('.').collect();
    let num_ok = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n == "0" || !n.starts_with('0'));
    nums.len() == 3
        && nums.iter().all(|n| num_ok(n))
        && pre.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
}

/// 条目路径：相对路径，各段由 `[a-z0-9_.-]` 组成（首段不含 `.`），不得出现 `.` / `..` 段，须带扩展名
pub fn is_valid_entry_path(p: &str) -> bool {
    if p.is_empty() || p.len() > 256 {
        return false;
    }
    let segs: Vec<&str> = p.split('/').collect();
    let charset = |s: &str, dot: bool| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || (dot && b == b'.'))
    };
    if segs.iter().any(|s| *s == "." || *s == "..") {
        return false;
    }
    let last = segs[segs.len() - 1];
    let ext_ok = last.rsplit_once('.').is_some_and(|(stem, ext)| {
        !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) && !stem.is_empty()
    });
    if segs.len() == 1 {
        // 单段：`[a-z0-9_-]+ \. [a-z0-9]+`
        return last.rsplit_once('.').is_some_and(|(stem, _)| charset(stem, false)) && ext_ok;
    }
    charset(segs[0], false) && segs[1..].iter().all(|s| charset(s, true)) && ext_ok
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_manifest(kind: PackageKind, m: &PackageManifest, payload_bytes: usize) -> Result<(), PackageError> {
    if m.schema != kind.schema() {
        return Err(PackageError::invalid(format!("schema 必须为 {}", kind.schema())));
    }
    if !is_valid_package_id(&m.id) {
        return Err(PackageError::invalid(format!("包 ID 非法：{}", m.id)));
    }
    if !is_valid_version(&m.version) {
        return Err(PackageError::invalid(format!("版本号非法：{}", m.version)));
    }
    // 与 JS `string.length`（UTF-16 码元数）一致
    let name_len = m.name.encode_utf16().count();
    if name_len == 0 || name_len > 128 {
        return Err(PackageError::invalid("name 长度必须在 1..128"));
    }
    if !kind.licenses().contains(&m.license.as_str()) {
        return Err(PackageError::invalid(format!("许可证不在允许列表：{}", m.license)));
    }
    let compat_len = m.engine_compat.encode_utf16().count();
    if compat_len == 0 || compat_len > 64 {
        return Err(PackageError::invalid("engine_compat 非法"));
    }
    if m.entries.len() > 0xFFFF {
        return Err(PackageError::invalid("条目过多"));
    }
    let mut expected = 0usize;
    let mut prev: Option<&str> = None;
    for e in &m.entries {
        if !is_valid_entry_path(&e.path) {
            return Err(PackageError::invalid(format!("条目路径非法：{}", e.path)));
        }
        if let Some(p) = prev {
            if p == e.path {
                return Err(PackageError::invalid(format!("条目路径重复：{}", e.path)));
            }
            if p > e.path.as_str() {
                return Err(PackageError::invalid("条目必须按路径升序排列"));
            }
        }
        prev = Some(&e.path);
        let Some(media) = kind.media_types(&e.kind) else {
            return Err(PackageError::invalid(format!("未知条目类型：{}", e.kind)));
        };
        if !media.contains(&e.media_type.as_str()) {
            return Err(PackageError::invalid(format!(
                "{} 不允许媒体类型 {}",
                e.kind, e.media_type
            )));
        }
        if !is_hex64(&e.blake3_256) {
            return Err(PackageError::invalid(format!(
                "条目 {} 的 blake3_256 必须是 64 位小写十六进制",
                e.path
            )));
        }
        if e.offset != expected as u64 {
            return Err(PackageError::invalid(format!("条目 {} 偏移不连续", e.path)));
        }
        let len = usize::try_from(e.length).map_err(|_| PackageError::invalid("条目长度溢出"))?;
        expected = align(
            expected
                .checked_add(len)
                .ok_or_else(|| PackageError::invalid("条目长度溢出"))?,
        );
    }
    if expected != payload_bytes {
        return Err(PackageError::invalid("条目总长与载荷区长度不符"));
    }
    Ok(())
}

/// 写出资源包；同一输入总是产生逐字节相同的输出（条目按路径排序）
pub fn write_package(input: &PackageInput) -> Result<Vec<u8>, PackageError> {
    let mut sorted: Vec<&PackageInputEntry> = input.entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut cursor = 0usize;
    let entries: Vec<PackageEntry> = sorted
        .iter()
        .map(|e| {
            let entry = PackageEntry {
                path: e.path.clone(),
                kind: e.kind.clone(),
                media_type: e.media_type.clone(),
                offset: cursor as u64,
                length: e.data.len() as u64,
                blake3_256: blake3::hash(&e.data).to_hex().to_string(),
            };
            cursor = align(cursor + e.data.len());
            entry
        })
        .collect();
    let manifest = PackageManifest {
        schema: input.kind.schema().to_owned(),
        id: input.id.clone(),
        version: input.version.clone(),
        name: input.name.clone(),
        license: input.license.clone(),
        engine_compat: input.engine_compat.clone(),
        entries,
    };
    validate_manifest(input.kind, &manifest, cursor)?;
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|e| PackageError::invalid(e.to_string()))?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(PackageError::invalid("清单超过 1 MiB"));
    }
    let manifest_region = align(manifest_bytes.len());
    let total = HEADER_BYTES + CONTENT_HASH_BYTES + manifest_region + cursor;
    if total > MAX_TOTAL_BYTES {
        return Err(PackageError::invalid("资源包超过 256 MiB"));
    }
    let mut payload = vec![0u8; cursor];
    for (e, m) in sorted.iter().zip(&manifest.entries) {
        let off = m.offset as usize;
        payload[off..off + e.data.len()].copy_from_slice(&e.data);
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&input.kind.magic());
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    out.extend_from_slice(&(manifest.entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&package_key(&input.id));
    out.extend_from_slice(&(manifest_bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&(total as u64).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    let mut h = blake3::Hasher::new();
    h.update(&manifest_bytes);
    h.update(&payload);
    out.extend_from_slice(h.finalize().as_bytes());
    out.extend_from_slice(&manifest_bytes);
    out.resize(HEADER_BYTES + CONTENT_HASH_BYTES + manifest_region, 0);
    out.extend_from_slice(&payload);
    debug_assert_eq!(out.len(), total);
    Ok(out)
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// 读取并完整校验资源包（结构、清单、整包与逐条目 BLAKE3）；`expected` 为 None 时按魔数识别类型
pub fn read_package(bytes: &[u8], expected: Option<PackageKind>) -> Result<Package, PackageError> {
    if bytes.len() < HEADER_BYTES + CONTENT_HASH_BYTES {
        return Err(PackageError::invalid("文件过短"));
    }
    let kind = PackageKind::from_magic(bytes).ok_or_else(|| PackageError::invalid("魔数不是 RYWL / RYPK / RYVW"))?;
    if let Some(want) = expected {
        if want != kind {
            return Err(PackageError::invalid(format!(
                "魔数不是 {}",
                String::from_utf8_lossy(&want.magic())
            )));
        }
    }
    let version = u16_at(bytes, 4);
    if version != FORMAT_VERSION {
        return Err(PackageError::invalid(format!("不支持的格式版本 {version}")));
    }
    let flags = u16_at(bytes, 6);
    if flags != 0 {
        return Err(PackageError::invalid(format!("未知标志位 0x{flags:x}")));
    }
    if usize::from(u16_at(bytes, 8)) != HEADER_BYTES {
        return Err(PackageError::invalid("header_bytes 必须为 64"));
    }
    if bytes[12..16] != [0; 4] || u64_at(bytes, 56) != 0 {
        return Err(PackageError::invalid("保留字段必须为 0"));
    }
    let big = |off: usize| -> Result<usize, PackageError> {
        let v = u64_at(bytes, off);
        if v > MAX_TOTAL_BYTES as u64 {
            return Err(PackageError::invalid("长度字段超出上限"));
        }
        Ok(v as usize)
    };
    let (manifest_len, payload_len, total) = (big(32)?, big(40)?, big(48)?);
    if total != bytes.len() {
        return Err(PackageError::invalid(format!(
            "文件长度 {} 与文件头 {total} 不符（截断或尾随数据）",
            bytes.len()
        )));
    }
    if manifest_len == 0 || manifest_len > MAX_MANIFEST_BYTES {
        return Err(PackageError::invalid("清单长度非法"));
    }
    let manifest_start = HEADER_BYTES + CONTENT_HASH_BYTES;
    let payload_start = manifest_start + align(manifest_len);
    if payload_start + payload_len != bytes.len() {
        return Err(PackageError::invalid("区段长度与文件长度不一致"));
    }
    let manifest_bytes = &bytes[manifest_start..manifest_start + manifest_len];
    let payload = &bytes[payload_start..];
    let mut h = blake3::Hasher::new();
    h.update(manifest_bytes);
    h.update(payload);
    let mut content_hash = [0u8; 32];
    content_hash.copy_from_slice(&bytes[HEADER_BYTES..manifest_start]);
    if h.finalize().as_bytes() != &content_hash {
        return Err(PackageError::checksum("整包 content_hash 不符"));
    }
    if bytes[manifest_start + manifest_len..payload_start]
        .iter()
        .any(|&b| b != 0)
    {
        return Err(PackageError::invalid("清单填充字节必须为 0"));
    }
    let text = std::str::from_utf8(manifest_bytes).map_err(|_| PackageError::invalid("清单不是合法的 UTF-8 JSON"))?;
    let manifest: PackageManifest =
        serde_json::from_str(text).map_err(|e| PackageError::invalid(format!("清单结构非法：{e}")))?;
    validate_manifest(kind, &manifest, payload_len)?;
    if manifest.entries.len() != usize::from(u16_at(bytes, 10)) {
        return Err(PackageError::invalid("entry_count 与清单条目数不符"));
    }
    if bytes[16..32] != package_key(&manifest.id) {
        return Err(PackageError::invalid("package_key 与清单 id 不符"));
    }
    let mut files = BTreeMap::new();
    for e in &manifest.entries {
        let (o, l) = (e.offset as usize, e.length as usize);
        let data = &payload[o..o + l];
        if blake3::hash(data).to_hex().as_str() != e.blake3_256 {
            return Err(PackageError::checksum(format!("条目 {} 的 BLAKE3 不符", e.path)));
        }
        files.insert(e.path.clone(), data.to_vec());
    }
    Ok(Package {
        kind,
        content_hash,
        manifest,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: PackageKind) -> PackageInput {
        PackageInput {
            kind,
            id: "community.sample".into(),
            version: "1.2.3".into(),
            name: "样例".into(),
            license: "CC0-1.0".into(),
            engine_compat: ">=1.0.0 <2.0.0".into(),
            entries: vec![
                PackageInputEntry {
                    path: "z/data.json".into(),
                    kind: "data".into(),
                    media_type: "application/json".into(),
                    data: b"{\"a\":1}".to_vec(),
                },
                PackageInputEntry {
                    path: "genomes/a.json".into(),
                    kind: "genome".into(),
                    media_type: "application/json".into(),
                    data: b"[]".to_vec(),
                },
            ],
        }
    }

    #[test]
    fn roundtrip_and_determinism() {
        let i = input(PackageKind::Rpack);
        let a = write_package(&i).unwrap();
        assert_eq!(a, write_package(&i).unwrap());
        assert_eq!(&a[..4], b"RYPK");
        let p = read_package(&a, Some(PackageKind::Rpack)).unwrap();
        assert_eq!(p.manifest.entries[0].path, "genomes/a.json");
        assert_eq!(p.file("z/data.json").unwrap(), b"{\"a\":1}");
        assert_eq!(p.entries_of("genome").count(), 1);
        assert_eq!(a.len() % 8, 0);
    }

    #[test]
    fn rejects_corruption_with_spec_codes() {
        let a = write_package(&input(PackageKind::Rpack)).unwrap();
        let mut flipped = a.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert_eq!(
            read_package(&flipped, None).unwrap_err().code,
            RynvekErrorCode::Blake3ChecksumMismatch
        );
        assert_eq!(
            read_package(&a[..a.len() - 8], None).unwrap_err().code,
            RynvekErrorCode::InvalidFileMagicOrVersion
        );
        assert_eq!(
            read_package(&a, Some(PackageKind::Rview)).unwrap_err().code,
            RynvekErrorCode::InvalidFileMagicOrVersion
        );
        let mut magic = a.clone();
        magic[0] = b'X';
        assert!(read_package(&magic, None).is_err());
    }

    #[test]
    fn kind_specific_validation() {
        // rview 不允许 genome 条目
        assert!(write_package(&input(PackageKind::Rview)).is_err());
        let mut i = input(PackageKind::Rpack);
        i.license = "GPL-2.0-only".into();
        assert!(write_package(&i).is_err());
        let mut i = input(PackageKind::Rpack);
        i.entries[0].path = "../escape.json".into();
        assert!(write_package(&i).is_err());
    }

    #[test]
    fn path_rules_match_ts_pattern() {
        for ok in ["a.json", "shaders/fog.wgsl", "a_b-c/d.e.f/g.png", "x/.hidden.json"] {
            assert!(is_valid_entry_path(ok), "{ok}");
        }
        for bad in [
            "",
            "a",
            "/a.json",
            "a/../b.json",
            "./a.json",
            "A.json",
            "a/b",
            "a\\b.json",
            "a.b.json",
            "a/b.",
        ] {
            assert!(!is_valid_entry_path(bad), "{bad}");
        }
        assert!(is_valid_package_id("rynvek.visual.industrial-default"));
        assert!(!is_valid_package_id("single"));
        assert!(!is_valid_package_id("a..b"));
        assert!(is_valid_version("0.1.0-rc.1"));
        assert!(!is_valid_version("01.0.0"));
    }
}
