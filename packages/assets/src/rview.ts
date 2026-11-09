// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// `.rview` 视觉包容器（魔数 `RYVW`，规范 10.1）的读写器。
//
// 规范只冻结了魔数、Little-Endian 与 64 字节对齐文件头；文件头字段位置沿用 `.rsnap` 的
// `SnapshotFileHeader` 布局（magic / format_version / flags / header_bytes / count / 16 字节 ID / 三个 u64），
// 其余结构按最佳实践补全（ERRATA E-019）：
//
// | 偏移 | 长度 | 字段 |
// | :--- | :--- | :--- |
// | 0  | 4  | `magic` = `RYVW` |
// | 4  | 2  | `format_version` = 1 |
// | 6  | 2  | `flags`（v1 必须为 0） |
// | 8  | 2  | `header_bytes` = 64 |
// | 10 | 2  | `entry_count` |
// | 12 | 4  | `reserved0` = 0 |
// | 16 | 16 | `package_key` = BLAKE3(UTF-8 包 ID) 前 16 字节 |
// | 32 | 8  | `manifest_bytes` |
// | 40 | 8  | `payload_bytes` |
// | 48 | 8  | `total_bytes`（整个文件长度，用于截断检测） |
// | 56 | 8  | `reserved1` = 0 |
//
// 文件头后紧跟 32 字节 `content_hash` = BLAKE3(manifest ‖ payload)，再跟 UTF-8 清单 JSON，
// 补零到 8 字节对齐后是载荷区；每个条目在载荷区内 8 字节对齐并带独立 BLAKE3。
// 结构 / 边界 / 清单错误 → 0x7001，摘要不符 → 0x7002。

import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { blake3Digest, bytesEqual, toHex } from "./bytes.js";

export const RVIEW_MAGIC = new Uint8Array([0x52, 0x59, 0x56, 0x57]);
export const RVIEW_FORMAT_VERSION = 1;
export const RVIEW_HEADER_BYTES = 64;
export const RVIEW_CONTENT_HASH_BYTES = 32;
/** 条目与载荷区对齐 */
export const RVIEW_ALIGN = 8;
/** 清单 JSON 上限 */
export const RVIEW_MAX_MANIFEST_BYTES = 1024 * 1024;
/** 整包上限（视觉包只含色板、主题、着色器与精灵图集） */
export const RVIEW_MAX_TOTAL_BYTES = 256 * 1024 * 1024;
/** 清单 schema 标识 */
export const RVIEW_MANIFEST_SCHEMA = "rynvek.rview.v1";

/** 条目类型 */
export type RviewEntryKind = "palette" | "theme" | "shader" | "sprite_atlas";
export const RVIEW_ENTRY_KINDS: readonly RviewEntryKind[] = ["palette", "theme", "shader", "sprite_atlas"] as const;

/** 各条目类型允许的媒体类型 */
export const RVIEW_MEDIA_TYPES: Readonly<Record<RviewEntryKind, readonly string[]>> = {
  palette: ["application/json"],
  theme: ["application/json"],
  shader: ["text/wgsl"],
  sprite_atlas: ["image/png", "application/json"],
};

/** 包 ID：小写点分命名空间，如 `rynvek.visual.industrial-default` */
export const RVIEW_PACKAGE_ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+$/;
/** 语义化版本（不含构建元数据） */
export const RVIEW_VERSION_PATTERN = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/;
/** 条目路径：相对路径，禁止 `..`、绝对路径与反斜杠 */
export const RVIEW_ENTRY_PATH_PATTERN = /^(?!.*(?:^|\/)\.\.?(?:\/|$))[a-z0-9_-]+(?:\/[a-z0-9_.-]+)*\.[a-z0-9]+$/;
/** 允许的内容许可证（ADR-L007：美术资产 CC0 / CC-BY；本仓库代码 AGPL） */
export const RVIEW_LICENSES = ["CC0-1.0", "CC-BY-4.0", "AGPL-3.0-only"] as const;

export interface RviewManifestEntry {
  readonly path: string;
  readonly kind: RviewEntryKind;
  readonly media_type: string;
  /** 相对载荷区起点的偏移 */
  readonly offset: number;
  readonly length: number;
  readonly blake3_256: string;
}

