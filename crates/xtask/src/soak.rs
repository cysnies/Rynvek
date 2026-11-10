// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `cargo xtask soak --duration <时长> [--report <路径>]`：以 `opt-dev` 配置构建并运行浸泡执行体
//! `rynvek-soak`（见 `src/bin/rynvek-soak.rs`）。
//!
//! 时长写法：`90s`、`15m`、`24h` 或纯秒数。完整 24 小时：`cargo xtask soak --duration 24h`。

use std::path::Path;
use std::time::Duration;

use crate::util;

/// ci-full-gate 使用的默认短浸泡时长（可用 `RYNVEK_SOAK_DURATION` 覆盖）
pub const CI_DEFAULT: &str = "120s";

/// 解析时长
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, mul) = match s.char_indices().last() {
        Some((i, 's')) => (&s[..i], 1),
        Some((i, 'm')) => (&s[..i], 60),
        Some((i, 'h')) => (&s[..i], 3_600),
        _ => (s, 1),
    };
    let n: u64 = num
        .parse()
        .map_err(|_| format!("非法时长 {s:?}（示例：90s、15m、24h）"))?;
    if n == 0 {
        return Err("时长必须大于 0".into());
    }
    Ok(Duration::from_secs(n * mul))
}

/// 构建并运行
pub fn run(root: &Path, duration: Duration, report: Option<&str>) -> Result<(), String> {
    let mut build = util::cargo();
    build.current_dir(root).args([
        "build",
        "--quiet",
        "--profile",
        "opt-dev",
        "-p",
        "xtask",
        "--bin",
        "rynvek-soak",
    ]);
    util::run(build, "构建 rynvek-soak（opt-dev）")?;
    let report = report.map_or_else(|| root.join("target/soak/soak-report.json"), |r| root.join(r));
    let mut c = std::process::Command::new(root.join("target/opt-dev/rynvek-soak"));
    c.arg("--duration-secs")
        .arg(duration.as_secs().to_string())
        .arg("--report")
        .arg(&report)
        .arg("--root")
        .arg(root);
    util::run(
        c,
        &format!("浸泡测试 {} 秒（报告 {}）", duration.as_secs(), report.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("15m").unwrap(), Duration::from_secs(900));
        assert_eq!(parse_duration("24h").unwrap(), Duration::from_secs(86_400));
        assert_eq!(parse_duration("42").unwrap(), Duration::from_secs(42));
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("abc").is_err());
    }
}
