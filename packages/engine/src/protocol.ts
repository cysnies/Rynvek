// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * React 主线程 ⇄ Dedicated Engine Worker 消息协议（规范 07 §7.1 第 1 条）。
 *
 * 与 Rust `rynvek-engine::protocol` 逐字段一一对应（ERRATA E-016）：`type` 为 SCREAMING_SNAKE_CASE，
 * 字段为 camelCase。`ENGINE_INIT` 额外携带不可 JSON 化的 `canvas: OffscreenCanvas`（随 transfer 列表移交）。
 * 跨语言一致性由共享夹具 `tests/engine/protocol_messages.json` 双端断言。
 */

import type { BuildingKind, FactionObservation } from "@rynvek/client";

/** GPU 使用策略 */
export type GpuMode = "auto" | "disabled" | "required";

/** 计算后端 */
export type ComputeBackendKind = "webgpu" | "cpu-reference";

/** L0–L7 渲染层位（与 `rynvek-render::RenderLayer` 一致） */
export const RenderLayer = {
  Terrain: 0,
  FieldHeatmap: 1,
  Structures: 2,
  Organisms: 3,
  Bonds: 4,
  NeuralOverlay: 5,
  FogOfWar: 6,
  Overlay: 7,
} as const;
export type RenderLayer = (typeof RenderLayer)[keyof typeof RenderLayer];

/** 全部图层开启 */
export const ALL_LAYERS = 0xff;

/** 由图层列表构造 `SET_LAYER_MASK` 位集 */
export function layerMask(layers: readonly RenderLayer[]): number {
  return layers.reduce<number>((m, l) => m | (1 << l), 0);
}

/** 五个场通道（热力图可选） */
export const FIELD_CHANNELS = ["nutrient", "energy", "signal_a", "signal_b", "hazard"] as const;
export type FieldChannel = (typeof FIELD_CHANNELS)[number];

/** `ENGINE_INIT` 中除画布外的字段 */
export interface EngineInitOptions {
  /** 设备像素宽高 */
  width: number;
  height: number;
  devicePixelRatio: number;
  /** 32 位十六进制世界 ID（缺省全零） */
  worldId?: string | null;
  gpuMode: GpuMode;
  /** 远程阵营只读显示，禁止本地 STEP/START。 */
  arenaFaction?: number;
}

/** 主线程 → Worker 命令（不含 `ENGINE_INIT`，后者由宿主随画布一起发送） */
export type EngineCommand =
  | { type: "RESIZE"; width: number; height: number; devicePixelRatio: number }
  | { type: "GENERAL_WORLD"; packageBytes: number[] }
  | { type: "STEP"; ticks: number }
  | { type: "START"; tickRate: number }
  | { type: "PAUSE" }
  | { type: "SET_CAMERA"; centerX: number; centerY: number; pxPerCell: number }
  | { type: "PICK_REQUEST"; requestId: number; x: number; y: number }
  | { type: "SET_LAYER_MASK"; mask: number }
  | { type: "SET_HEATMAP_CHANNEL"; channel: FieldChannel }
  | { type: "SIMULATE_DEVICE_LOST" }
  | { type:"ARENA_THEME";worldId:string;requestId:number;background:number;factions:readonly number[] }
  | { type: "ARENA_MAP"; worldId: string; map: {width: number; height: number; tiles: number[]; tilesHash: string} }
  | { type: "ARENA_SELECTION"; worldId: string; units: number[]; building: number | null }
  | { type: "ARENA_OVERLAY"; worldId: string; drag: [[number, number], [number, number]] | null; placement: {kind: BuildingKind; origin: [number, number]} | null }
  | { type: "ARENA_OBSERVATION"; worldId: string; observation: FactionObservation }
  | { type: "SHUTDOWN" };

/** `ENGINE_INIT` 消息（`canvas` 为已转移的 OffscreenCanvas） */
export interface EngineInitMessage<C = unknown> extends EngineInitOptions {
  type: "ENGINE_INIT";
  canvas: C;
}

/** 全部命令类型（含 `ENGINE_INIT`） */
export const ENGINE_COMMAND_TYPES = [
  "ENGINE_INIT",
  "RESIZE",
  "GENERAL_WORLD",
  "STEP",
  "START",
  "PAUSE",
  "SET_CAMERA",
  "PICK_REQUEST",
  "SET_LAYER_MASK",
  "SET_HEATMAP_CHANNEL",
  "SIMULATE_DEVICE_LOST",
  "ARENA_THEME",
  "ARENA_MAP",
  "ARENA_SELECTION",
  "ARENA_OVERLAY",
  "ARENA_OBSERVATION",
  "SHUTDOWN",
] as const;