export interface RviewManifest {
  readonly schema: typeof RVIEW_MANIFEST_SCHEMA;
  readonly id: string;
  readonly version: string;
  readonly name: string;
  readonly license: (typeof RVIEW_LICENSES)[number];
  readonly engine_compat: string;
  readonly entries: readonly RviewManifestEntry[];
}

export interface RviewHeader {
  readonly formatVersion: number;
  readonly flags: number;
  readonly entryCount: number;
  readonly packageKey: Uint8Array;
  readonly manifestBytes: number;
  readonly payloadBytes: number;
  readonly totalBytes: number;
}

export interface RviewArchive {
  readonly header: RviewHeader;
  readonly contentHash: Uint8Array;
  readonly manifest: RviewManifest;
  /** 路径 → 条目字节（零拷贝子视图） */
  readonly files: ReadonlyMap<string, Uint8Array>;
}

/** 写入时的条目描述 */
export interface RviewInputEntry {
  readonly path: string;
  readonly kind: RviewEntryKind;
  readonly media_type: string;
  readonly data: Uint8Array;
}

export interface RviewInput {
  readonly id: string;
  readonly version: string;
  readonly name: string;
  readonly license: RviewManifest["license"];
  readonly engine_compat: string;
  readonly entries: readonly RviewInputEntry[];
}

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

function invalid(detail: string): never {
  throw new RynvekError(RynvekErrorCode.InvalidFileMagicOrVersion, detail);
}

function checksum(detail: string): never {
  throw new RynvekError(RynvekErrorCode.Blake3ChecksumMismatch, detail);
}

const align = (n: number): number => Math.ceil(n / RVIEW_ALIGN) * RVIEW_ALIGN;

/** 包 ID 的 16 字节键 */
export function rviewPackageKey(id: string): Uint8Array {
  return blake3Digest(encoder.encode(id)).slice(0, 16);
}

/** 生成 `.rview`；同一输入总是产生逐字节相同的输出（条目按路径排序） */
export function writeRview(input: RviewInput): Uint8Array {
  const sorted = [...input.entries].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  let cursor = 0;
  const entries: RviewManifestEntry[] = sorted.map((e) => {
    const entry: RviewManifestEntry = {
      path: e.path,
      kind: e.kind,
      media_type: e.media_type,
      offset: cursor,
      length: e.data.length,
      blake3_256: toHex(blake3Digest(e.data)),
    };
    cursor = align(cursor + e.data.length);
    return entry;
  });
  const manifest: RviewManifest = {
    schema: RVIEW_MANIFEST_SCHEMA,
    id: input.id,
    version: input.version,
    name: input.name,
    license: input.license,
    engine_compat: input.engine_compat,
    entries,
  };
  validateManifest(manifest, cursor);

  const manifestBytes = encoder.encode(JSON.stringify(manifest));
  const manifestRegion = align(manifestBytes.length);
  const payload = new Uint8Array(cursor);
  sorted.forEach((e, i) => payload.set(e.data, entries[i]?.offset ?? 0));

  const totalBytes = RVIEW_HEADER_BYTES + RVIEW_CONTENT_HASH_BYTES + manifestRegion + payload.length;
  const out = new Uint8Array(totalBytes);
  const view = new DataView(out.buffer);
  out.set(RVIEW_MAGIC, 0);
  view.setUint16(4, RVIEW_FORMAT_VERSION, true);
  view.setUint16(6, 0, true);
  view.setUint16(8, RVIEW_HEADER_BYTES, true);
  view.setUint16(10, entries.length, true);
  out.set(rviewPackageKey(input.id), 16);
  view.setBigUint64(32, BigInt(manifestBytes.length), true);
  view.setBigUint64(40, BigInt(payload.length), true);
  view.setBigUint64(48, BigInt(totalBytes), true);
  out.set(blake3Digest(manifestBytes, payload), RVIEW_HEADER_BYTES);
  const manifestStart = RVIEW_HEADER_BYTES + RVIEW_CONTENT_HASH_BYTES;
  out.set(manifestBytes, manifestStart);
  out.set(payload, manifestStart + manifestRegion);
  return out;
}

