// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 子进程辅助：统一的命令回显与失败信息。

use std::path::Path;
use std::process::Command;

/// `cargo` 可执行文件（在 `cargo xtask` 下由 Cargo 注入 `CARGO`）
pub fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

/// 运行命令并要求成功退出
pub fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("▶ {what}");
    let status = cmd.status().map_err(|e| format!("{what}：无法启动：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}：失败（{status}）"))
    }
}

/// 在指定目录运行命令
pub fn run_in(dir: &Path, program: &str, args: &[&str], what: &str) -> Result<(), String> {
    let mut c = Command::new(program);
    c.current_dir(dir).args(args);
    run(c, what)
}

/// 在 `PATH` 中查找可执行文件
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}
