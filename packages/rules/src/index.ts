// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * `@rynvek/rules`：Rynvek 规则创作 SDK（客户端 SDK，`Apache-2.0 WITH LLVM-exception`，GAP-15）。
 *
 * - 源码工具链：`@rynvek/rules/ast` 子入口以真实TS AST生成原Builder图，主入口不自动加载解析器。
 * - Tier-0：`Kernel.define` 声明式 AST Builder → `rynvek.rule-graph.v1` JSON → Rust `rynvek-rules` 编译为 `.rir` / WGSL；
 * - Tier-1：`QuickJsSandbox`（QuickJS-NG WASM）确定性执行 TS/JS 规则脚本。
 */

export * from "./ir.js";
export * from "./kernel.js";
export * from "./sandbox.js";