/** 解析 64 字节文件头 */
export function readRviewHeader(bytes: Uint8Array): RviewHeader {
  if (bytes.length < RVIEW_HEADER_BYTES + RVIEW_CONTENT_HASH_BYTES) invalid("文件过短");
  for (let i = 0; i < 4; i++) if (bytes[i] !== RVIEW_MAGIC[i]) invalid("魔数不是 RYVW");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const formatVersion = view.getUint16(4, true);
  if (formatVersion !== RVIEW_FORMAT_VERSION) invalid(`不支持的格式版本 ${formatVersion}`);
  const flags = view.getUint16(6, true);
  if (flags !== 0) invalid(`未知标志位 0x${flags.toString(16)}`);
  if (view.getUint16(8, true) !== RVIEW_HEADER_BYTES) invalid("header_bytes 必须为 64");
  if (view.getUint32(12, true) !== 0 || view.getBigUint64(56, true) !== 0n) invalid("保留字段必须为 0");
  const big = (off: number): number => {
    const v = view.getBigUint64(off, true);
    if (v > BigInt(RVIEW_MAX_TOTAL_BYTES)) invalid("长度字段超出上限");
    return Number(v);
  };
  return {
    formatVersion,
    flags,
    entryCount: view.getUint16(10, true),
    packageKey: bytes.slice(16, 32),
    manifestBytes: big(32),
    payloadBytes: big(40),
    totalBytes: big(48),
  };
}

/** 读取并完整校验 `.rview`（结构、清单、整包与逐条目 BLAKE3） */
export function readRview(bytes: Uint8Array): RviewArchive {
  const header = readRviewHeader(bytes);
  if (header.totalBytes !== bytes.length) invalid(`文件长度 ${bytes.length} 与文件头 ${header.totalBytes} 不符（截断或尾随数据）`);
  if (header.manifestBytes === 0 || header.manifestBytes > RVIEW_MAX_MANIFEST_BYTES) invalid("清单长度非法");
  const manifestStart = RVIEW_HEADER_BYTES + RVIEW_CONTENT_HASH_BYTES;
  const payloadStart = manifestStart + align(header.manifestBytes);
  if (payloadStart + header.payloadBytes !== bytes.length) invalid("区段长度与文件长度不一致");

  const manifestBytes = bytes.subarray(manifestStart, manifestStart + header.manifestBytes);
  const payload = bytes.subarray(payloadStart, payloadStart + header.payloadBytes);
  const contentHash = bytes.slice(RVIEW_HEADER_BYTES, manifestStart);
  if (!bytesEqual(blake3Digest(manifestBytes, payload), contentHash)) checksum("整包 content_hash 不符");
  for (let i = manifestStart + header.manifestBytes; i < payloadStart; i++) {
    if (bytes[i] !== 0) invalid("清单填充字节必须为 0");
  }

  let raw: unknown;
  try {
    raw = JSON.parse(decoder.decode(manifestBytes));
  } catch {
    invalid("清单不是合法的 UTF-8 JSON");
  }
  const manifest = parseRviewManifest(raw, header.payloadBytes);
  if (manifest.entries.length !== header.entryCount) invalid("entry_count 与清单条目数不符");
  if (!bytesEqual(rviewPackageKey(manifest.id), header.packageKey)) invalid("package_key 与清单 id 不符");

  const files = new Map<string, Uint8Array>();
  for (const e of manifest.entries) {
    const data = payload.subarray(e.offset, e.offset + e.length);
    if (toHex(blake3Digest(data)) !== e.blake3_256) checksum(`条目 ${e.path} 的 BLAKE3 不符`);
    files.set(e.path, data);
  }
  return { header, contentHash, manifest, files };
}

