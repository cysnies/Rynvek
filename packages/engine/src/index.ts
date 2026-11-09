// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/engine`：Rynvek 浏览器引擎宿主（`AGPL-3.0-only`，不在 GAP-15 客户端 SDK 许可清单内）。
 *
 * - `protocol`：与 `rynvek-engine::protocol` 逐字段对齐的主线程 ⇄ Worker 消息类型与运行时校验；
 * - `host`：主线程 `EngineHost`（OffscreenCanvas 一次性移交、状态机、拾取关联）；
 * - `worker`：Worker 侧胶水 `runEngineWorker`（转交 `rynvek-wasm` 的 `EngineWorker`）。
 */

export * from "./host.js";
export * from "./protocol.js";
