// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek 工程自动化任务入口（通过 `cargo xtask <子命令>` 调用，别名见 `.cargo/config.toml`）。

#![deny(unsafe_code)]

mod ci;
mod gate;
mod golden_vectors;
mod plugin_fixtures;
mod python;
mod soak;
mod util;

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
    (
        "license-gate",
        "禁用旧称、SPDX 文件头、双层许可证矩阵、第三方许可证与 SBOM 门禁",
    ),
    (
        "python-env",
        "按 python/uv.lock 在 python/.venv 中可复现安装依赖并 maturin develop",
    ),
    ("pytest [参数…]", "在 python/.venv 中运行 pytest（缺省 python/tests）"),
    (
        "install-pytest-shim [目录]",
        "写入转发到本仓库的 pytest 入口（缺省 ~/.local/bin）",
    ),
    ("soak --duration <时长>", "浸泡测试（90s / 15m / 24h；--report <路径>）"),
    (
        "ci-full-gate",
        "完整门禁：fmt、clippy、测试、黄金向量、夹具、许可证、TS、Python、短浸泡",
    ),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("verify-golden-vectors") => run_verify_golden_vectors(),
        Some("build-plugin-fixtures") => {
            let check = rest.iter().any(|a| a == "--check");
            report(plugin_fixtures::run(&root, check))
        }
        Some("license-gate") => report(gate::run(&root)),
        Some("python-env") => report(python::ensure_env(&root).map(|_| ())),
        Some("pytest") => report(python::pytest(&root, rest)),
        Some("install-pytest-shim") => report(python::install_shim(rest.first().map(String::as_str))),
        Some("soak") => report(run_soak(&root, rest)),
        Some("ci-full-gate") => report(ci::run(&root, rest)),
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

fn run_soak(root: &std::path::Path, args: &[String]) -> Result<(), String> {
    let mut duration = None;
    let mut report_path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--duration" => duration = Some(soak::parse_duration(it.next().ok_or("--duration 需要参数")?)?),
            "--report" => report_path = Some(it.next().ok_or("--report 需要参数")?.clone()),
            other => return Err(format!("未知参数 {other}")),
        }
    }
    let duration = duration.ok_or("缺少 --duration（示例：--duration 24h）")?;
    soak::run(root, duration, report_path.as_deref())
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
        println!("  {name:<28} {about}");
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}
