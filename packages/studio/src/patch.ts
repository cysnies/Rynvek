// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// `WorldPatch` 创作态补丁（规范 10.2，`crates/rynvek-world/src/authoring.rs`）的 TypeScript 镜像。
//
// 线上形态严格对应 serde 缺省 JSON 表示：
//   - 结构体字段为 snake_case，按声明顺序输出；
//   - `[u8; N]` 序列化为 N 个整数的数组；
//   - `WorldPatchOp` 为外部标记枚举：`{ "SetWorldParam": { ... } }`；
//   - `LiveEditSafetyClass` 在规范中未派生 serde，这里取 serde 对单元变体的缺省形态——变体名字符串（ERRATA E-021）。

import { RynvekError, RynvekErrorCode } from "@rynvek/core";

/** 实时修改的安全分级（数值与 Rust `#[repr(u8)]` 一致） */
export const LiveEditSafetyClass = {
  /** Class 0：视觉颜色、UI 布局、AudioProfile 参数，任意渲染帧立即热生效，不影响权威状态哈希 */
  HotSafePresentation: 0,
  /** Class 1：环境场参数、建筑造价、单位数值属性，打包为系统命令在下一 Tick Phase 01 生效 */
  TickBoundarySafe: 1,
  /** Class 2：神经网络拓扑规则、TypeScript 规则热重载（.rir），在下一个 Coupling Epoch（tick % 4 == 0）原子切换 */
  EpochBoundarySafe: 2,
  /** Class 3：更改 Chunk 尺寸、空间维度或数值 Profile，必须创建新 WorldFork 分支重启 */
  RequiresForkRestart: 3,
} as const;

export type LiveEditSafetyClass = (typeof LiveEditSafetyClass)[keyof typeof LiveEditSafetyClass];
export type LiveEditSafetyClassName = keyof typeof LiveEditSafetyClass;

export const SAFETY_CLASS_NAMES: readonly LiveEditSafetyClassName[] = [
  "HotSafePresentation",
  "TickBoundarySafe",
  "EpochBoundarySafe",
  "RequiresForkRestart",
];

/** 混合逻辑时钟时间戳 */
export interface HlcTimestamp {
  readonly wall_time_ms: number;
  readonly logical_counter: number;
  readonly author_node_id: number;
}

export type WorldPatchOp =
  | { readonly SetWorldParam: { readonly key_path: string; readonly new_value_json: string } }
  | {
      readonly PaintTerrainRect: {
        readonly x0: number;
        readonly y0: number;
        readonly width: number;
        readonly height: number;
        readonly tile_kind: number;
      };
    }
  | { readonly UpsertSeedGenome: { readonly genome_slug: string; readonly genome_bytes_b64: string } }
  | {
      readonly UpdateRuleModule: {
        readonly module_id: string;
        readonly ts_source: string;
        readonly compiled_rir_blake3: readonly number[];
      };
    }
  | { readonly RemoveEntityTemplate: { readonly template_id: string } };

export type WorldPatchOpKind = "SetWorldParam" | "PaintTerrainRect" | "UpsertSeedGenome" | "UpdateRuleModule" | "RemoveEntityTemplate";

export const WORLD_PATCH_OP_KINDS: readonly WorldPatchOpKind[] = [
  "SetWorldParam",
  "PaintTerrainRect",
  "UpsertSeedGenome",
  "UpdateRuleModule",
  "RemoveEntityTemplate",
];

export interface WorldPatch {
  readonly patch_id: readonly number[];
  readonly base_authoring_revision: number;
  readonly hlc: HlcTimestamp;
  readonly safety_class: LiveEditSafetyClassName;
  readonly operations: readonly WorldPatchOp[];
}

/** 地形类型（规范 09 `TerrainTileKind`，数值即 `tile_kind`） */
export const TerrainTileKind = {
  OpenSubstrate: 0,
  ConductiveBasalt: 1,
  NutrientMarsh: 2,
  AcidicFissure: 3,
  ImperviousWall: 4,
} as const;
export type TerrainTileKind = (typeof TerrainTileKind)[keyof typeof TerrainTileKind];

