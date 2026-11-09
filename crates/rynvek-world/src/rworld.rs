// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rworld` 世界包：在统一容器之上约定
//! - 恰好一个 `world_config` 条目，路径固定为 `world.json`（结构见 `protocols/schemas/rworld.schema.json`）；
//! - 可选一个 `lockfile` 条目，路径固定为 `dependencies.lock`（规范 10.3.1）；
//! - 任意数量的 `genome` / `data` 附属条目（M8 只做校验与分发，不注入世界）。
//!
//! 世界配置只能引用规范已冻结的 Arena 模板（`arena_duel` / `arena_pve`），
//! 因而由 `.rworld` 创建的世界与 JSON 模板创建的世界走同一条确定性路径。

use rynvek_rts::{ArenaWorld, MatchConfig};
use serde::{Deserialize, Serialize};

use crate::package::{Package, PackageError, PackageKind, read_package};

/// 世界配置条目路径
pub const WORLD_CONFIG_PATH: &str = "world.json";
/// 依赖锁定文件路径
pub const LOCKFILE_PATH: &str = "dependencies.lock";
/// `world.json` 的 `schema` 字段
pub const WORLD_CONFIG_SCHEMA: &str = "rynvek.world-config.v1";
/// 上传 `.rworld` 的 Content-Type
pub const RWORLD_MEDIA_TYPE: &str = "application/vnd.rynvek.rworld";

/// `world.json`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldConfigFile {
    pub schema: String,
    /// `arena_duel` 或 `arena_pve`
    pub template: String,
    pub map_seed: u64,
    /// PvE 玩家数（1–2；双人局固定为 2，可省略）
    #[serde(default)]
    pub players: Option<u8>,
}

impl WorldConfigFile {
    /// 转换为对局配置
    pub fn match_config(&self) -> Result<MatchConfig, String> {
        if self.schema != WORLD_CONFIG_SCHEMA {
            return Err(format!("world.json schema 必须为 {WORLD_CONFIG_SCHEMA}"));
        }
        match (self.template.as_str(), self.players) {
            ("arena_duel", None | Some(2)) => Ok(MatchConfig::duel(self.map_seed)),
            ("arena_duel", Some(p)) => Err(format!("arena_duel 固定 2 名玩家，不能为 {p}")),
            ("arena_pve", p) => {
                let p = p.unwrap_or(1);
                if (1..=2).contains(&p) {
                    Ok(MatchConfig::pve(self.map_seed, p))
                } else {
                    Err(format!("arena_pve 玩家数必须为 1–2，不能为 {p}"))
                }
            }
            (other, _) => Err(format!("未知模板 `{other}`")),
        }
    }

    /// 对应的 `worlds.game_mode`
    pub fn game_mode(&self) -> &'static str {
        if self.template == "arena_pve" {
            "PvE_Survival"
        } else {
            "PvP_Custom"
        }
    }
}

/// 已校验的世界包
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldBundle {
    pub package: Package,
    pub config: WorldConfigFile,
    pub match_config: MatchConfig,
    /// `dependencies.lock` 原文（JSON）
    pub lockfile: Option<serde_json::Value>,
}

fn invalid(detail: impl Into<String>) -> PackageError {
    PackageError {
        code: rynvek_core::error::RynvekErrorCode::InvalidFileMagicOrVersion,
        detail: detail.into(),
    }
}

impl WorldBundle {
    /// 解析并校验 `.rworld`
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PackageError> {
        let package = read_package(bytes, Some(PackageKind::Rworld))?;
        Self::from_package(package)
    }

    pub fn from_package(package: Package) -> Result<Self, PackageError> {
        let configs: Vec<_> = package.entries_of("world_config").collect();
        let [(entry, data)] = configs.as_slice() else {
            return Err(invalid("`.rworld` 必须恰好包含一个 world_config 条目"));
        };
        if entry.path != WORLD_CONFIG_PATH {
            return Err(invalid(format!("world_config 条目路径必须为 {WORLD_CONFIG_PATH}")));
        }
        let config: WorldConfigFile =
            serde_json::from_slice(data).map_err(|e| invalid(format!("world.json 非法：{e}")))?;
        let match_config = config.match_config().map_err(invalid)?;
        let locks: Vec<_> = package.entries_of("lockfile").collect();
        let lockfile = match locks.as_slice() {
            [] => None,
            [(e, d)] if e.path == LOCKFILE_PATH => {
                Some(serde_json::from_slice(d).map_err(|e| invalid(format!("dependencies.lock 非法：{e}")))?)
            }
            _ => return Err(invalid(format!("lockfile 条目最多一个且路径必须为 {LOCKFILE_PATH}"))),
        };
        Ok(Self {
            package,
            config,
            match_config,
            lockfile,
        })
    }

    /// 创建权威世界（第 0 Tick）
    pub fn instantiate(&self) -> ArenaWorld {
        ArenaWorld::new(self.match_config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PackageInput, PackageInputEntry, write_package};

    pub(crate) fn sample(config: &str, extra: Vec<PackageInputEntry>) -> Vec<u8> {
        let mut entries = vec![PackageInputEntry {
            path: WORLD_CONFIG_PATH.into(),
            kind: "world_config".into(),
            media_type: "application/json".into(),
            data: config.as_bytes().to_vec(),
        }];
        entries.extend(extra);
        write_package(&PackageInput {
            kind: PackageKind::Rworld,
            id: "community.worlds.duel-42".into(),
            version: "1.0.0".into(),
            name: "Duel 42".into(),
            license: "CC0-1.0".into(),
            engine_compat: ">=1.0.0 <2.0.0".into(),
            entries,
        })
        .unwrap()
    }

    #[test]
    fn loads_world_and_matches_template_state_root() {
        let bytes = sample(
            r#"{"schema":"rynvek.world-config.v1","template":"arena_duel","map_seed":42}"#,
            vec![],
        );
        let b = WorldBundle::from_bytes(&bytes).unwrap();
        assert_eq!(b.config.game_mode(), "PvP_Custom");
        assert_eq!(
            b.instantiate().state_hash(),
            ArenaWorld::new(MatchConfig::duel(42)).state_hash()
        );
    }

    #[test]
    fn rejects_bad_configs() {
        for cfg in [
            r#"{"schema":"rynvek.world-config.v1","template":"arena_duel","map_seed":1,"players":1}"#,
            r#"{"schema":"rynvek.world-config.v1","template":"nope","map_seed":1}"#,
            r#"{"schema":"other","template":"arena_pve","map_seed":1}"#,
            r#"{"schema":"rynvek.world-config.v1","template":"arena_pve","map_seed":1,"extra":true}"#,
            "not json",
        ] {
            let e = WorldBundle::from_bytes(&sample(cfg, vec![])).unwrap_err();
            assert_eq!(
                e.code,
                rynvek_core::error::RynvekErrorCode::InvalidFileMagicOrVersion,
                "{cfg}"
            );
        }
    }

    #[test]
    fn lockfile_is_parsed() {
        let lock = PackageInputEntry {
            path: LOCKFILE_PATH.into(),
            kind: "lockfile".into(),
            media_type: "application/json".into(),
            data: br#"{"lockfile_version":1,"packages":[]}"#.to_vec(),
        };
        let b = WorldBundle::from_bytes(&sample(
            r#"{"schema":"rynvek.world-config.v1","template":"arena_pve","map_seed":3,"players":2}"#,
            vec![lock],
        ))
        .unwrap();
        assert_eq!(b.lockfile.unwrap()["lockfile_version"], 1);
        assert_eq!(b.match_config.players, 2);
    }
}
