// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// AudioDirector：把语义音频事件路由到占位合成器或显式内存加载器提供的采样（Arena仅受限PCM）。
//
// 规范只给出合成器本体；事件路由、重复触发冷却与发声数上限按游戏音频最佳实践补全（ERRATA E-018）：
// 大规模 RTS 同帧可能产生上百个同类事件，不加限制会削波并淹没关键告警。

import { AudioMixer, type AudioBusName } from "./buses.js";
import {
  createPlaceholderManifest,
  type AudioManifest,
  type AudioManifestEntry,
} from "./manifest.js";
import { PLACEHOLDER_SYNTH_RECIPES, ProceduralPlaceholderSynth, type SynthRecipe } from "./placeholder_synth.js";

/** 各总线同一事件缺省冷却（毫秒）：告警最长，避免“基地遭到攻击”连续刷屏 */
export const DEFAULT_COOLDOWN_MS: Readonly<Record<AudioBusName, number>> = {
  sfx_ui: 25,
  sfx_world: 40,
  sfx_combat: 20,
  voice_alert: 1500,
  neural_sonify: 30,
};

/** 同一事件缺省最大并发发声数 */
export const DEFAULT_MAX_VOICES = 4;

/** 各总线最大并发发声数 */
export const BUS_VOICE_LIMITS: Readonly<Record<AudioBusName, number>> = {
  sfx_ui: 8,
  sfx_world: 16,
  sfx_combat: 24,
  voice_alert: 2,
  neural_sonify: 12,
};

/** 采样加载器：输入清单中的 `src`，返回解码后的 AudioBuffer */
export type AudioAssetLoader = (src: string) => Promise<AudioBuffer>;

export interface PlayOptions {
  /** 强度 [0, 1]，映射为峰值电平 */
  readonly intensity?: number;
  /** 声像 [-1, 1] */
  readonly pan?: number;
  /** Camera-derived distance attenuation, applied equally to synth and samples. */
  readonly attenuation?:number;
}

export type PlaySkipReason = "unknown_event" | "context_not_running" | "muted" | "cooldown" | "voice_limit";

export type PlayResult =
  | { readonly played: true; readonly source: "synth" | "sample"; readonly endTime: number }
  | { readonly played: false; readonly reason: PlaySkipReason };

interface ActiveVoice {
  readonly eventId: string;
  readonly bus: AudioBusName;
  readonly endTime: number;
}

export class AudioDirector {
  private readonly ctx: AudioContext;
  public readonly mixer: AudioMixer;
  private readonly synth: ProceduralPlaceholderSynth;
  private readonly recipes: Readonly<Record<string,SynthRecipe>>;
  private readonly loader: AudioAssetLoader | null;
  private manifest: AudioManifest;
  private readonly samples = new Map<string, AudioBuffer>();
  private readonly lastTrigger = new Map<string, number>();
  private voices: ActiveVoice[] = [];

  public constructor(
    ctx: AudioContext,
    options: { mixer?: AudioMixer; manifest?: AudioManifest; loader?: AudioAssetLoader; recipes?: Readonly<Record<string,SynthRecipe>> } = {},
  ) {
    this.ctx = ctx;
    this.mixer = options.mixer ?? new AudioMixer(ctx);
    this.recipes = options.recipes ?? PLACEHOLDER_SYNTH_RECIPES;
    this.synth = new ProceduralPlaceholderSynth(ctx, this.recipes);
    this.loader = options.loader ?? null;
    this.manifest = options.manifest ?? createPlaceholderManifest();
  }

  /** 浏览器自动播放策略要求在用户手势中恢复上下文 */
  public async unlock(): Promise<void> {
    if (this.ctx.state !== "running") await this.ctx.resume();
  }

  public get currentManifest(): AudioManifest {
    return this.manifest;
  }

  /** 热替换清单：已加载但不再引用的采样被释放，新引用的采样需重新 `preload` */
  public replaceManifest(manifest: AudioManifest): void {
    this.manifest = manifest;
    const referenced = new Set(assetSources(manifest));
    for (const src of [...this.samples.keys()]) {
      if (!referenced.has(src)) this.samples.delete(src);
    }
  }

  /**
   * 用显式注入的加载器预加载清单中全部采样；失败的条目保持未加载，播放时回退到占位合成（若有配方）。
   * 返回加载失败的 src 列表。
   */
  public async preload(): Promise<string[]> {
    const loader = this.loader;
    if (loader === null) return assetSources(this.manifest);
    const failed: string[] = [];
    await Promise.all(
      assetSources(this.manifest)
        .filter((src) => !this.samples.has(src))
        .map(async (src) => {
          try {
            this.samples.set(src, await loader(src));
          } catch {
            failed.push(src);
          }
        }),
    );
    return failed.sort();
  }

