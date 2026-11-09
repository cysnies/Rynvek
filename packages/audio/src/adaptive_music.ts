// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 6 轨自适应音乐（项目文档 6.2，D617–D637）：120 BPM，Stem A–F 同步循环，
// 随世界状态“安静观察 → 基地建设 → 规模生产 → 局部交火 → 决战告警”在小节边界叠加或抽离音轨。
//
// 规范未给出状态与音轨的对应关系、拍号与淡变时长，按最佳实践补全（ERRATA E-018）：
// 4/4 拍（一小节 2 秒）；状态逐级累加音轨；每次变化在下一小节线开始、用一拍（0.5 秒）线性淡变。
// 程序化占位阶段没有分轨音频文件，控制器照常调度各轨电平，接入 .ogg 后无需改动调度逻辑。

/** 速度（每分钟拍数） */
export const MUSIC_BPM = 120;
/** 每小节拍数（4/4） */
export const BEATS_PER_BAR = 4;
/** 一拍时长（秒） */
export const BEAT_DURATION_SEC = 60 / MUSIC_BPM;
/** 一小节时长（秒） */
export const BAR_DURATION_SEC = BEAT_DURATION_SEC * BEATS_PER_BAR;
/** 音轨淡入 / 淡出时长：一拍 */
export const STEM_FADE_SEC = BEAT_DURATION_SEC;
/** 调度提前量：距下一小节线不足该值时顺延一小节，保证参数自动化在音频线程到达前写入 */
export const SCHEDULE_LOOKAHEAD_SEC = 0.05;

export type MusicStemId = "A" | "B" | "C" | "D" | "E" | "F";

/** 六条分轨（名称逐字取自项目文档 6.2） */
export const MUSIC_STEMS: readonly { readonly id: MusicStemId; readonly name: string }[] = [
  { id: "A", name: "Ambient Pad" },
  { id: "B", name: "Sub Bass" },
  { id: "C", name: "Industrial Percussion" },
  { id: "D", name: "Logic Arpeggio" },
  { id: "E", name: "Combat Synth" },
  { id: "F", name: "High Intensity Lead" },
] as const;

/** 五个世界音乐状态，按强度递增 */
export type MusicState = "quiet_observation" | "base_building" | "mass_production" | "skirmish" | "final_alert";

export const MUSIC_STATES: readonly MusicState[] = [
  "quiet_observation",
  "base_building",
  "mass_production",
  "skirmish",
  "final_alert",
] as const;

/** 各状态启用的音轨（逐级累加，越激烈层次越厚） */
export const MUSIC_STATE_STEMS: Readonly<Record<MusicState, readonly MusicStemId[]>> = {
  quiet_observation: ["A"],
  base_building: ["A", "B"],
  mass_production: ["A", "B", "C", "D"],
  skirmish: ["A", "B", "C", "D", "E"],
  final_alert: ["A", "B", "C", "D", "E", "F"],
};

/** 各音轨满电平（线性增益），低频与打击乐略低以留出音效空间 */
export const STEM_LEVELS: Readonly<Record<MusicStemId, number>> = {
  A: 0.6,
  B: 0.5,
  C: 0.45,
  D: 0.5,
  E: 0.55,
  F: 0.5,
};

/** 不早于 `now + lookahead` 的最近小节线（以 `origin` 为第 0 小节起点） */
export function nextBarBoundary(now: number, origin: number, lookahead = SCHEDULE_LOOKAHEAD_SEC): number {
  const earliest = now + lookahead;
  if (earliest <= origin) return origin;
  // 先取整再乘，避免浮点累计误差；1e-9 容差让恰好落在小节线上的时刻不被顺延
  const bars = Math.ceil((earliest - origin) / BAR_DURATION_SEC - 1e-9);
  return origin + bars * BAR_DURATION_SEC;
}

/** 单条音轨的一次电平变化 */
export interface StemRamp {
  readonly stem: MusicStemId;
  readonly from: number;
  readonly to: number;
  /** 淡变开始（小节线） */
  readonly startTime: number;
  /** 淡变结束 */
  readonly endTime: number;
}

