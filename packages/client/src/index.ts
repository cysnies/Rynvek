// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/client`：Rynvek Arena 客户端运行时。
 *
 * - `protocol`：`rynvek-arena/1` 线上协议类型（与 Rust serde 形态逐字段一致）；
 * - `client`：`ArenaClient` 对局连接（租约、Fencing Token、序号关联、本地预检）；
 * - `balance` / `terrain` / `fixed`：平衡表镜像、地形解码与定点坐标工具；
 * - `apm`：600 APM 预算镜像；
 * - `input`：`InputActionRouter` 运行期输入路由（规范 12.1）；
 * - `transport`：WebSocket / 内存传输。
 */

export * from "./apm.js";
export * from "./balance.js";
export * from "./client.js";
export * from "./fixed.js";
export * from "./input.js";
export * from "./protocol.js";
export * from "./terrain.js";
export * from "./transport.js";