  /** 当前仍在发声的数量（按预计结束时间统计） */
  public activeVoiceCount(bus?: AudioBusName): number {
    this.pruneVoices();
    return bus === undefined ? this.voices.length : this.voices.filter((v) => v.bus === bus).length;
  }

  /** 触发一条语义音频事件 */
  public play(eventId: string, options: PlayOptions = {}): PlayResult {
    const entry = this.manifest.events[eventId];
    if (entry === undefined) return { played: false, reason: "unknown_event" };
    if (this.ctx.state !== "running") return { played: false, reason: "context_not_running" };
    if (!this.mixer.isAudible(entry.bus)) return { played: false, reason: "muted" };

    const now = this.ctx.currentTime;
    const cooldownSec = (entry.cooldown_ms ?? DEFAULT_COOLDOWN_MS[entry.bus]) / 1000;
    const last = this.lastTrigger.get(eventId);
    if (last !== undefined && now - last < cooldownSec) return { played: false, reason: "cooldown" };

    this.pruneVoices();
    const sameEvent = this.voices.filter((v) => v.eventId === eventId).length;
    const sameBus = this.voices.filter((v) => v.bus === entry.bus).length;
    if (sameEvent >= (entry.max_voices ?? DEFAULT_MAX_VOICES) || sameBus >= BUS_VOICE_LIMITS[entry.bus]) {
      return { played: false, reason: "voice_limit" };
    }

    const result = this.render(eventId, entry, options);
    if (result === null) return { played: false, reason: "unknown_event" };
    this.lastTrigger.set(eventId, now);
    this.voices.push({ eventId, bus: entry.bus, endTime: result.endTime });
    return { played: true, ...result };
  }

  private render(
    eventId: string,
    entry: AudioManifestEntry,
    options: PlayOptions,
  ): { source: "synth" | "sample"; endTime: number } | null {
    const now = this.ctx.currentTime;
    const intensity = clamp01(options.intensity ?? 1) * clamp01(options.attenuation ?? 1);
    const pan = options.pan ?? 0;
    const sample = entry.status === "asset" ? this.samples.get(entry.src) : undefined;
    if (sample !== undefined) {
      const source = this.ctx.createBufferSource();
      source.buffer = sample;
      const gain = this.ctx.createGain();
      gain.gain.setValueAtTime(clamp01(intensity) * (entry.gain ?? 1), now);
      const panner = this.ctx.createStereoPanner();
      panner.pan.setValueAtTime(Math.max(-1, Math.min(1, pan)), now);
      source.connect(gain).connect(panner).connect(this.mixer.bus(entry.bus));
      source.onended=()=>{source.disconnect();gain.disconnect();panner.disconnect();};
      source.start(now);
      return { source: "sample", endTime: now + sample.duration };
    }
    // 占位条目，或采样尚未加载 / 加载失败时回退到程序化合成
    const recipe = this.recipes[eventId];
    if (recipe === undefined) return null;
    let destination: AudioNode = this.mixer.bus(entry.bus);
    if (entry.gain !== undefined && entry.gain !== 1) {
      const trim = this.ctx.createGain();
      trim.gain.setValueAtTime(entry.gain, now);
      trim.connect(destination);
      destination = trim;
    }
    this.synth.trigger(eventId, destination, intensity, pan);
    return { source: "synth", endTime: now + (recipe.attackMs + recipe.decayMs + recipe.releaseMs)/1000 };
  }

  private pruneVoices(): void {
    const now = this.ctx.currentTime;
    this.voices = this.voices.filter((v) => v.endTime > now);
  }
}

/** 配方总时长（秒）= 起音 + 衰减 + 释音，与合成器的 `tEnd - now` 一致 */
export function recipeDurationSec(eventId: string): number {
  const r = PLACEHOLDER_SYNTH_RECIPES[eventId];
  if (r === undefined) return 0;
  const tAttack = r.attackMs / 1000;
  const tDecay = tAttack + r.decayMs / 1000;
  return tDecay + r.releaseMs / 1000;
}

function assetSources(manifest: AudioManifest): string[] {
  const out = new Set<string>();
  for (const entry of Object.values(manifest.events)) {
    if (entry.status === "asset") out.add(entry.src);
  }
  return [...out].sort();
}

function clamp01(v: number): number {
  return Number.isFinite(v)?Math.max(0, Math.min(1, v)):0;
}
