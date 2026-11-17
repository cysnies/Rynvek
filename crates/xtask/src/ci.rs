// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `cargo xtask ci-full-gate`：M8 自治验收的完整门禁（阶段 M8 验收指令的第二段）。
//!
//! 依次执行（任一步失败即停止并返回非零退出码）：
//!
//! | # | 步骤 |
//! |---|---|
//! | 1 | `cargo fmt --all --check` |
//! | 2 | `cargo clippy --workspace --all-targets -- -D warnings` |
//! | 3 | `cargo test --workspace` |
//! | 4 | 跨语言黄金向量 `verify-golden-vectors` |
//! | 5 | 插件夹具可复现 `build-plugin-fixtures --check` |
//! | 6 | 命名 / SPDX / 第三方许可证 / SBOM 门禁 `license-gate` |
//! | 7 | TypeScript：`pnpm install --frozen-lockfile --offline` 后 `pnpm -r typecheck`、`pnpm -r test`、`pnpm -r build` |
//! | 8 | Python：`cargo xtask pytest`（uv.lock 可复现环境） |
//! | 9 | 短浸泡：与 24 小时浸泡同一执行体、同一不变量，默认 120 秒（`RYNVEK_SOAK_DURATION` 或 `--soak-duration` 覆盖） |
//!
//! `--skip <步骤名>` 可跳过指定步骤（仅供本地排障；验收不得使用）。

use std::path::Path;
use std::time::Instant;

use crate::{golden_vectors, plugin_fixtures, python, soak, util};

const STEPS: &[&str] = &[
    "fmt",
    "clippy",
    "test",
    "golden",
    "plugin-fixtures",
    "license",
    "ts",
    "python",
    "soak",
];

/// 解析参数并运行全部步骤
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut skip = Vec::new();
    let mut soak_duration = std::env::var("RYNVEK_SOAK_DURATION").unwrap_or_else(|_| soak::CI_DEFAULT.into());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--skip" => {
                let s = it.next().ok_or("--skip 需要步骤名")?;
                if !STEPS.contains(&s.as_str()) {
                    return Err(format!("未知步骤 {s}（可选：{}）", STEPS.join(" / ")));
                }
                skip.push(s.clone());
            }
            "--soak-duration" => soak_duration = it.next().ok_or("--soak-duration 需要参数")?.clone(),
            other => return Err(format!("未知参数 {other}")),
        }
    }
    let soak_duration = soak::parse_duration(&soak_duration)?;
    let started = Instant::now();
    let mut summary = Vec::new();
    for &step in STEPS {
        if skip.iter().any(|s| s == step) {
            summary.push(format!("  - {step:<16} 已跳过"));
            continue;
        }
        eprintln!("\n══ ci-full-gate · {step} ══");
        let t = Instant::now();
        let r = match step {
            "fmt" => cargo(root, &["fmt", "--all", "--check"], "cargo fmt --check"),
            "clippy" => cargo(
                root,
                &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
                "cargo clippy -D warnings",
            ),
            "test" => cargo(root, &["test", "--workspace", "--quiet"], "cargo test --workspace"),
            "golden" => golden(root),
            "plugin-fixtures" => plugin_fixtures::run(root, true),
            "license" => crate::gate::run(root),
            "ts" => typescript(root),
            "python" => python::pytest(root, &[]),
            "soak" => soak::run(root, soak_duration, Some("target/soak/ci-soak-report.json")),
            _ => unreachable!("步骤表与分派一致"),
        };
        let secs = t.elapsed().as_secs_f64();
        match r {
            Ok(()) => summary.push(format!("  ✔ {step:<16} {secs:>8.1}s")),
            Err(e) => {
                summary.push(format!("  ✗ {step:<16} {secs:>8.1}s  {e}"));
                print_summary(&summary, started);
                return Err(format!("ci-full-gate 在步骤 {step} 失败"));
            }
        }
    }
    print_summary(&summary, started);
    Ok(())
}

fn print_summary(lines: &[String], started: Instant) {
    eprintln!("\n══ ci-full-gate 汇总（{:.1}s）══", started.elapsed().as_secs_f64());
    for l in lines {
        eprintln!("{l}");
    }
}

fn cargo(root: &Path, args: &[&str], what: &str) -> Result<(), String> {
    let mut c = util::cargo();
    c.current_dir(root).args(args);
    util::run(c, what)
}

fn golden(root: &Path) -> Result<(), String> {
    let report = golden_vectors::verify_file(&root.join(golden_vectors::BASELINE_RELATIVE_PATH))
        .map_err(|e| format!("无法加载黄金向量：{e}"))?;
    report.print();
    if report.all_passed() {
        Ok(())
    } else {
        Err("黄金向量不一致".into())
    }
}

fn typescript(root: &Path) -> Result<(), String> {
    let pnpm = util::which("pnpm").ok_or("未找到 pnpm（工具链冻结 pnpm 10 / Node 22）")?;
    let pnpm = pnpm.to_string_lossy().into_owned();
    util::run_in(
        root,
        &pnpm,
        &["install", "--frozen-lockfile", "--offline", "--silent"],
        "pnpm install --frozen-lockfile --offline",
    )?;
    // 工作区包的exports指向dist；干净检出必须先按依赖DAG生成真实声明/产物。
    // 否则旧dist会让本机假绿，CI在agent-sdk导入@rynvek/core时以TS2307失败。
    // 后续原typecheck/test/build全部保留，且不静默吞掉具体编译诊断。
    util::run_in(
        root,
        &pnpm,
        &["-r", "--filter", "./packages/**", "--fail-if-no-match", "build"],
        "pnpm 工作区包依赖DAG前置构建（干净检出）",
    )?;
    for script in ["typecheck", "test", "build"] {
        util::run_in(root, &pnpm, &["-r", script], &format!("pnpm -r {script}"))?;
    }
    Ok(())
}
