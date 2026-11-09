// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek 工程自动化任务入口（通过 `cargo xtask <子命令>` 调用，别名见 `.cargo/config.toml`）。

#![deny(unsafe_code)]

mod golden_vectors;
mod plugin_fixtures;

use std::path::PathBuf;
use std::process::ExitCode;

/// 可用子命令及其说明
const COMMANDS: &[(&str, &str)] = &[
    (
        "verify-golden-vectors",
        "校验 tests/golden_vectors/v1_baseline.json 中的全部跨语言黄金向量",
    ),
    (
        "build-plugin-fixtures",
        "重建示例插件 Component 夹具（--check：只比较不写入）",
    ),
];

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("verify-golden-vectors") => run_verify_golden_vectors(),
        Some("build-plugin-fixtures") => {
            let check = std::env::args().skip(2).any(|a| a == "--check");
            report(plugin_fixtures::run(&workspace_root(), check))
        }
        Some("help" | "--help" | "-h") | None => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(unknown) => {
            eprintln!("未知子命令：{unknown}\n");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

/// 把子命令结果转为退出码
fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误：{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!("用法：cargo xtask <子命令>\n\n子命令：");
    for (name, about) in COMMANDS {
        println!("  {name:<24} {about}");
    }
}

fn run_verify_golden_vectors() -> ExitCode {
    let path = workspace_root().join(golden_vectors::BASELINE_RELATIVE_PATH);
    match golden_vectors::verify_file(&path) {
        Ok(report) => {
            report.print();
            if report.all_passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("无法加载黄金向量文件 {}：{err}", path.display());
            ExitCode::FAILURE
        }
    }
}

/// 工作区根目录（xtask 位于 `crates/xtask`）
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
