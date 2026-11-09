// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// `.rview` 加载器：容器校验 → 依赖锁比对 → 按条目类型解码 → WGSL 安全审查。
//
// 条目内容格式规范未给出，按最佳实践补全（ERRATA E-019）：
//   palette      `{ "colors": { "<name>": "#RRGGBB" } }`
//   theme        `{ "tokens": { "--rv-*": "<css 值>" } }`（是否允许覆盖某 Token 由 @rynvek/visualization 按主题层规则裁决）
//   shader       WGSL 源码（UTF-8），审查失败 → 0x9004
//   sprite_atlas PNG 图像，或 `{ "image": "<同包 PNG 路径>", "frames": { "<name>": { x, y, w, h } } }`
// 内容格式错误 → 0x7001；依赖锁冲突 → 0x9003。

import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { blake3Digest, toHex } from "./bytes.js";
import { readRview, type RviewArchive, type RviewManifest } from "./rview.js";
import { TrustLevel } from "./trust.js";
import { reviewRviewShader, type WgslReviewReport } from "./wgsl_review.js";

export interface RviewPalette {
  readonly colors: Readonly<Record<string, string>>;
}

export interface RviewThemeOverride {
  readonly tokens: Readonly<Record<string, string>>;
}

export interface RviewShader {
  readonly source: string;
  readonly review: WgslReviewReport;
}

