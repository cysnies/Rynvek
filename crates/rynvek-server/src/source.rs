// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! AGPLv3 §13 源码要约（规范 11.3 路由 `02`，ADR-L014 / ADR-L016）。
//!
//! 通过网络与本服务交互的每个用户都必须能拿到 **当前运行二进制** 的对应源码：
//! 构建脚本嵌入精确 Git Commit 与由 `Cargo.lock` 生成的 SPDX 2.3 SBOM，本模块据此生成要约。
//! 源码 Tarball 指向仓库该 Commit 的归档（`{repository}/archive/{commit}.tar.gz`）；
//! 工作区含未提交修改时 `tree_dirty = true`，运营方必须另行提供修改后的源码（`source_tarball_url` 可配置覆盖）。

use serde::{Deserialize, Serialize};

/// 构建时嵌入的 Commit
pub const SOURCE_COMMIT: &str = env!("RYNVEK_SOURCE_COMMIT");
/// 构建时工作区是否含未提交修改
pub const SOURCE_DIRTY: &str = env!("RYNVEK_SOURCE_DIRTY");
/// 提交时间（UTC，RFC 3339）
pub const SOURCE_COMMIT_TIME: &str = env!("RYNVEK_SOURCE_COMMIT_TIME");
/// 构建时生成的 SPDX 2.3 SBOM（JSON）
pub const SBOM_SPDX_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/sbom.spdx.json"));
/// 仓库地址（工作区 `package.repository`）
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// 版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// 本服务许可证
pub const LICENSE: &str = "AGPL-3.0-only";
/// SBOM 路由（规范外的补充路由，见 ERRATA）
pub const SBOM_PATH: &str = "/api/v1/meta/sbom.spdx.json";

/// 源码要约响应体
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceOffer {
    pub license: String,
    pub agpl_section: String,
    pub name: String,
    pub version: String,
    pub repository: String,
    pub git_commit: String,
    pub commit_time: String,
    pub tree_dirty: bool,
    pub source_tarball_url: String,
    pub sbom_url: String,
    pub sbom_format: String,
}

impl SourceOffer {
    /// 以构建信息生成要约；`public_base_url` 为本服务对外地址（用于拼接 SBOM 绝对链接），
    /// `tarball_override` 允许运营方指向自托管的（修改后的）源码包
    pub fn current(public_base_url: &str, tarball_override: Option<&str>) -> Self {
        let base = public_base_url.trim_end_matches('/');
        Self {
            license: LICENSE.to_owned(),
            agpl_section: "AGPL-3.0 §13 Remote Network Interaction".to_owned(),
            name: "rynvek-server".to_owned(),
            version: VERSION.to_owned(),
            repository: REPOSITORY.to_owned(),
            git_commit: SOURCE_COMMIT.to_owned(),
            commit_time: SOURCE_COMMIT_TIME.to_owned(),
            tree_dirty: SOURCE_DIRTY == "true",
            source_tarball_url: tarball_override
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{}/archive/{SOURCE_COMMIT}.tar.gz", REPOSITORY.trim_end_matches('/'))),
            sbom_url: format!("{base}{SBOM_PATH}"),
            sbom_format: "SPDX-2.3+json".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_commit_is_exact_and_sbom_is_spdx_2_3() {
        assert!(
            SOURCE_COMMIT.len() == 40 && SOURCE_COMMIT.bytes().all(|b| b.is_ascii_hexdigit()),
            "应嵌入完整 40 位 commit：{SOURCE_COMMIT}"
        );
        let sbom: serde_json::Value = serde_json::from_str(SBOM_SPDX_JSON).unwrap();
        assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
        assert_eq!(sbom["dataLicense"], "CC0-1.0");
        let pkgs = sbom["packages"].as_array().unwrap();
        let me = pkgs.iter().find(|p| p["name"] == "rynvek-server").unwrap();
        assert_eq!(me["licenseDeclared"], "AGPL-3.0-only");
        let proto = pkgs.iter().find(|p| p["name"] == "rynvek-protocol").unwrap();
        assert_eq!(proto["licenseDeclared"], "Apache-2.0 WITH LLVM-exception");
        assert!(pkgs.iter().any(|p| p["name"] == "axum"));
        assert!(sbom["documentNamespace"].as_str().unwrap().contains(SOURCE_COMMIT));
    }

    #[test]
    fn offer_points_at_this_commit() {
        let o = SourceOffer::current("https://arena.example/", None);
        assert_eq!(o.git_commit, SOURCE_COMMIT);
        assert_eq!(
            o.source_tarball_url,
            format!("https://github.com/rynvek/rynvek/archive/{SOURCE_COMMIT}.tar.gz")
        );
        assert_eq!(o.sbom_url, "https://arena.example/api/v1/meta/sbom.spdx.json");
        assert_eq!(o.license, "AGPL-3.0-only");
    }
}