/** 单个补丁的操作数上限与单次涂刷面积上限（防止一次补丁拖垮所有协作端） */
export const MAX_OPERATIONS_PER_PATCH = 4096;
export const MAX_PAINT_AREA = 1 << 20;
export const KEY_PATH_PATTERN = /^[a-z][a-z0-9_]*(?:\.[a-z0-9_]+)*$/;
export const SLUG_PATTERN = /^[a-z0-9]+(?:[-_.][a-z0-9]+)*$/;
const BASE64_PATTERN = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

export function opKind(op: WorldPatchOp): WorldPatchOpKind {
  return Object.keys(op)[0] as WorldPatchOpKind;
}

export function compareHlc(a: HlcTimestamp, b: HlcTimestamp): number {
  // 与 Rust `#[derive(Ord)]` 一致：按字段声明顺序逐项比较
  return (
    a.wall_time_ms - b.wall_time_ms || a.logical_counter - b.logical_counter || a.author_node_id - b.author_node_id
  );
}

export function patchIdHex(id: readonly number[]): string {
  return id.map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** 补丁全序：HLC，其次 patch_id 字节序（同一 HLC 只会来自同一作者的同一补丁，次序键只为防御） */
export function comparePatches(a: WorldPatch, b: WorldPatch): number {
  const c = compareHlc(a.hlc, b.hlc);
  if (c !== 0) return c;
  const ha = patchIdHex(a.patch_id);
  const hb = patchIdHex(b.patch_id);
  return ha < hb ? -1 : ha > hb ? 1 : 0;
}

function invalid(detail: string): never {
  // 规范未给出补丁格式错误码，复用宿主不变量违例（与 E-016 中 Worker 协议违例一致）
  throw new RynvekError(RynvekErrorCode.SimulationInvariantViolated, `WorldPatch: ${detail}`);
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function exactKeys(obj: Record<string, unknown>, keys: readonly string[], where: string): void {
  const got = Object.keys(obj);
  for (const k of got) if (!keys.includes(k)) invalid(`${where} 含未知字段 ${k}`);
  for (const k of keys) if (!(k in obj)) invalid(`${where} 缺少字段 ${k}`);
}

function uint(v: unknown, bits: 8 | 32 | 53, where: string): number {
  const max = bits === 53 ? Number.MAX_SAFE_INTEGER : 2 ** bits - 1;
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > max) invalid(`${where} 必须是 u${bits === 53 ? 64 : bits}`);
  return v;
}

function int32(v: unknown, where: string): number {
  if (typeof v !== "number" || !Number.isInteger(v) || v < -(2 ** 31) || v > 2 ** 31 - 1) invalid(`${where} 必须是 i32`);
  return v;
}

function bytes(v: unknown, len: number, where: string): number[] {
  if (!Array.isArray(v) || v.length !== len) invalid(`${where} 必须是 ${len} 字节数组`);
  return v.map((b, i) => uint(b, 8, `${where}[${i}]`));
}

function string(v: unknown, where: string, max = 1 << 20): string {
  if (typeof v !== "string" || v.length > max) invalid(`${where} 必须是字符串（≤ ${max}）`);
  return v;
}

export function parseHlc(v: unknown, where = "hlc"): HlcTimestamp {
  if (!isRecord(v)) invalid(`${where} 必须是对象`);
  exactKeys(v, ["wall_time_ms", "logical_counter", "author_node_id"], where);
  return {
    wall_time_ms: uint(v["wall_time_ms"], 53, `${where}.wall_time_ms`),
    logical_counter: uint(v["logical_counter"], 32, `${where}.logical_counter`),
    author_node_id: uint(v["author_node_id"], 32, `${where}.author_node_id`),
  };
}

/** 校验并规范化单个操作（字段顺序与 Rust 声明一致） */
export function parseWorldPatchOp(v: unknown, where = "op"): WorldPatchOp {
  if (!isRecord(v) || Object.keys(v).length !== 1) invalid(`${where} 必须是单键外部标记对象`);
  const kind = Object.keys(v)[0] as string;
  const body = v[kind];
  if (!isRecord(body)) invalid(`${where}.${kind} 必须是对象`);
  const w = `${where}.${kind}`;
  switch (kind) {
    case "SetWorldParam": {
      exactKeys(body, ["key_path", "new_value_json"], w);
      const key_path = string(body["key_path"], `${w}.key_path`, 256);
      if (!KEY_PATH_PATTERN.test(key_path)) invalid(`${w}.key_path 格式非法`);
      const new_value_json = string(body["new_value_json"], `${w}.new_value_json`, 64 * 1024);
      try {
        JSON.parse(new_value_json);
      } catch {
        invalid(`${w}.new_value_json 不是合法 JSON`);
      }
      return { SetWorldParam: { key_path, new_value_json } };
    }
    case "PaintTerrainRect": {
      exactKeys(body, ["x0", "y0", "width", "height", "tile_kind"], w);
      const op = {
        x0: int32(body["x0"], `${w}.x0`),
        y0: int32(body["y0"], `${w}.y0`),
        width: uint(body["width"], 32, `${w}.width`),
        height: uint(body["height"], 32, `${w}.height`),
        tile_kind: uint(body["tile_kind"], 8, `${w}.tile_kind`),
      };
      if (op.width * op.height > MAX_PAINT_AREA) invalid(`${w} 面积超过 ${MAX_PAINT_AREA}`);
      if (op.tile_kind > TerrainTileKind.ImperviousWall) invalid(`${w}.tile_kind 不是已知地形类型`);
      return { PaintTerrainRect: op };
    }
    case "UpsertSeedGenome": {
      exactKeys(body, ["genome_slug", "genome_bytes_b64"], w);
      const genome_slug = string(body["genome_slug"], `${w}.genome_slug`, 128);
      if (!SLUG_PATTERN.test(genome_slug)) invalid(`${w}.genome_slug 格式非法`);
      const genome_bytes_b64 = string(body["genome_bytes_b64"], `${w}.genome_bytes_b64`);
      if (!BASE64_PATTERN.test(genome_bytes_b64)) invalid(`${w}.genome_bytes_b64 不是标准 Base64`);
      return { UpsertSeedGenome: { genome_slug, genome_bytes_b64 } };
    }
    case "UpdateRuleModule": {
      exactKeys(body, ["module_id", "ts_source", "compiled_rir_blake3"], w);
      const module_id = string(body["module_id"], `${w}.module_id`, 128);
      if (!SLUG_PATTERN.test(module_id)) invalid(`${w}.module_id 格式非法`);
      return {
        UpdateRuleModule: {
          module_id,
          ts_source: string(body["ts_source"], `${w}.ts_source`),
          compiled_rir_blake3: bytes(body["compiled_rir_blake3"], 32, `${w}.compiled_rir_blake3`),
        },
      };
    }
    case "RemoveEntityTemplate": {
      exactKeys(body, ["template_id"], w);
      const template_id = string(body["template_id"], `${w}.template_id`, 128);
      if (!SLUG_PATTERN.test(template_id)) invalid(`${w}.template_id 格式非法`);
      return { RemoveEntityTemplate: { template_id } };
    }
    default:
      invalid(`${where} 未知操作 ${kind}`);
  }
}

/** 校验 serde JSON 值并收窄为 `WorldPatch` */
export function parseWorldPatch(v: unknown): WorldPatch {
  if (!isRecord(v)) invalid("补丁必须是对象");
  exactKeys(v, ["patch_id", "base_authoring_revision", "hlc", "safety_class", "operations"], "patch");
  const safety = v["safety_class"];
  if (typeof safety !== "string" || !(SAFETY_CLASS_NAMES as readonly string[]).includes(safety)) {
    invalid(`safety_class 必须是 ${SAFETY_CLASS_NAMES.join(" | ")}`);
  }
  const ops = v["operations"];
  if (!Array.isArray(ops) || ops.length === 0 || ops.length > MAX_OPERATIONS_PER_PATCH) {
    invalid(`operations 必须是 1..${MAX_OPERATIONS_PER_PATCH} 个操作`);
  }
  return {
    patch_id: bytes(v["patch_id"], 16, "patch_id"),
    base_authoring_revision: uint(v["base_authoring_revision"], 53, "base_authoring_revision"),
    hlc: parseHlc(v["hlc"]),
    safety_class: safety as LiveEditSafetyClassName,
    operations: ops.map((op, i) => parseWorldPatchOp(op, `operations[${i}]`)),
  };
}

/** 序列化为与 serde_json 逐字节一致的紧凑 JSON（依赖 parse 后的规范字段顺序） */
export function serializeWorldPatch(patch: WorldPatch): string {
  return JSON.stringify(parseWorldPatch(JSON.parse(JSON.stringify(patch))));
}
