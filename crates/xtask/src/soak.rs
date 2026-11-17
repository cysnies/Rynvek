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
    let report = report.map_or_else(|| root.join("target/soak/soak-report.json"), |r| root.join(r));
    let c = soak_command(root, duration, &report);
    util::run(
        c,
        &format!("浸泡测试 {} 秒（报告 {}）", duration.as_secs(), report.display()),
    )
}

fn soak_command(root: &Path, duration: Duration, report: &Path) -> std::process::Command {
    // 交给Cargo构建并解析执行体：同时尊重共享CARGO_TARGET_DIR、配置文件与平台后缀。
    // 不猜测root/target，也不复制二进制伪装通过；仍执行同一opt-dev浸泡程序。
    let mut c = util::cargo();
    c.current_dir(root)
        .args([
            "run",
            "--quiet",
            "--profile",
            "opt-dev",
            "-p",
            "xtask",
            "--bin",
            "rynvek-soak",
            "--",
        ])
        .arg("--duration-secs")
        .arg(duration.as_secs().to_string())
        .arg("--report")
        .arg(report)
        .arg("--root")
        .arg(root);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_honors_cargo_target_resolution() {
        let root = std::env::temp_dir().join("rynvek-soak-runner-contract");
        let report = root.join("target/soak/report.json");
        let cmd = soak_command(&root, Duration::from_secs(90), &report);
        assert_eq!(
            cmd.get_program(),
            util::cargo().get_program(),
            "执行体必须由Cargo解析，不能猜测root/target忽略共享CARGO_TARGET_DIR"
        );
        assert_eq!(cmd.get_current_dir(), Some(root.as_path()));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], "run", "必须真实运行浸泡程序而不是仅构建");
        assert!(args.windows(2).any(|w| w == ["--profile", "opt-dev"]));
        assert!(args.windows(2).any(|w| w == ["--bin", "rynvek-soak"]));
        assert!(args.windows(2).any(|w| w == ["--duration-secs", "90"]));
        assert!(args.iter().any(|a| *a == report.as_os_str()));
    }

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
