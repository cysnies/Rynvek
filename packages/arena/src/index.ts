// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/arena`：Rynvek Arena 游戏层。
 *
 * - `state`：界面状态与 HUD 视图模型；
 * - `placement`：建造虚影预览与确定性选址；
 * - `controller`：逻辑动作 → 界面状态 / 权威命令；
 * - `minimap`：小地图光栅化；
 * - `agent`：只用六个受控工具行动的 `ToolAgent`；
 * - `app`：席位级前端闭环组装。
 */

export * from "./agent.js";
export * from "./app.js";
export * from "./controller.js";
export * from "./minimap.js";
export * from "./placement.js";
export * from "./state.js";
