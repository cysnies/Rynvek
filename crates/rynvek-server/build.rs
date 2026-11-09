// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 构建脚本：为 AGPLv3 §13 源码要约（`/api/v1/meta/source-offer`）嵌入
//!
//! 1. `RYNVEK_SOURCE_COMMIT`：当前构建对应的精确 Git Commit（可用同名环境变量覆盖，供无 `.git` 的源码包构建）；
//! 2. `RYNVEK_SOURCE_DIRTY`：工作区是否含未提交修改（`true` 时要约会如实声明）；
//! 3. `RYNVEK_SOURCE_COMMIT_TIME`：提交时间（RFC 3339），同时作为 SBOM `created` 时间以保证可复现；
//! 4. `$OUT_DIR/sbom.spdx.json`：由工作区 `Cargo.lock` 生成的 SPDX 2.3 SBOM（ADR-L016）。
//!
//! 只用标准库，避免为构建脚本引入依赖。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// SPDX ID 只允许 `[A-Za-z0-9.-]`
fn spdx_id(name: &str, version: &str) -> String {
    let raw = format!("SPDXRef-Package-{name}-{version}");
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[derive(Default)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn parse_lock(text: &str) -> Vec<LockPackage> {
    let mut out = Vec::new();
    let mut cur: Option<LockPackage> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            out.extend(cur.take());
            cur = Some(LockPackage::default());
            continue;
        }
        let Some(p) = cur.as_mut() else { continue };
        let Some((k, v)) = line.split_once(" = ") else { continue };
        let v = v.trim_matches('"').to_owned();
        match k {
            "name" => p.name = v,
            "version" => p.version = v,
            "source" => p.source = Some(v),
            "checksum" => p.checksum = Some(v),
            _ => {}
        }
    }
    out.extend(cur);
    out
}

/// 工作区自有 crate 的声明许可证（与各 crate 的 SPDX 头一致）
fn local_license(name: &str) -> &'static str {
    if name == "rynvek-protocol" {
        "Apache-2.0 WITH LLVM-exception"
    } else {
        "AGPL-3.0-only"
    }
}

fn sbom(packages: &[LockPackage], commit: &str, created: &str, repo: &str, version: &str) -> String {
    let root_id = spdx_id("rynvek-server", version);
    let mut pkgs = Vec::new();
    let mut rels = vec![format!(
        "{{\"spdxElementId\":\"SPDXRef-DOCUMENT\",\"relationshipType\":\"DESCRIBES\",\"relatedSpdxElement\":{}}}",
        json_str(&root_id)
    )];
    for p in packages {
        let id = spdx_id(&p.name, &p.version);
        let local = p.source.is_none();
        let download = match &p.source {
            Some(s) if s.starts_with("registry+") => {
                format!("https://crates.io/api/v1/crates/{}/{}/download", p.name, p.version)
            }
            Some(s) if s.starts_with("git+") => s.trim_start_matches("git+").to_owned(),
            Some(_) => "NOASSERTION".to_owned(),
            None => format!("git+{repo}@{commit}"),
        };
        let license = if local { local_license(&p.name) } else { "NOASSERTION" };
        let checksum = p
            .checksum
            .as_ref()
            .map(|c| {
                format!(
                    ",\"checksums\":[{{\"algorithm\":\"SHA256\",\"checksumValue\":{}}}]",
                    json_str(c)
                )
            })
            .unwrap_or_default();
        pkgs.push(format!(
            "{{\"SPDXID\":{},\"name\":{},\"versionInfo\":{},\"downloadLocation\":{},\"filesAnalyzed\":false,\
             \"licenseConcluded\":\"NOASSERTION\",\"licenseDeclared\":{},\"copyrightText\":\"NOASSERTION\",\
             \"supplier\":{}{checksum},\"externalRefs\":[{{\"referenceCategory\":\"PACKAGE-MANAGER\",\
             \"referenceType\":\"purl\",\"referenceLocator\":{}}}]}}",
            json_str(&id),
            json_str(&p.name),
            json_str(&p.version),
            json_str(&download),
            json_str(license),
            json_str(if local {
                "Organization: The Rynvek Contributors"
            } else {
                "NOASSERTION"
            }),
            json_str(&format!("pkg:cargo/{}@{}", p.name, p.version)),
        ));
        if id != root_id {
            rels.push(format!(
                "{{\"spdxElementId\":{},\"relationshipType\":\"CONTAINS\",\"relatedSpdxElement\":{}}}",
                json_str(&root_id),
                json_str(&id)
            ));
        }
    }
    format!(
        "{{\"spdxVersion\":\"SPDX-2.3\",\"dataLicense\":\"CC0-1.0\",\"SPDXID\":\"SPDXRef-DOCUMENT\",\
         \"name\":{},\"documentNamespace\":{},\"creationInfo\":{{\"created\":{},\
         \"creators\":[\"Organization: The Rynvek Contributors\",\"Tool: rynvek-server-build.rs\"]}},\
         \"documentDescribes\":[{}],\"packages\":[{}],\"relationships\":[{}]}}",
        json_str(&format!("rynvek-server-{version}")),
        json_str(&format!("{repo}/spdx/rynvek-server-{version}-{commit}")),
        json_str(created),
        json_str(&root_id),
        pkgs.join(","),
        rels.join(",")
    )
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo 提供"));
    let root = manifest.join("../..");
    println!("cargo:rerun-if-env-changed=RYNVEK_SOURCE_COMMIT");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    // HEAD 或分支引用变化时重新嵌入 commit
    let git_dir = git(&root, &["rev-parse", "--absolute-git-dir"]).map(PathBuf::from);
    if let Some(dir) = &git_dir {
        println!("cargo:rerun-if-changed={}", dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", dir.join("index").display());
        if let Some(r) = git(&root, &["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", dir.join(r).display());
        }
        println!("cargo:rerun-if-changed={}", dir.join("packed-refs").display());
    }
    let commit = env::var("RYNVEK_SOURCE_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git(&root, &["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let dirty = git(&root, &["status", "--porcelain", "--untracked-files=no"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    // SPDX 要求 UTC `YYYY-MM-DDThh:mm:ssZ`：以 TZ=UTC 格式化提交时间（可复现，不用构建时刻）
    let created = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "-1", "--date=format-local:%Y-%m-%dT%H:%M:%SZ", "--format=%cd"])
        .env("TZ", "UTC")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned());
    println!("cargo:rustc-env=RYNVEK_SOURCE_COMMIT={commit}");
    println!("cargo:rustc-env=RYNVEK_SOURCE_DIRTY={dirty}");
    println!("cargo:rustc-env=RYNVEK_SOURCE_COMMIT_TIME={created}");

    let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap_or_default();
    let repo = env::var("CARGO_PKG_REPOSITORY").unwrap_or_default();
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let doc = sbom(&parse_lock(&lock), &commit, &created, &repo, &version);
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo 提供")).join("sbom.spdx.json");
    fs::write(out, doc).expect("写入 SBOM");
}