/** 校验清单 JSON 结构并收窄类型；`payloadBytes` 用于越界检查 */
export function parseRviewManifest(raw: unknown, payloadBytes: number): RviewManifest {
  if (!isRecord(raw)) invalid("清单必须是对象");
  onlyKeys(raw, ["schema", "id", "version", "name", "license", "engine_compat", "entries"], "清单");
  if (raw["schema"] !== RVIEW_MANIFEST_SCHEMA) invalid(`schema 必须为 ${RVIEW_MANIFEST_SCHEMA}`);
  const entriesRaw = raw["entries"];
  if (!Array.isArray(entriesRaw)) invalid("entries 必须是数组");
  const entries = entriesRaw.map((e: unknown, i): RviewManifestEntry => {
    if (!isRecord(e)) invalid(`entries[${i}] 必须是对象`);
    onlyKeys(e, ["path", "kind", "media_type", "offset", "length", "blake3_256"], `entries[${i}]`);
    return {
      path: str(e["path"], `entries[${i}].path`),
      kind: str(e["kind"], `entries[${i}].kind`) as RviewEntryKind,
      media_type: str(e["media_type"], `entries[${i}].media_type`),
      offset: int(e["offset"], `entries[${i}].offset`),
      length: int(e["length"], `entries[${i}].length`),
      blake3_256: str(e["blake3_256"], `entries[${i}].blake3_256`),
    };
  });
  const manifest: RviewManifest = {
    schema: RVIEW_MANIFEST_SCHEMA,
    id: str(raw["id"], "id"),
    version: str(raw["version"], "version"),
    name: str(raw["name"], "name"),
    license: str(raw["license"], "license") as RviewManifest["license"],
    engine_compat: str(raw["engine_compat"], "engine_compat"),
    entries,
  };
  validateManifest(manifest, payloadBytes);
  return manifest;
}

function validateManifest(m: RviewManifest, payloadBytes: number): void {
  if (!RVIEW_PACKAGE_ID_PATTERN.test(m.id)) invalid(`包 ID 非法：${m.id}`);
  if (!RVIEW_VERSION_PATTERN.test(m.version)) invalid(`版本号非法：${m.version}`);
  if (m.name.length === 0 || m.name.length > 128) invalid("name 长度必须在 1..128");
  if (!(RVIEW_LICENSES as readonly string[]).includes(m.license)) invalid(`许可证不在允许列表：${m.license}`);
  if (m.engine_compat.length === 0 || m.engine_compat.length > 64) invalid("engine_compat 非法");
  if (m.entries.length > 0xffff) invalid("条目过多");
  const seen = new Set<string>();
  let expectedOffset = 0;
  for (const e of m.entries) {
    if (!RVIEW_ENTRY_PATH_PATTERN.test(e.path) || e.path.length > 256) invalid(`条目路径非法：${e.path}`);
    if (seen.has(e.path)) invalid(`条目路径重复：${e.path}`);
    seen.add(e.path);
    if (!RVIEW_ENTRY_KINDS.includes(e.kind)) invalid(`未知条目类型：${e.kind}`);
    if (!RVIEW_MEDIA_TYPES[e.kind].includes(e.media_type)) invalid(`${e.kind} 不允许媒体类型 ${e.media_type}`);
    if (!/^[0-9a-f]{64}$/.test(e.blake3_256)) invalid(`条目 ${e.path} 的 blake3_256 必须是 64 位小写十六进制`);
    // 条目按路径排序且紧密 8 字节对齐排布，杜绝重叠与隐藏数据
    if (e.offset !== expectedOffset) invalid(`条目 ${e.path} 偏移不连续`);
    expectedOffset = align(e.offset + e.length);
  }
  const paths = m.entries.map((e) => e.path);
  if (paths.some((p, i) => i > 0 && (paths[i - 1] ?? "") >= p)) invalid("条目必须按路径升序排列");
  if (expectedOffset !== payloadBytes) invalid("条目总长与载荷区长度不符");
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function onlyKeys(obj: Record<string, unknown>, allowed: readonly string[], where: string): void {
  for (const k of Object.keys(obj)) if (!allowed.includes(k)) invalid(`${where} 含未知字段 ${k}`);
  for (const k of allowed) if (!(k in obj)) invalid(`${where} 缺少字段 ${k}`);
}

function str(v: unknown, where: string): string {
  if (typeof v !== "string") invalid(`${where} 必须是字符串`);
  return v;
}

function int(v: unknown, where: string): number {
  if (typeof v !== "number" || !Number.isSafeInteger(v) || v < 0) invalid(`${where} 必须是非负整数`);
  return v;
}
