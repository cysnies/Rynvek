// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `cargo xtask build-plugin-fixtures [--check]`：重建示例插件 Component 夹具。
//!
//! 流程：以 wasm32-unknown-unknown 编译 `examples/plugins/probe`（独立工作区）→
//! 用 wit-component 把核心模块与内嵌的 `component-type` 段封装为 `rynvek:plugin@1.0.0` Component →
//! 写入 `crates/rynvek-rules/tests/fixtures/plugin/probe.component.wasm`。
//! 编译时用 `--remap-path-prefix` 抹去本机路径，保证同一工具链下产物逐字节可复现；
//! `--check` 只重建并与已提交夹具比较（ci-full-gate 使用）。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 示例插件工程目录（相对工作区根）
pub const PROBE_DIR: &str = "examples/plugins/probe";
/// 已提交夹具路径（相对工作区根）
pub const PROBE_FIXTURE: &str = "crates/rynvek-rules/tests/fixtures/plugin/probe.component.wasm";

/// 编译并封装示例插件，返回 Component 字节
pub fn build_probe(root: &Path) -> Result<Vec<u8>, String> {
    let target_dir = root.join("target/plugin-fixtures");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .unwrap_or_default();
    // 工具链 sysroot（启用 rust-src 时标准库泛型实例化会带上本机 rustup 路径）
    let sysroot = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .args(["--print", "sysroot"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let mut rustflags = format!(
        "--remap-path-prefix={}=/rynvek --remap-path-prefix={}=/cargo",
        root.display(),
        cargo_home.display()
    );
    if !sysroot.is_empty() {
        rustflags.push_str(&format!(" --remap-path-prefix={sysroot}=/rustc"));
    }
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
        ])
        .arg(root.join(PROBE_DIR).join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .env("RUSTFLAGS", rustflags)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .map_err(|e| format!("无法启动 cargo：{e}"))?;
    if !status.success() {
        return Err(format!("示例插件编译失败：{status}"));
    }
    let core = target_dir.join("wasm32-unknown-unknown/release/rynvek_plugin_probe.wasm");
    let module = std::fs::read(&core).map_err(|e| format!("读取 {} 失败：{e}", core.display()))?;
    componentize(&module)
}

/// 把带 `component-type` 段的核心模块封装为 Component
pub fn componentize(module: &[u8]) -> Result<Vec<u8>, String> {
    wit_component::ComponentEncoder::default()
        .module(module)
        .map_err(|e| format!("解析核心模块失败：{e:#}"))?
        .validate(true)
        .encode()
        .map_err(|e| format!("封装 Component 失败：{e:#}"))
}

/// 子命令入口
pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let bytes = build_probe(root)?;
    let fixture = root.join(PROBE_FIXTURE);
    let hash = blake3::hash(&bytes).to_hex();
    if check {
        let committed = std::fs::read(&fixture).map_err(|e| format!("读取已提交夹具失败：{e}"))?;
        if committed != bytes {
            return Err(format!(
                "插件夹具已过期：重建产物 blake3={hash} 与已提交的 {} 不一致，请运行 `cargo xtask build-plugin-fixtures`",
                blake3::hash(&committed).to_hex()
            ));
        }
        println!("插件夹具最新：{PROBE_FIXTURE}（{} 字节，blake3={hash}）", bytes.len());
    } else {
        if let Some(dir) = fixture.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建夹具目录失败：{e}"))?;
        }
        std::fs::write(&fixture, &bytes).map_err(|e| format!("写入夹具失败：{e}"))?;
        println!("已写入 {PROBE_FIXTURE}（{} 字节，blake3={hash}）", bytes.len());
    }
    Ok(())
}
