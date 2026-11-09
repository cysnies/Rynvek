// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/ui`：Rynvek 设计系统（`AGPL-3.0-only`）。
 *
 * - `tokens` / `theme` / `color`：`--rv-*` 三主题令牌、五层主题隔离、对比度审计与样式表生成；
 * - `actions`：规范 12.1 的 36 项逻辑动作全表与键盘匹配；
 * - `fuzzy`：命令面板模糊匹配；
 * - `components`：shadcn 风格 React 组件（ThemeProvider、Button、Panel、Tabs、Toolbar、StatusBar、Kbd、CommandPalette）。
 */

export * from "./actions.js";
export * from "./color.js";
export * from "./fuzzy.js";
export * from "./theme.js";
export * from "./tokens.js";
export * from "./components/Button.js";
export * from "./components/CommandPalette.js";
export * from "./components/Panel.js";
export * from "./components/Tabs.js";
export * from "./components/ThemeProvider.js";
export * from "./components/Toolbar.js";
export { COMPONENT_STYLESHEET } from "./components/styles.js";
