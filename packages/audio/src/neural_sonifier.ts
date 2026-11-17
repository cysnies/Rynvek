// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { nextBarBoundary, SCHEDULE_LOOKAHEAD_SEC } from "./adaptive_music.js";

/** 展示层映射参数；ringDensity是历史环占用率，不是瞬时发放率或相位相干度。 */
export interface NeuralSoundLevels { readonly ringDensity: number; readonly continuousLevel: number }

/** 两条低电平纯音分别可听化真实脉冲环密度与连续模型隐藏态。无虚构spike事件/随机触发器。 */
export class NeuralStateSonifier {
  private readonly ringOsc: OscillatorNode;
  private readonly continuousOsc: OscillatorNode;
  private readonly ringGain: GainNode;
  private readonly continuousGain: GainNode;
  private readonly origin: number;
  private stopped = false;
  private previous = "";
  constructor(private readonly ctx: AudioContext, destination: AudioNode) {
    this.origin = ctx.currentTime + SCHEDULE_LOOKAHEAD_SEC;
    this.ringGain = ctx.createGain(); this.continuousGain = ctx.createGain();
    this.ringGain.gain.setValueAtTime(0, ctx.currentTime); this.continuousGain.gain.setValueAtTime(0, ctx.currentTime);
    this.ringGain.connect(destination); this.continuousGain.connect(destination);
    this.ringOsc = ctx.createOscillator(); this.ringOsc.type = "sine"; this.ringOsc.frequency.setValueAtTime(220, ctx.currentTime);
    this.continuousOsc = ctx.createOscillator(); this.continuousOsc.type = "triangle"; this.continuousOsc.frequency.setValueAtTime(110, ctx.currentTime);
    this.ringOsc.connect(this.ringGain); this.continuousOsc.connect(this.continuousGain);
    this.ringOsc.start(this.origin); this.continuousOsc.start(this.origin);
  }
  update(levels: NeuralSoundLevels): void {
    if (this.stopped || this.ctx.state !== "running") return;
    const ring = level(levels.ringDensity), continuous = level(levels.continuousLevel);
    const key = `${ring}:${continuous}`; if (key === this.previous) return; this.previous = key;
    const now = this.ctx.currentTime, at = nextBarBoundary(now, this.origin);
    // 复用既有小节调度网格调整音高；电平短平滑跟随当前状态，不堆积逐spike音符。
    for (const [osc, freq] of [[this.ringOsc, 220 + ring * 440], [this.continuousOsc, 110 + continuous * 220]] as const) {
      osc.frequency.cancelScheduledValues(at); osc.frequency.setTargetAtTime(freq, at, .04);
    }
    for (const [gain, value] of [[this.ringGain, ring * .08], [this.continuousGain, continuous * .06]] as const) {
      gain.gain.cancelScheduledValues(now); gain.gain.setTargetAtTime(value, now, .02);
    }
  }
  stop(): void {
    if (this.stopped) return; this.stopped = true;
    for (const gain of [this.ringGain, this.continuousGain]) {
      gain.gain.cancelScheduledValues(this.ctx.currentTime); gain.gain.setValueAtTime(0, this.ctx.currentTime); gain.disconnect();
    }
    for (const osc of [this.ringOsc, this.continuousOsc]) { osc.stop(this.ctx.currentTime); osc.disconnect(); }
  }
}
function level(value: number): number { return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0; }
