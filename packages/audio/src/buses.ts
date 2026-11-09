// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 五大混音总线与静音循环。
//
// 总线名称取自规范 12.2 `SynthRecipe.bus` 的字面量联合类型；拓扑为
// 各总线 GainNode → master GainNode → ctx.destination。
// 规范未给出缺省电平与 `global.cycle_audio_mute`（动作 07）的循环语义，按最佳实践补全（ERRATA E-018）。

import type { SynthRecipe } from "./placeholder_synth.js";

/** 混音总线名（与规范字面量逐字一致） */
export type AudioBusName = SynthRecipe["bus"];

/** 五大总线，按固定顺序（UI → 世界 → 战斗 → 告警 → 神经声音化） */
export const AUDIO_BUS_NAMES: readonly AudioBusName[] = [
  "sfx_ui",
  "sfx_world",
  "sfx_combat",
  "voice_alert",
  "neural_sonify",
] as const;

/** 缺省总线电平（线性增益）：告警最突出，持续性的神经声音化最低，避免长时间聆听疲劳 */
export const DEFAULT_BUS_GAINS: Readonly<Record<AudioBusName, number>> = {
  sfx_ui: 0.8,
  sfx_world: 0.7,
  sfx_combat: 0.75,
  voice_alert: 1.0,
  neural_sonify: 0.5,
};

/**
 * 静音循环状态（`global.cycle_audio_mute`）：
 * `none` 全部发声 → `sfx` 仅保留 `voice_alert`（关键告警不应被一键静掉）→ `all` 全部静音 → `none`。
 */
export type MuteState = "none" | "sfx" | "all";

export const MUTE_CYCLE: readonly MuteState[] = ["none", "sfx", "all"] as const;

/** 电平变化的平滑时间常数（秒），避免直接跳变产生爆音 */
export const GAIN_SMOOTHING_SEC = 0.015;

export function isAudioBusName(value: string): value is AudioBusName {
  return (AUDIO_BUS_NAMES as readonly string[]).includes(value);
}

/** 混音器：持有五条总线与主输出，负责电平、静音与平滑过渡 */
export class AudioMixer {
  private readonly ctx: AudioContext;
  private readonly master: GainNode;
  private readonly buses = new Map<AudioBusName, GainNode>();
  private readonly levels = new Map<AudioBusName, number>();
  private masterLevel: number;
  private mute: MuteState = "none";

  public constructor(ctx: AudioContext, options: { masterGain?: number; busGains?: Partial<Record<AudioBusName, number>> } = {}) {
    this.ctx = ctx;
    this.masterLevel = clampGain(options.masterGain ?? 1);
    this.master = ctx.createGain();
    this.master.gain.setValueAtTime(this.masterLevel, ctx.currentTime);
    this.master.connect(ctx.destination);
    for (const name of AUDIO_BUS_NAMES) {
      const bus = ctx.createGain();
      const level = clampGain(options.busGains?.[name] ?? DEFAULT_BUS_GAINS[name]);
      this.levels.set(name, level);
      bus.gain.setValueAtTime(level, ctx.currentTime);
      bus.connect(this.master);
      this.buses.set(name, bus);
    }
  }

  /** 取总线输入节点（合成器与采样播放器的连接目标） */
  public bus(name: AudioBusName): GainNode {
    const node = this.buses.get(name);
    if (node === undefined) throw new RangeError(`未知混音总线：${String(name)}`);
    return node;
  }

  public get muteState(): MuteState {
    return this.mute;
  }

  /** 用户设定的总线电平（不受静音影响） */
  public busLevel(name: AudioBusName): number {
    return this.levels.get(name) ?? 0;
  }

  public get masterGain(): number {
    return this.masterLevel;
  }

  /** 总线在当前静音状态下是否发声 */
  public isAudible(name: AudioBusName): boolean {
    if (this.mute === "all") return false;
    if (this.mute === "sfx") return name === "voice_alert";
    return true;
  }

  public setBusGain(name: AudioBusName, gain: number): void {
    this.levels.set(name, clampGain(gain));
    this.applyBus(name);
  }

  public setMasterGain(gain: number): void {
    this.masterLevel = clampGain(gain);
    this.smooth(this.master.gain, this.masterLevel);
  }

  public setMuteState(state: MuteState): void {
    this.mute = state;
    for (const name of AUDIO_BUS_NAMES) this.applyBus(name);
  }

  /** 执行一次静音循环并返回新状态 */
  public cycleMute(): MuteState {
    const idx = MUTE_CYCLE.indexOf(this.mute);
    const next = MUTE_CYCLE[(idx + 1) % MUTE_CYCLE.length] ?? "none";
    this.setMuteState(next);
    return next;
  }

  private applyBus(name: AudioBusName): void {
    this.smooth(this.bus(name).gain, this.isAudible(name) ? this.busLevel(name) : 0);
  }

  private smooth(param: AudioParam, value: number): void {
    const now = this.ctx.currentTime;
    param.cancelScheduledValues(now);
    param.setTargetAtTime(value, now, GAIN_SMOOTHING_SEC);
  }
}

function clampGain(g: number): number {
  if (!Number.isFinite(g)) return 0;
  return Math.max(0, Math.min(1, g));
}
