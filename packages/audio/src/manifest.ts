// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// AudioManifest：语义事件 → 音源 的映射清单。
//
// 项目文档 6.2：开发期 24 类事件全部为程序化占位（`status: "placeholder"`），
// 后期只需把条目改为 CC0 / 原创 `.ogg` 路径即可热替换。
// 规范只冻结了 `status: "placeholder"` 与 `.ogg`，其余字段按最佳实践补全（ERRATA E-018），
// 对应 JSON Schema：`protocols/schemas/audio-manifest.schema.json`。

import { AUDIO_BUS_NAMES, isAudioBusName, type AudioBusName } from "./buses.js";
import { PLACEHOLDER_SYNTH_RECIPES } from "./placeholder_synth.js";

/** 清单格式标识 */
export const AUDIO_MANIFEST_SCHEMA = "rynvek.audio-manifest.v1";

/** 音源状态：程序化占位 / 已替换为音频文件 */
export type AudioSourceStatus = "placeholder" | "asset";

export const AUDIO_SOURCE_STATUSES: readonly AudioSourceStatus[] = ["placeholder", "asset"] as const;

/** 允许的音源许可证（与 `LICENSES/` 目录及 ADR-L007 的内容许可一致） */
export const AUDIO_ASSET_LICENSES = ["CC0-1.0", "CC-BY-4.0"] as const;
export type AudioAssetLicense = (typeof AUDIO_ASSET_LICENSES)[number];

interface AudioManifestEntryBase {
  /** 输出总线 */
  readonly bus: AudioBusName;
  /** 条目级增益 [0, 1]，缺省 1 */
  readonly gain?: number;
  /** 同一事件的最短重复触发间隔（毫秒），缺省见 `DEFAULT_COOLDOWN_MS` */
  readonly cooldown_ms?: number;
  /** 同一事件的最大并发发声数，缺省见 `DEFAULT_MAX_VOICES` */
  readonly max_voices?: number;
}

/** 程序化占位条目：由 `ProceduralPlaceholderSynth` 按事件名实时合成 */
export interface PlaceholderAudioEntry extends AudioManifestEntryBase {
  readonly status: "placeholder";
}

/** 音频文件条目：`.ogg` 相对路径（相对清单所在目录） */
export interface AssetAudioEntry extends AudioManifestEntryBase {
  readonly status: "asset";
  readonly src: string;
  readonly license: AudioAssetLicense;
}

export type AudioManifestEntry = PlaceholderAudioEntry | AssetAudioEntry;

export interface AudioManifest {
  readonly schema: typeof AUDIO_MANIFEST_SCHEMA;
  readonly events: Readonly<Record<string, AudioManifestEntry>>;
}

/** 24 类标准语义事件（顺序同规范配方表） */
export const STANDARD_AUDIO_EVENT_IDS: readonly string[] = Object.keys(PLACEHOLDER_SYNTH_RECIPES);

/** 事件名格式：`类别.名称`，小写蛇形 */
export const AUDIO_EVENT_ID_PATTERN = /^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$/;

/** `.ogg` 相对路径格式：禁止绝对路径、协议前缀、反斜杠与 `..` 穿越 */
export const AUDIO_ASSET_SRC_PATTERN = /^(?!\/)(?!.*\\)(?!.*(?:^|\/)\.\.(?:\/|$))(?![a-zA-Z][a-zA-Z0-9+.-]*:)[A-Za-z0-9_./-]+\.ogg$/;

/** 由规范配方表生成的开发期缺省清单：24 条全部为占位 */
export function createPlaceholderManifest(): AudioManifest {
  const events: Record<string, AudioManifestEntry> = {};
  for (const [id, recipe] of Object.entries(PLACEHOLDER_SYNTH_RECIPES)) {
    events[id] = { status: "placeholder", bus: recipe.bus };
  }
  return { schema: AUDIO_MANIFEST_SCHEMA, events };
}

/** 清单校验错误，`path` 指向出错字段 */
export class AudioManifestError extends Error {
  public constructor(
    public readonly path: string,
    message: string,
  ) {
    super(`${path}: ${message}`);
    this.name = "AudioManifestError";
  }
}

/**
 * 校验未知 JSON 并收窄为 `AudioManifest`。
 * 额外约束：占位条目必须对应规范配方（合成器只认识这 24 类），且总线须与配方一致。
 */
