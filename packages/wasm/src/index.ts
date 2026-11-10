// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/wasm`：`crates/rynvek-wasm` C ABI 桥接与 Worker 消息编解码适配器（规范 07 / 审计报告 §12.2）。
 */

import { RynvekError, RynvekErrorCode } from "@rynvek/core";

/** `crates/rynvek-wasm` 导出的最小 WebAssembly C ABI 符号表契约 */
export const RYNVEK_WASM_EXPORTS = [
  "rynvek_wasm_alloc",
  "rynvek_wasm_dealloc",
  "rynvek_host_new",
  "rynvek_host_free",
  "rynvek_host_begin_init",
  "rynvek_host_complete_init_headless",
  "rynvek_host_handle_json",
  "rynvek_host_on_frame",
  "rynvek_host_drain_events_json",
] as const;

export type RynvekWasmExportName = (typeof RYNVEK_WASM_EXPORTS)[number];

export interface WasmEngineInitPayload {
  readonly type: "ENGINE_INIT";
  readonly width: number;
  readonly height: number;
  readonly devicePixelRatio: number;
  readonly worldId?: string;
  readonly gpuMode: "auto" | "required" | "disabled";
}

/** 构造并校验发送给 WASM Worker 的 `ENGINE_INIT` JSON 字符串 */
export function encodeWasmInitCommand(payload: WasmEngineInitPayload): string {
  if (!Number.isInteger(payload.width) || !Number.isInteger(payload.height) || payload.width <= 0 || payload.height <= 0) {
    throw new RynvekError(RynvekErrorCode.DimensionMismatch, `画布尺寸非法：${payload.width}x${payload.height}`);
  }
  return JSON.stringify(payload);
}

/** 校验一个 WebAssembly 实例导出对象是否完整包含 Rynvek WASM ABI 符号 */
export function verifyWasmExports(exportsObj: Readonly<Record<string, unknown>>): boolean {
  return RYNVEK_WASM_EXPORTS.every((name) => typeof exportsObj[name] === "function");
}
