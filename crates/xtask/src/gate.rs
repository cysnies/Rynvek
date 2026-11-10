// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 仓库级合规门禁（`cargo xtask license-gate`，ci-full-gate 的一部分）：
//!
//! 1. **禁用旧称**：受版本控制的文本文件中不得出现品牌迁移前的旧名（ADR-BRAND-001）；
//!    历史文档（`docs/`）记录迁移过程，豁免；
//! 2. **SPDX 文件头**：源代码文件必须在前 8 行声明 `SPDX-License-Identifier`，
//!    无法内嵌注释的文件必须被 `REUSE.toml` 的批量声明覆盖；
//! 3. **双层许可证矩阵**（GAP-15）：公开协议、客户端 SDK、Python `rynvek` 纯 Python 层与示例插件为
//!    `Apache-2.0 WITH LLVM-exception`，引擎 / 服务端 / 应用（含 Python 原生扩展）为 `AGPL-3.0-only`；
//! 4. **第三方依赖许可证与禁用项**：`cargo deny --offline check licenses bans`（白名单见 `deny.toml`）；
//! 5. **SBOM**：`rynvek sbom` 输出合法的 SPDX 2.3 文档。

use std::path::Path;
use std::process::Command;

use crate::util;

const APACHE: &str = "Apache-2.0 WITH LLVM-exception";
const AGPL: &str = "AGPL-3.0-only";

/// 旧称（拆开书写，避免本文件自身命中）
fn forbidden_words() -> [String; 4] {
    [
        ["neuro", "verse"].concat(),
        [".n", "view"].concat(),
        ["rynvek", "-runtime"].concat(),
        ["rynvek", "_runtime"].concat(),
    ]
}

/// 单独成词的旧缩写（两字母品牌缩写本身，及其 `_` / `-` / `@…/` 前后缀形式，大小写不敏感）
fn has_short_legacy_token(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t.eq_ignore_ascii_case(&["n", "v"].concat()))
}

/// 豁免文件：断言旧称“不存在”的守卫测试
const NAME_EXEMPT: &[&str] = &["packages/ui/test/theme.test.ts"];

/// 视为文本、参与旧称扫描的扩展名
const TEXT_EXT: &[&str] = &[
    "rs", "ts", "tsx", "js", "mjs", "cjs", "py", "pyi", "wit", "toml", "yaml", "yml", "json", "md", "sh", "css",
    "wgsl", "html", "proto", "txt",
];

/// 必须带 SPDX 文件头（或被 REUSE.toml 覆盖）的源代码扩展名
const CODE_EXT: &[&str] = &[
    "rs", "ts", "tsx", "js", "mjs", "cjs", "py", "pyi", "wit", "toml", "yaml", "yml", "sh", "css", "wgsl", "html",
    "proto",
];

/// Apache 区域（GAP-15 / 项目文档 7.1）；`python/native/` 例外（AGPL 原生扩展）
fn required_license(path: &str) -> Option<&'static str> {
    const APACHE_PREFIXES: &[&str] = &[
        "protocols/",
        "crates/rynvek-protocol/",
        "packages/core/",
        "packages/protocol/",
        "packages/rules/",
        "packages/agent-sdk/",
        "examples/plugins/",
        "python/rynvek/",
        "python/tests/",
        "python/pyproject.toml",
    ];
    if path.starts_with("python/native/") {
        return Some(AGPL);
    }
    if APACHE_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return Some(APACHE);
    }
    if path.starts_with("crates/") || path.starts_with("packages/") || path.starts_with("apps/") {
        return Some(AGPL);
    }
    None
}

fn extension(path: &str) -> &str {
    path.rsplit_once('.').map_or("", |(_, e)| e)
}

/// 简单 glob：`**` 跨目录，`*` 不跨 `/`
fn glob_match(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', b'/', rest @ ..] => (0..=s.len()).any(|i| (i == 0 || s[i - 1] == b'/') && go(rest, &s[i..])),
            [b'*', b'*', rest @ ..] => (0..=s.len()).any(|i| go(rest, &s[i..])),
            [b'*', rest @ ..] => (0..=s.len())
                .take_while(|&i| i == 0 || s[i - 1] != b'/')
                .any(|i| go(rest, &s[i..])),
            [c, rest @ ..] => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

/// 读取 `REUSE.toml` 中全部 `path` 模式（只解析本仓库使用的子集：字符串或字符串数组）
fn reuse_patterns(root: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(root.join("REUSE.toml")).map_err(|e| format!("读取 REUSE.toml：{e}"))?;
    let mut out = Vec::new();
    let mut in_array = false;
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("path") {
            let rest = rest.trim_start().trim_start_matches('=').trim();
            in_array = rest.starts_with('[') && !rest.contains(']');
            collect_quoted(rest, &mut out);
        } else if in_array {
            if l.starts_with(']') {
                in_array = false;
            } else {
                collect_quoted(l, &mut out);
            }
        }
    }
    Ok(out)
}

