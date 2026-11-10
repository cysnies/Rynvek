// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/lab`：Rynvek Lab 批量多种子演化实验。
 *
 * - `spec`：实验规格类型、与服务端一致的校验与 run 展开；
 * - `design`：流式构建器、全因子扫描与单因子消融；
 * - `client`：路由 17–18 客户端（提交、轮询、下载 Parquet）；
 * - `lineage`：结果表行类型与谱系 / 指标分析。
 */

export * from "./client.js";
export * from "./design.js";
export * from "./lineage.js";
export * from "./spec.js";