/** Worker → 主线程事件 */
export type EngineEvent =
  | {
      type: "ENGINE_READY";
      backend: ComputeBackendKind;
      /** `TierA` … `TierD`；无 GPU 时为 null */
      profile: string | null;
      adapter: string | null;
      generation: number;
      /** 实际启用的只读竞技场阵营；旧版本/本地仿真不回显。 */
      arenaFaction?: number;
    }
  | { type:"ARENA_THEME_APPLIED";worldId:string;requestId:number;background:number;factions:readonly number[] }
  | { type: "TICK_COMMITTED"; tick: number; stateRoot: string }
  | { type: "FRAME_RENDERED"; tick: number }
  | { type: "PICK_RESULT"; requestId: number; entitySerial: number | null }
  | { type: "DEVICE_LOST"; tick: number; reason: string }
  | { type: "RECOVERY_PROGRESS"; step: number; phase: string }
  | { type: "RECOVERED"; generation: number; resumeTick: number }
  | { type: "BACKEND_DEGRADED"; reason: string }
  | { type: "GPU_DIVERGENCE"; tick: number; channel: string }
  | { type: "ENGINE_ERROR"; code: number; message: string };

export type EngineEventType = EngineEvent["type"];
export type EngineEventOf<T extends EngineEventType> = Extract<EngineEvent, { type: T }>;

type FieldKind = "optional-u8" | "u32" | "number" | "string" | "nullable-string" | "nullable-u32" | "backend" | "hex64" | "opaque7";

/** 每种事件的字段模式（运行时校验 Worker 消息） */
const EVENT_SCHEMA: { readonly [K in EngineEventType]: Readonly<Record<string, FieldKind>> } = {
  ENGINE_READY: { backend: "backend", profile: "nullable-string", adapter: "nullable-string", generation: "u32", arenaFaction: "optional-u8" },
  ARENA_THEME_APPLIED:{worldId:"string",requestId:"u32",background:"u32",factions:"opaque7"},
  TICK_COMMITTED: { tick: "number", stateRoot: "hex64" },
  FRAME_RENDERED: { tick: "number" },
  PICK_RESULT: { requestId: "u32", entitySerial: "nullable-u32" },
  DEVICE_LOST: { tick: "number", reason: "string" },
  RECOVERY_PROGRESS: { step: "u32", phase: "string" },
  RECOVERED: { generation: "u32", resumeTick: "number" },
  BACKEND_DEGRADED: { reason: "string" },
  GPU_DIVERGENCE: { tick: "number", channel: "string" },
  ENGINE_ERROR: { code: "u32", message: "string" },
};

export const ENGINE_EVENT_TYPES = Object.keys(EVENT_SCHEMA) as EngineEventType[];

const isU32 = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0 && v <= 0xffff_ffff;

function checkField(kind: FieldKind, v: unknown): boolean {
  switch (kind) {
    case "opaque7":return Array.isArray(v)&&v.length===7&&v.every(c=>isU32(c)&&(c>>>24)===255);
    case "optional-u8":
      return v === undefined || (isU32(v) && v <= 255);
    case "u32":
      return isU32(v);
    case "number":
      // Tick 为 u64：JSON 中以 number 传递（2^53 以内精确）
      return typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
    case "string":
      return typeof v === "string";
    case "nullable-string":
      return v === null || typeof v === "string";
    case "nullable-u32":
      return v === null || isU32(v);
    case "backend":
      return v === "webgpu" || v === "cpu-reference";
    case "hex64":
      return typeof v === "string" && /^[0-9a-f]{64}$/.test(v);
  }
}

/** 校验并收窄一条 Worker 事件；非法时返回 null */
export function parseEngineEvent(value: unknown): EngineEvent | null {
  if (typeof value !== "object" || value === null) return null;
  const rec = value as Record<string, unknown>;
  const type = rec["type"];
  if (typeof type !== "string" || !Object.hasOwn(EVENT_SCHEMA, type)) return null;
  const schema = EVENT_SCHEMA[type as EngineEventType];
  for (const [field, kind] of Object.entries(schema)) {
    if (!checkField(kind, rec[field])) return null;
  }
  return value as EngineEvent;
}

/** Worker 每次回调可能送达单个事件或事件数组（`rynvek-wasm` 的 ABI 返回 JSON 数组） */
export function parseEngineEvents(data: unknown): { events: EngineEvent[]; rejected: unknown[] } {
  const items = Array.isArray(data) ? data : [data];
  const events: EngineEvent[] = [];
  const rejected: unknown[] = [];
  for (const item of items) {
    const ev = parseEngineEvent(item);
    if (ev) events.push(ev);
    else rejected.push(item);
  }
  return { events, rejected };
}

/** 统一错误码中与宿主相关的子集（与 `rynvek-core::RynvekErrorCode` 相同数值） */
export const EngineErrorCode = {
  DimensionMismatch: 0x1004,
  SimulationInvariantViolated: 0x3003,
  WebGpuAdapterUnavailable: 0x6001,
  GpuDeviceLost: 0x6002,
  GpuBufferAllocationFailed: 0x6003,
  WgslShaderValidationFailed: 0x6004,
} as const;