/** 计算状态切换需要的音轨电平变化（只包含电平改变的音轨） */
export function planStemTransition(from: MusicState | null, to: MusicState, at: number): StemRamp[] {
  const before = new Set(from === null ? [] : MUSIC_STATE_STEMS[from]);
  const after = new Set(MUSIC_STATE_STEMS[to]);
  const ramps: StemRamp[] = [];
  for (const { id } of MUSIC_STEMS) {
    const a = before.has(id) ? STEM_LEVELS[id] : 0;
    const b = after.has(id) ? STEM_LEVELS[id] : 0;
    if (a !== b) ramps.push({ stem: id, from: a, to: b, startTime: at, endTime: at + STEM_FADE_SEC });
  }
  return ramps;
}

/** 自适应音乐控制器：每条音轨一个 GainNode，汇入调用方给定的目标节点 */
export class AdaptiveMusicController {
  private readonly ctx: AudioContext;
  private readonly stemGains = new Map<MusicStemId, GainNode>();
  private readonly sources: AudioBufferSourceNode[] = [];
  private origin: number | null = null;
  private target: MusicState | null = null;

  public constructor(ctx: AudioContext, destination: AudioNode) {
    this.ctx = ctx;
    for (const { id } of MUSIC_STEMS) {
      const g = ctx.createGain();
      g.gain.setValueAtTime(0, ctx.currentTime);
      g.connect(destination);
      this.stemGains.set(id, g);
    }
  }

  /** 节拍网格起点；未启动时为 null */
  public get barOrigin(): number | null {
    return this.origin;
  }

  /** 最近一次请求的目标状态 */
  public get state(): MusicState | null {
    return this.target;
  }

  public stemGain(id: MusicStemId): GainNode {
    const g = this.stemGains.get(id);
    if (g === undefined) throw new RangeError(`未知音轨：${id}`);
    return g;
  }

  /**
   * 启动节拍网格（默认从下一个调度提前量之后开始）。
   * 提供分轨缓冲时，所有音轨在同一时刻循环起播，保证六轨采样级同步。
   */
  public start(buffers: Partial<Record<MusicStemId, AudioBuffer>> = {}, at?: number): number {
    if (this.origin !== null) return this.origin;
    const origin = at ?? this.ctx.currentTime + SCHEDULE_LOOKAHEAD_SEC;
    this.origin = origin;
    for (const { id } of MUSIC_STEMS) {
      const buffer = buffers[id];
      if (buffer === undefined) continue;
      const src = this.ctx.createBufferSource();
      src.buffer = buffer;
      src.loop = true;
      src.connect(this.stemGain(id));
      src.start(origin);
      this.sources.push(src);
    }
    return origin;
  }

  /** 请求切换状态：在下一小节线开始淡变；返回实际调度的电平变化 */
  public setState(next: MusicState): StemRamp[] {
    const origin = this.origin ?? this.start();
    if (next === this.target) return [];
    const at = nextBarBoundary(this.ctx.currentTime, origin);
    const ramps = planStemTransition(this.target, next, at);
    for (const r of ramps) {
      const param = this.stemGain(r.stem).gain;
      // 若同一小节内连续切换，撤销尚未生效的上一份计划，以本次为准
      param.cancelScheduledValues(at);
      param.setValueAtTime(r.from, r.startTime);
      param.linearRampToValueAtTime(r.to, r.endTime);
    }
    this.target = next;
    return ramps;
  }

  /** 停止全部分轨并释放节点 */
  public stop(): void {
    const now = this.ctx.currentTime;
    for (const src of this.sources) src.stop(now);
    this.sources.length = 0;
    for (const g of this.stemGains.values()) {
      g.gain.cancelScheduledValues(now);
      g.gain.setValueAtTime(0, now);
    }
    this.origin = null;
    this.target = null;
  }
}
