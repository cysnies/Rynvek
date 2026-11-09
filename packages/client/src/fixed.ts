// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Q32.32 原始坐标与 Tile 坐标互换。客户端下发的目标点一律取 Tile 中心 `(x + 0.5, y + 0.5)`，
 * 与 Rust `Vec2::tile_center` 逐位相同，避免浮点舍入导致的跨语言差异。
 */

import type { Vec2Raw } from "./protocol.js";

export const Q32_ONE = 2 ** 32;
const HALF = 2 ** 31;

/** Tile 中心点的 Q32.32 原始值 */
export function tileCenterRaw(x: number, y: number): Vec2Raw {
  return [x * Q32_ONE + HALF, y * Q32_ONE + HALF];
}

/** Q32.32 原始坐标 → 所在 Tile（向下取整） */
export function rawToTile(p: Vec2Raw): [number, number] {
  return [Math.floor(p[0] / Q32_ONE), Math.floor(p[1] / Q32_ONE)];
}

/** Q32.32 原始坐标 → 浮点 Tile 坐标（仅用于渲染与 UI，不回流权威层） */
export function rawToFloat(p: Vec2Raw): [number, number] {
  return [p[0] / Q32_ONE, p[1] / Q32_ONE];
}

/** milli 定点 → 展示用整数单位（向零截断，与 Rust 整数除法一致） */
export function milliToWhole(v: number): number {
  return Math.trunc(v / 1000);
}
