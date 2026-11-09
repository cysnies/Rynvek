// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Schema 注册表（`SchemaRegistry`，D334–D349）。
//!
//! 按 `(领域, 版本)` 登记 Schema 文档，以 Canonical JSON 的 BLAKE3-256 摘要作为内容指纹，
//! 并持有 [`SchemaMigrationGraph`] 负责把旧版本文档原子地升级到目标版本。

use core::fmt;
use std::collections::BTreeMap;

use serde_json::Value;

use crate::canonical_json::{CanonicalJsonError, to_canonical_vec};
use crate::migration::{MigrationEdge, SchemaMigrationGraph};

/// Schema 内容指纹：Canonical JSON 字节的 BLAKE3-256 摘要
pub type SchemaFingerprint = [u8; 32];

/// 一条已登记的 Schema 修订
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredSchema {
    pub domain: String,
    pub version: u32,
    pub document: Value,
    pub fingerprint: SchemaFingerprint,
}

/// Schema 注册表错误
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaError {
    /// 同一 `(领域, 版本)` 已登记了内容不同的 Schema（修订一经发布不可变）
    ConflictingRevision { domain: String, version: u32 },
    /// 引用了尚未登记的 `(领域, 版本)`
    UnknownRevision { domain: String, version: u32 },
    /// 迁移边的起止版本相同
    SelfLoopMigration { domain: String, version: u32 },
    /// Schema 文档无法规范化
    CanonicalJson(CanonicalJsonError),
    /// 迁移图中不存在可达路径（对应错误码 `0x7004 SchemaMigrationPathNotFound`）
    MigrationPathNotFound(String),
    /// 迁移变换函数执行失败
    MigrationFailed {
        domain: String,
        from_ver: u32,
        to_ver: u32,
        reason: String,
    },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingRevision { domain, version } => {
                write!(
                    f,
                    "schema {domain} v{version} is already registered with different content"
                )
            }
            Self::UnknownRevision { domain, version } => write!(f, "schema {domain} v{version} is not registered"),
            Self::SelfLoopMigration { domain, version } => {
                write!(f, "migration edge for {domain} must not map v{version} onto itself")
            }
            Self::CanonicalJson(err) => write!(f, "schema document is not canonicalizable: {err}"),
            Self::MigrationPathNotFound(msg) => f.write_str(msg),
            Self::MigrationFailed {
                domain,
                from_ver,
                to_ver,
                reason,
            } => {
                write!(f, "migration {domain} v{from_ver} -> v{to_ver} failed: {reason}")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

impl From<CanonicalJsonError> for SchemaError {
    fn from(err: CanonicalJsonError) -> Self {
        Self::CanonicalJson(err)
    }
}

/// 计算 JSON 文档的内容指纹
pub fn fingerprint_of(document: &Value) -> Result<SchemaFingerprint, CanonicalJsonError> {
    let canonical = to_canonical_vec(document)?;
    Ok(*blake3::hash(&canonical).as_bytes())
}

/// Schema 注册表
#[derive(Debug, Default)]
pub struct SchemaRegistry {
    revisions: BTreeMap<String, BTreeMap<u32, RegisteredSchema>>,
    migrations: SchemaMigrationGraph,
}

impl SchemaRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个 Schema 修订。
    ///
    /// 重复登记内容完全相同的修订是幂等的；内容不同则返回 [`SchemaError::ConflictingRevision`]。
    pub fn register(&mut self, domain: &str, version: u32, document: Value) -> Result<&RegisteredSchema, SchemaError> {
        let fingerprint = fingerprint_of(&document)?;
        let versions = self.revisions.entry(domain.to_owned()).or_default();
        if let Some(existing) = versions.get(&version) {
            if existing.fingerprint != fingerprint {
                return Err(SchemaError::ConflictingRevision {
                    domain: domain.to_owned(),
                    version,
                });
            }
        }
        Ok(versions.entry(version).or_insert_with(|| RegisteredSchema {
            domain: domain.to_owned(),
            version,
            document,
            fingerprint,
        }))
    }

    /// 查询指定修订
    pub fn get(&self, domain: &str, version: u32) -> Option<&RegisteredSchema> {
        self.revisions.get(domain)?.get(&version)
    }

    /// 指定领域已登记的最高版本号
    pub fn latest_version(&self, domain: &str) -> Option<u32> {
        self.revisions.get(domain)?.keys().next_back().copied()
    }

    /// 全部已登记领域（按字典序）
    pub fn domains(&self) -> impl Iterator<Item = &str> {
        self.revisions.keys().map(String::as_str)
    }

    /// 只读访问迁移图
    pub fn migrations(&self) -> &SchemaMigrationGraph {
        &self.migrations
    }

    /// 登记一条迁移边；起止两个版本都必须已登记
    pub fn register_migration(&mut self, domain: &str, edge: MigrationEdge) -> Result<(), SchemaError> {
        if edge.from_ver == edge.to_ver {
            return Err(SchemaError::SelfLoopMigration {
                domain: domain.to_owned(),
                version: edge.from_ver,
            });
        }
        for version in [edge.from_ver, edge.to_ver] {
            self.require_revision(domain, version)?;
        }
        self.migrations.add_edge(domain, edge);
        Ok(())
    }

    /// 把 `document` 从 `from_ver` 原子地迁移到 `target_ver`。
    ///
    /// 变换在副本上依次执行，只有全部步骤成功后才写回；任一步失败时原文档保持不变。
    pub fn migrate(
        &self,
        domain: &str,
        document: &mut Value,
        from_ver: u32,
        target_ver: u32,
    ) -> Result<(), SchemaError> {
        self.require_revision(domain, from_ver)?;
        self.require_revision(domain, target_ver)?;
        let path = self
            .migrations
            .find_migration_path(domain, from_ver, target_ver)
            .map_err(SchemaError::MigrationPathNotFound)?;
        if path.is_empty() {
            return Ok(());
        }

        let edges = &self.migrations.edges_by_domain[domain];
        let mut working = document.clone();
        for edge in path.into_iter().map(|idx| &edges[idx]) {
            (edge.transform_fn)(&mut working).map_err(|reason| SchemaError::MigrationFailed {
                domain: domain.to_owned(),
                from_ver: edge.from_ver,
                to_ver: edge.to_ver,
                reason,
            })?;
        }
        *document = working;
        Ok(())
    }

    fn require_revision(&self, domain: &str, version: u32) -> Result<(), SchemaError> {
        match self.get(domain, version) {
            Some(_) => Ok(()),
            None => Err(SchemaError::UnknownRevision {
                domain: domain.to_owned(),
                version,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DOMAIN: &str = "rynvek.world";

    /// v1 → v2：把 `name` 字段重命名为 `title`
    fn rename_name_to_title(doc: &mut Value) -> Result<(), String> {
        let obj = doc.as_object_mut().ok_or("document must be an object")?;
        let name = obj.remove("name").ok_or("missing field `name`")?;
        obj.insert("title".to_owned(), name);
        Ok(())
    }

    /// v2 → v3：补充默认 `tick_hz`
    fn add_default_tick_hz(doc: &mut Value) -> Result<(), String> {
        let obj = doc.as_object_mut().ok_or("document must be an object")?;
        obj.entry("tick_hz").or_insert(json!(30));
        Ok(())
    }

    fn registry_with_three_versions() -> SchemaRegistry {
        let mut registry = SchemaRegistry::new();
        for version in 1..=3 {
            registry
                .register(DOMAIN, version, json!({ "$id": format!("{DOMAIN}.v{version}") }))
                .unwrap();
        }
        let edges = [
            (1, 2, rename_name_to_title as fn(&mut Value) -> Result<(), String>),
            (2, 3, add_default_tick_hz),
        ];
        for (from_ver, to_ver, transform_fn) in edges {
            let edge = MigrationEdge {
                from_ver,
                to_ver,
                is_lossless: true,
                transform_fn,
            };
            registry.register_migration(DOMAIN, edge).unwrap();
        }
        registry
    }

    #[test]
    fn registration_is_idempotent_but_immutable() {
        let mut registry = SchemaRegistry::new();
        let fp = registry
            .register(DOMAIN, 1, json!({"b": 1, "a": 2}))
            .unwrap()
            .fingerprint;
        // 键顺序不同但规范形式相同 → 幂等
        assert_eq!(
            registry
                .register(DOMAIN, 1, json!({"a": 2, "b": 1}))
                .unwrap()
                .fingerprint,
            fp
        );
        assert_eq!(
            registry.register(DOMAIN, 1, json!({"a": 3})),
            Err(SchemaError::ConflictingRevision {
                domain: DOMAIN.to_owned(),
                version: 1
            })
        );
    }

    #[test]
    fn fingerprint_hashes_canonical_bytes() {
        let doc = json!({"z": [1, 2], "a": "x"});
        let expected = *blake3::hash(br#"{"a":"x","z":[1,2]}"#).as_bytes();
        assert_eq!(fingerprint_of(&doc), Ok(expected));
    }

    #[test]
    fn latest_version_and_domains() {
        let registry = registry_with_three_versions();
        assert_eq!(registry.latest_version(DOMAIN), Some(3));
        assert_eq!(registry.latest_version("missing"), None);
        assert_eq!(registry.domains().collect::<Vec<_>>(), vec![DOMAIN]);
    }

    #[test]
    fn migrate_applies_all_steps_in_order() {
        let registry = registry_with_three_versions();
        let mut doc = json!({"name": "Genesis"});
        registry.migrate(DOMAIN, &mut doc, 1, 3).unwrap();
        assert_eq!(doc, json!({"title": "Genesis", "tick_hz": 30}));
    }

    #[test]
    fn failed_migration_leaves_document_untouched() {
        let registry = registry_with_three_versions();
        let mut doc = json!({"untitled": true});
        let err = registry.migrate(DOMAIN, &mut doc, 1, 3).unwrap_err();
        assert!(matches!(
            err,
            SchemaError::MigrationFailed {
                from_ver: 1,
                to_ver: 2,
                ..
            }
        ));
        assert_eq!(doc, json!({"untitled": true}));
    }

    #[test]
    fn invalid_edges_and_paths_are_rejected() {
        let mut registry = registry_with_three_versions();
        let self_loop = MigrationEdge {
            from_ver: 2,
            to_ver: 2,
            is_lossless: true,
            transform_fn: add_default_tick_hz,
        };
        assert!(matches!(
            registry.register_migration(DOMAIN, self_loop),
            Err(SchemaError::SelfLoopMigration { .. })
        ));
        let dangling = MigrationEdge {
            from_ver: 3,
            to_ver: 9,
            is_lossless: true,
            transform_fn: add_default_tick_hz,
        };
        assert!(matches!(
            registry.register_migration(DOMAIN, dangling),
            Err(SchemaError::UnknownRevision { version: 9, .. })
        ));
        let mut doc = json!({});
        assert!(matches!(
            registry.migrate(DOMAIN, &mut doc, 3, 1),
            Err(SchemaError::MigrationPathNotFound(_))
        ));
    }
}