fn collect_quoted(s: &str, out: &mut Vec<String>) {
    let mut parts = s.split('"');
    parts.next();
    while let (Some(inner), Some(_)) = (parts.next(), parts.next()) {
        out.push(inner.to_owned());
    }
}

fn header_license(text: &str) -> Option<String> {
    text.lines().take(8).find_map(|l| {
        let (_, rest) = l.split_once("SPDX-License-Identifier:")?;
        Some(
            rest.trim()
                .trim_end_matches("*/")
                .trim_end_matches("-->")
                .trim()
                .to_owned(),
        )
    })
}

fn tracked_files(root: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| format!("git ls-files：{e}"))?;
    if !out.status.success() {
        return Err("git ls-files 失败".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

/// 文件扫描（旧称 + SPDX）；返回违规列表
pub fn scan(root: &Path) -> Result<(usize, Vec<String>), String> {
    let files = tracked_files(root)?;
    let reuse = reuse_patterns(root)?;
    let words = forbidden_words();
    let mut problems = Vec::new();
    for f in &files {
        let ext = extension(f);
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue; // 二进制夹具
        };
        if TEXT_EXT.contains(&ext) && !f.starts_with("docs/") && !NAME_EXEMPT.contains(&f.as_str()) {
            for (n, line) in text.lines().enumerate() {
                let lower = line.to_ascii_lowercase();
                if words.iter().any(|w| lower.contains(w.as_str())) || has_short_legacy_token(line) {
                    problems.push(format!("{f}:{}：出现禁用旧称", n + 1));
                }
            }
        }
        if CODE_EXT.contains(&ext) {
            match header_license(&text) {
                Some(lic) => {
                    if let Some(want) = required_license(f).filter(|w| *w != lic) {
                        problems.push(format!("{f}：许可证 {lic}，该区域要求 {want}"));
                    }
                }
                None if reuse.iter().any(|p| glob_match(p, f)) => {}
                None => problems.push(format!(
                    "{f}：缺少 SPDX-License-Identifier 文件头且未被 REUSE.toml 覆盖"
                )),
            }
        }
    }
    Ok((files.len(), problems))
}

/// 完整门禁
pub fn run(root: &Path) -> Result<(), String> {
    let (n, problems) = scan(root)?;
    if !problems.is_empty() {
        for p in &problems {
            eprintln!("  ✗ {p}");
        }
        return Err(format!("命名 / SPDX 门禁发现 {} 处违规", problems.len()));
    }
    eprintln!("✔ 命名与 SPDX 门禁：{n} 个受版本控制文件");
    if util::which("cargo-deny").is_none() {
        return Err("未安装 cargo-deny（`cargo install cargo-deny --locked`）；第三方许可证门禁无法执行".into());
    }
    let mut c = util::cargo();
    c.current_dir(root)
        .args(["deny", "--offline", "check", "licenses", "bans"]);
    util::run(c, "cargo deny --offline check licenses bans")?;
    let out = util::cargo()
        .current_dir(root)
        .args(["run", "--quiet", "-p", "rynvek-cli", "--", "sbom"])
        .output()
        .map_err(|e| format!("rynvek sbom：{e}"))?;
    if !out.status.success() {
        return Err(format!("rynvek sbom 失败：{}", String::from_utf8_lossy(&out.stderr)));
    }
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("SBOM 不是合法 JSON：{e}"))?;
    let packages = doc["packages"].as_array().map_or(0, Vec::len);
    if doc["spdxVersion"] != "SPDX-2.3" || packages == 0 {
        return Err("SBOM 不是非空的 SPDX 2.3 文档".into());
    }
    eprintln!("✔ SBOM：SPDX-2.3，{packages} 个软件包");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("tests/**", "tests/packages/sample.rworld"));
        assert!(glob_match("crates/**/Cargo.toml", "crates/rynvek-core/Cargo.toml"));
        assert!(glob_match("packages/**/*.json", "packages/ui/package.json"));
        assert!(!glob_match("packages/*.json", "packages/ui/package.json"));
        assert!(glob_match("Cargo.lock", "Cargo.lock"));
        assert!(!glob_match("Cargo.lock", "python/Cargo.lock"));
    }

    #[test]
    fn legacy_tokens() {
        assert!(has_short_legacy_token(&format!("--{}-color", ["n", "v"].concat())));
        assert!(has_short_legacy_token(&format!("{}_TICK", ["N", "V"].concat())));
        assert!(!has_short_legacy_token("environment nvidia envy"));
    }

    #[test]
    fn license_regions() {
        assert_eq!(required_license("python/native/src/lib.rs"), Some(AGPL));
        assert_eq!(required_license("python/rynvek/env.py"), Some(APACHE));
        assert_eq!(required_license("packages/lab/src/index.ts"), Some(AGPL));
        assert_eq!(required_license("packages/rules/src/index.ts"), Some(APACHE));
        assert_eq!(required_license("README.md"), None);
    }
}