export interface SpriteFrame {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

export type RviewSpriteAtlas =
  | { readonly format: "png"; readonly bytes: Uint8Array }
  | { readonly format: "frames"; readonly image: string; readonly frames: Readonly<Record<string, SpriteFrame>> };

export interface RviewPackage {
  readonly manifest: RviewManifest;
  readonly trustLevel: TrustLevel;
  /** 整个 `.rview` 文件的 BLAKE3（与 `dependencies.lock` 中的 `blake3_256` 对应） */
  readonly fileBlake3: string;
  readonly palettes: ReadonlyMap<string, RviewPalette>;
  readonly themes: ReadonlyMap<string, RviewThemeOverride>;
  readonly shaders: ReadonlyMap<string, RviewShader>;
  readonly sprites: ReadonlyMap<string, RviewSpriteAtlas>;
}

/** `dependencies.lock` 中的单个包条目（规范 10.3.1） */
export interface LockedPackage {
  readonly id: string;
  readonly version: string;
  readonly kind: "rworld" | "rpack" | "rview" | "audio" | "agent" | "dataset";
  readonly blake3_256: string;
  readonly ed25519_signer?: string;
  readonly ed25519_sig?: string;
  readonly dependencies?: readonly string[];
}

export interface DependencyLock {
  readonly lockfile_version: number;
  readonly packages: readonly LockedPackage[];
}

export interface LoadRviewOptions {
  readonly trustLevel: TrustLevel;
  /** 提供时，包必须与锁文件中的同 ID 条目完全一致 */
  readonly lock?: DependencyLock;
}

export const PALETTE_COLOR_NAME_PATTERN = /^[a-z0-9]+(?:[-_][a-z0-9]+)*$/;
export const HEX_COLOR_PATTERN = /^#[0-9A-Fa-f]{6}$/;
export const RV_TOKEN_PATTERN = /^--rv-[a-z0-9]+(?:-[a-z0-9]+)*$/;
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

const decoder = new TextDecoder("utf-8", { fatal: true });

function invalid(detail: string): never {
  throw new RynvekError(RynvekErrorCode.InvalidFileMagicOrVersion, detail);
}

function lockConflict(detail: string): never {
  throw new RynvekError(RynvekErrorCode.DependencyLockConflict, detail);
}

/** 把 `.rview` 与锁文件条目比对：缺失、类型 / 版本 / 摘要不一致均为 0x9003 */
export function verifyAgainstLock(manifest: RviewManifest, fileBlake3: string, lock: DependencyLock): void {
  if (lock.lockfile_version !== 1) lockConflict(`不支持的 lockfile_version ${lock.lockfile_version}`);
  const pinned = lock.packages.filter((p) => p.id === manifest.id);
  if (pinned.length !== 1) lockConflict(`锁文件中 ${manifest.id} 出现 ${pinned.length} 次`);
  const p = pinned[0] as LockedPackage;
  if (p.kind !== "rview") lockConflict(`${manifest.id} 在锁文件中的类型为 ${p.kind}`);
  if (p.version !== manifest.version) lockConflict(`${manifest.id} 版本 ${manifest.version} 与锁定的 ${p.version} 不符`);
  if (p.blake3_256.toLowerCase() !== fileBlake3) lockConflict(`${manifest.id} 的 BLAKE3 与锁文件不符`);
}

/** 加载并解码视觉包 */
export function loadRview(bytes: Uint8Array, options: LoadRviewOptions): RviewPackage {
  const archive: RviewArchive = readRview(bytes);
  const fileBlake3 = toHex(blake3Digest(bytes));
  if (options.lock !== undefined) verifyAgainstLock(archive.manifest, fileBlake3, options.lock);

  const palettes = new Map<string, RviewPalette>();
  const themes = new Map<string, RviewThemeOverride>();
  const shaders = new Map<string, RviewShader>();
  const sprites = new Map<string, RviewSpriteAtlas>();

  for (const entry of archive.manifest.entries) {
    const data = archive.files.get(entry.path) ?? new Uint8Array();
    switch (entry.kind) {
      case "palette":
        palettes.set(entry.path, parsePalette(json(data, entry.path), entry.path));
        break;
      case "theme":
        themes.set(entry.path, parseTheme(json(data, entry.path), entry.path));
        break;
      case "shader": {
        const source = text(data, entry.path);
        shaders.set(entry.path, { source, review: reviewRviewShader(source, options.trustLevel) });
        break;
      }
      case "sprite_atlas":
        if (entry.media_type === "image/png") {
          if (!PNG_SIGNATURE.every((b, i) => data[i] === b)) invalid(`${entry.path} 不是 PNG`);
          sprites.set(entry.path, { format: "png", bytes: data });
        } else {
          sprites.set(entry.path, parseFrames(json(data, entry.path), entry.path, archive));
        }
        break;
    }
  }
  return { manifest: archive.manifest, trustLevel: options.trustLevel, fileBlake3, palettes, themes, shaders, sprites };
}

function text(data: Uint8Array, path: string): string {
  try {
    return decoder.decode(data);
  } catch {
    invalid(`${path} 不是合法 UTF-8`);
  }
}

function json(data: Uint8Array, path: string): unknown {
  try {
    return JSON.parse(text(data, path));
  } catch (e) {
    if (e instanceof RynvekError) throw e;
    invalid(`${path} 不是合法 JSON`);
  }
}

function record(v: unknown, where: string): Record<string, unknown> {
  if (typeof v !== "object" || v === null || Array.isArray(v)) invalid(`${where} 必须是对象`);
  return v as Record<string, unknown>;
}

function parsePalette(raw: unknown, path: string): RviewPalette {
  const obj = record(raw, path);
  if (Object.keys(obj).some((k) => k !== "colors")) invalid(`${path} 只允许 colors 字段`);
  const colors: Record<string, string> = {};
  for (const [name, value] of Object.entries(record(obj["colors"], `${path}.colors`))) {
    if (!PALETTE_COLOR_NAME_PATTERN.test(name)) invalid(`${path} 色名非法：${name}`);
    if (typeof value !== "string" || !HEX_COLOR_PATTERN.test(value)) invalid(`${path}.${name} 必须是 #RRGGBB`);
    colors[name] = value.toUpperCase();
  }
  return { colors };
}

function parseTheme(raw: unknown, path: string): RviewThemeOverride {
  const obj = record(raw, path);
  if (Object.keys(obj).some((k) => k !== "tokens")) invalid(`${path} 只允许 tokens 字段`);
  const tokens: Record<string, string> = {};
  for (const [name, value] of Object.entries(record(obj["tokens"], `${path}.tokens`))) {
    if (!RV_TOKEN_PATTERN.test(name)) invalid(`${path} Token 名必须以 --rv- 开头：${name}`);
    if (typeof value !== "string" || value.length === 0 || value.length > 256) invalid(`${path}.${name} 值非法`);
    tokens[name] = value;
  }
  return { tokens };
}

function parseFrames(raw: unknown, path: string, archive: RviewArchive): RviewSpriteAtlas {
  const obj = record(raw, path);
  if (Object.keys(obj).some((k) => k !== "image" && k !== "frames")) invalid(`${path} 只允许 image / frames 字段`);
  const image = obj["image"];
  const imageEntry = archive.manifest.entries.find((e) => e.path === image);
  if (typeof image !== "string" || imageEntry?.media_type !== "image/png") invalid(`${path}.image 必须指向同包 PNG`);
  const frames: Record<string, SpriteFrame> = {};
  for (const [name, f] of Object.entries(record(obj["frames"], `${path}.frames`))) {
    const fr = record(f, `${path}.frames.${name}`);
    const n = (k: string): number => {
      const v = fr[k];
      if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 16384) invalid(`${path}.frames.${name}.${k} 非法`);
      return v;
    };
    frames[name] = { x: n("x"), y: n("y"), w: n("w"), h: n("h") };
  }
  return { format: "frames", image, frames };
}