export function parseAudioManifest(input: unknown): AudioManifest {
  if (!isRecord(input)) throw new AudioManifestError("$", "清单必须是对象");
  rejectUnknownKeys(input, ["schema", "events"], "$");
  if (input["schema"] !== AUDIO_MANIFEST_SCHEMA) {
    throw new AudioManifestError("$.schema", `必须为 "${AUDIO_MANIFEST_SCHEMA}"`);
  }
  const rawEvents = input["events"];
  if (!isRecord(rawEvents)) throw new AudioManifestError("$.events", "必须是对象");
  const events: Record<string, AudioManifestEntry> = {};
  for (const [id, raw] of Object.entries(rawEvents)) {
    events[id] = parseEntry(id, raw);
  }
  return { schema: AUDIO_MANIFEST_SCHEMA, events };
}

function parseEntry(id: string, raw: unknown): AudioManifestEntry {
  const path = `$.events["${id}"]`;
  if (!AUDIO_EVENT_ID_PATTERN.test(id)) throw new AudioManifestError(path, "事件名必须形如 category.name");
  if (!isRecord(raw)) throw new AudioManifestError(path, "条目必须是对象");
  const bus = raw["bus"];
  if (typeof bus !== "string" || !isAudioBusName(bus)) {
    throw new AudioManifestError(`${path}.bus`, `必须是 ${AUDIO_BUS_NAMES.join(" | ")} 之一`);
  }
  const base: { bus: AudioBusName; gain?: number; cooldown_ms?: number; max_voices?: number } = { bus };
  if (raw["gain"] !== undefined) base.gain = numberIn(raw["gain"], 0, 1, `${path}.gain`);
  if (raw["cooldown_ms"] !== undefined) base.cooldown_ms = numberIn(raw["cooldown_ms"], 0, 60_000, `${path}.cooldown_ms`);
  if (raw["max_voices"] !== undefined) {
    const v = numberIn(raw["max_voices"], 1, 64, `${path}.max_voices`);
    if (!Number.isInteger(v)) throw new AudioManifestError(`${path}.max_voices`, "必须是整数");
    base.max_voices = v;
  }
  const common = ["status", "bus", "gain", "cooldown_ms", "max_voices"];
  if (raw["status"] === "placeholder") {
    rejectUnknownKeys(raw, common, path);
    const recipe = PLACEHOLDER_SYNTH_RECIPES[id];
    if (recipe === undefined) throw new AudioManifestError(path, "占位条目必须对应规范 12.2 的 24 类合成配方之一");
    if (recipe.bus !== bus) throw new AudioManifestError(`${path}.bus`, `占位条目总线必须与配方一致（${recipe.bus}）`);
    return { ...base, status: "placeholder" };
  }
  if (raw["status"] === "asset") {
    rejectUnknownKeys(raw, [...common, "src", "license"], path);
    const src = raw["src"];
    if (typeof src !== "string" || !AUDIO_ASSET_SRC_PATTERN.test(src)) {
      throw new AudioManifestError(`${path}.src`, "必须是不含 .. 的相对 .ogg 路径");
    }
    const license = raw["license"];
    if (typeof license !== "string" || !(AUDIO_ASSET_LICENSES as readonly string[]).includes(license)) {
      throw new AudioManifestError(`${path}.license`, `必须是 ${AUDIO_ASSET_LICENSES.join(" | ")} 之一`);
    }
    return { ...base, status: "asset", src, license: license as AudioAssetLicense };
  }
  throw new AudioManifestError(`${path}.status`, `必须是 ${AUDIO_SOURCE_STATUSES.join(" | ")} 之一`);
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function rejectUnknownKeys(obj: Record<string, unknown>, allowed: readonly string[], path: string): void {
  for (const key of Object.keys(obj)) {
    if (!allowed.includes(key)) throw new AudioManifestError(`${path}.${key}`, "未知字段");
  }
}

function numberIn(v: unknown, min: number, max: number, path: string): number {
  if (typeof v !== "number" || !Number.isFinite(v) || v < min || v > max) {
    throw new AudioManifestError(path, `必须是 [${min}, ${max}] 内的数值`);
  }
  return v;
}
