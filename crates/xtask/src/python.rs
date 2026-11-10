// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Python 科研环境的可复现构建：
//!
//! - `cargo xtask python-env`：用 uv 按 `python/uv.lock` 在 `python/.venv` 中安装精确版本依赖
//!   （`--frozen`，不改锁文件、不触碰全局环境），再以 `opt-dev` 配置 `maturin develop` 原生扩展；
//! - `cargo xtask pytest [参数…]`：先执行上述步骤（依赖未变时跳过 uv 同步），再用 venv 内的 pytest 运行；
//! - `cargo xtask install-pytest-shim [目录]`：在 `PATH` 目录（缺省 `~/.local/bin`）写入一个只转发到本仓库的 `pytest` 入口，
//!   使验收命令 `pytest python/tests` 在不全局安装任何 Python 包的前提下可直接执行。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

/// 锁定的 CPython 版本（规范 1.1：Python >= 3.12；工具链冻结 3.13.5）
pub const PYTHON_VERSION: &str = "3.13";

fn python_dir(root: &Path) -> PathBuf {
    root.join("python")
}

fn venv_dir(root: &Path) -> PathBuf {
    python_dir(root).join(".venv")
}

fn uv() -> Result<PathBuf, String> {
    util::which("uv")
        .or_else(|| Some(PathBuf::from("/usr/local/bin/uv")).filter(|p| p.is_file()))
        .ok_or_else(|| "未找到 uv（https://docs.astral.sh/uv/）；Python 环境需要它按 uv.lock 可复现安装".into())
}

/// 同步依赖并安装原生扩展
pub fn ensure_env(root: &Path) -> Result<PathBuf, String> {
    let py = python_dir(root);
    let venv = venv_dir(root);
    let lock = std::fs::read(py.join("uv.lock")).map_err(|e| format!("读取 python/uv.lock：{e}"))?;
    let stamp_path = venv.join(".rynvek-lock-blake3");
    let stamp = blake3::hash(&lock).to_hex().to_string();
    let synced = std::fs::read_to_string(&stamp_path).is_ok_and(|s| s.trim() == stamp);
    if !synced {
        let mut c = Command::new(uv()?);
        c.current_dir(&py)
            .args(["sync", "--frozen", "--python", PYTHON_VERSION])
            .env("UV_PROJECT_ENVIRONMENT", &venv);
        util::run(c, "uv sync --frozen（python/uv.lock → python/.venv）")?;
        std::fs::write(&stamp_path, &stamp).map_err(|e| e.to_string())?;
    }
    let mut c = Command::new(venv.join("bin/maturin"));
    c.current_dir(&py)
        .args(["develop", "--uv", "--profile", "opt-dev", "--quiet"])
        .env("VIRTUAL_ENV", &venv)
        .env("PATH", prepend_path(&venv.join("bin")));
    util::run(c, "maturin develop --profile opt-dev（rynvek._native）")?;
    Ok(venv)
}

fn prepend_path(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(p) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(paths).unwrap_or_default()
}

/// 运行 pytest；`args` 为空时运行 `python/tests`
pub fn pytest(root: &Path, args: &[String]) -> Result<(), String> {
    let venv = ensure_env(root)?;
    let mut c = Command::new(venv.join("bin/python"));
    c.args(["-m", "pytest"]);
    if args.is_empty() {
        c.arg(python_dir(root).join("tests"));
    } else {
        c.args(args);
    }
    c.env("VIRTUAL_ENV", &venv).env("PATH", prepend_path(&venv.join("bin")));
    util::run(c, "pytest")
}

/// `pytest` 转发入口（POSIX sh）
const SHIM: &str = r#"#!/bin/sh
# Rynvek 仓库的 pytest 入口（由 `cargo xtask install-pytest-shim` 生成，可安全删除）。
# 在 Rynvek 工作树内：转发到 `cargo xtask pytest`（python/.venv 按 uv.lock 可复现构建）；
# 在其他目录：若存在已激活的虚拟环境则使用其中的 pytest，否则报错，不安装任何全局包。
root=$(git rev-parse --show-toplevel 2>/dev/null)
if [ -n "$root" ] && [ -f "$root/python/pyproject.toml" ] && [ -f "$root/crates/xtask/Cargo.toml" ]; then
    exec cargo xtask pytest "$@"
fi
if [ -n "$VIRTUAL_ENV" ] && [ -x "$VIRTUAL_ENV/bin/pytest" ]; then
    exec "$VIRTUAL_ENV/bin/pytest" "$@"
fi
echo "pytest：当前目录不在 Rynvek 工作树内，且没有激活的虚拟环境" >&2
exit 127
"#;

/// 写入 `pytest` 转发入口
pub fn install_shim(dir: Option<&str>) -> Result<(), String> {
    // 缺省写入用户级 `~/.local/bin`（XDG 约定的用户可执行目录，无需 root、不影响系统 Python）
    let dir = match dir {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var("HOME").map_err(|_| "HOME 未设置")?).join(".local/bin"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {}：{e}", dir.display()))?;
    let path = dir.join("pytest");
    if path.exists() {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if !existing.contains("cargo xtask install-pytest-shim") {
            return Err(format!("{} 已存在且不是 Rynvek 入口，拒绝覆盖", path.display()));
        }
    }
    std::fs::write(&path, SHIM).map_err(|e| format!("写入 {}：{e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    eprintln!("已写入 {}", path.display());
    Ok(())
}
