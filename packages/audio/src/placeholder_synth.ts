// packages/audio/src/placeholder_synth.ts
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 规范 12.2 冻结源码：24 类语义占位音效的 Web Audio 程序化合成器（零外部音频文件依赖）。
// 缺省配方与算法保持规范原值；私有音效制品可显式提供经过sound-bank限界的独立配方映射。

/** 单条合成配方 */
export interface SynthRecipe {
  /** 目标混音总线 */
  readonly bus: "sfx_ui" | "sfx_world" | "sfx_combat" | "voice_alert" | "neural_sonify";
  /** 振荡器波形 */
  readonly oscType: OscillatorType;
  /** 起始频率（Hz） */
  readonly freqStart: number;
  /** 结束频率（Hz），在整个包络时长内指数滑音 */
  readonly freqEnd: number;
  /** 起音时长（毫秒） */
  readonly attackMs: number;
  /** 衰减时长（毫秒） */
  readonly decayMs: number;
  /** 衰减终点相对峰值的电平 */
  readonly sustainLevel: number;
  /** 释音时长（毫秒） */
  readonly releaseMs: number;
  /** 双二阶滤波器类型 */
  readonly filterType: BiquadFilterType;
  /** 滤波器截止 / 中心频率（Hz） */
  readonly filterFreq: number;
  /** 滤波器品质因数 */
  readonly filterQ: number;
  readonly noiseMix: number; // 粉红噪声混合比例 [0.0, 1.0]
}

/** 24 类语义音效配方表 */
export const PLACEHOLDER_SYNTH_RECIPES: Readonly<Record<string, SynthRecipe>> = {
  "ui.hover":                  { bus: "sfx_ui",      oscType: "sine",     freqStart: 880,  freqEnd: 920,  attackMs: 2,  decayMs: 18,  sustainLevel: 0.0,  releaseMs: 10,  filterType: "highpass", filterFreq: 600,  filterQ: 0.7, noiseMix: 0.0  },
  "ui.click":                  { bus: "sfx_ui",      oscType: "triangle", freqStart: 1320, freqEnd: 440,  attackMs: 1,  decayMs: 35,  sustainLevel: 0.0,  releaseMs: 14,  filterType: "bandpass", filterFreq: 1200, filterQ: 1.5, noiseMix: 0.1  },
  "ui.confirm":                { bus: "sfx_ui",      oscType: "triangle", freqStart: 587,  freqEnd: 880,  attackMs: 3,  decayMs: 80,  sustainLevel: 0.2,  releaseMs: 60,  filterType: "lowpass",  filterFreq: 3200, filterQ: 1.0, noiseMix: 0.0  },
  "ui.cancel":                 { bus: "sfx_ui",      oscType: "sawtooth", freqStart: 440,  freqEnd: 330,  attackMs: 3,  decayMs: 70,  sustainLevel: 0.1,  releaseMs: 50,  filterType: "lowpass",  filterFreq: 1800, filterQ: 1.2, noiseMix: 0.0  },
  "ui.error":                  { bus: "voice_alert", oscType: "sawtooth", freqStart: 165,  freqEnd: 147,  attackMs: 5,  decayMs: 140, sustainLevel: 0.4,  releaseMs: 95,  filterType: "lowpass",  filterFreq: 950,  filterQ: 2.5, noiseMix: 0.05 },
  "ui.panel_toggle":           { bus: "sfx_ui",      oscType: "sine",     freqStart: 300,  freqEnd: 750,  attackMs: 4,  decayMs: 45,  sustainLevel: 0.0,  releaseMs: 21,  filterType: "bandpass", filterFreq: 800,  filterQ: 1.8, noiseMix: 0.15 },
  "building.placed":           { bus: "sfx_world",   oscType: "square",   freqStart: 220,  freqEnd: 110,  attackMs: 2,  decayMs: 65,  sustainLevel: 0.15, releaseMs: 53,  filterType: "lowpass",  filterFreq: 1600, filterQ: 2.0, noiseMix: 0.2  },
  "building.completed":        { bus: "sfx_world",   oscType: "triangle", freqStart: 523,  freqEnd: 1046, attackMs: 4,  decayMs: 90,  sustainLevel: 0.25, releaseMs: 86,  filterType: "lowpass",  filterFreq: 4000, filterQ: 1.0, noiseMix: 0.0  },
  "building.destroyed":        { bus: "sfx_combat",  oscType: "sawtooth", freqStart: 140,  freqEnd: 38,   attackMs: 4,  decayMs: 220, sustainLevel: 0.2,  releaseMs: 226, filterType: "lowpass",  filterFreq: 450,  filterQ: 1.8, noiseMix: 0.65 },
  "building.extractor_cycle":  { bus: "sfx_world",   oscType: "sine",     freqStart: 180,  freqEnd: 240,  attackMs: 8,  decayMs: 50,  sustainLevel: 0.1,  releaseMs: 32,  filterType: "bandpass", filterFreq: 420,  filterQ: 3.0, noiseMix: 0.05 },
  "unit.spawned":              { bus: "sfx_world",   oscType: "sine",     freqStart: 330,  freqEnd: 660,  attackMs: 6,  decayMs: 75,  sustainLevel: 0.2,  releaseMs: 59,  filterType: "lowpass",  filterFreq: 2800, filterQ: 1.4, noiseMix: 0.0  },
  "unit.selected":             { bus: "sfx_ui",      oscType: "triangle", freqStart: 659,  freqEnd: 988,  attackMs: 2,  decayMs: 48,  sustainLevel: 0.0,  releaseMs: 30,  filterType: "bandpass", filterFreq: 1500, filterQ: 1.2, noiseMix: 0.0  },
  "unit.order_move":           { bus: "sfx_ui",      oscType: "sine",     freqStart: 523,  freqEnd: 659,  attackMs: 2,  decayMs: 38,  sustainLevel: 0.0,  releaseMs: 25,  filterType: "lowpass",  filterFreq: 2400, filterQ: 1.0, noiseMix: 0.0  },
  "unit.order_attack":         { bus: "sfx_ui",      oscType: "sawtooth", freqStart: 440,  freqEnd: 880,  attackMs: 2,  decayMs: 55,  sustainLevel: 0.15, releaseMs: 38,  filterType: "bandpass", filterFreq: 1800, filterQ: 2.2, noiseMix: 0.05 },
  "combat.weapon_pulse":       { bus: "sfx_combat",  oscType: "sawtooth", freqStart: 1480, freqEnd: 220,  attackMs: 1,  decayMs: 55,  sustainLevel: 0.0,  releaseMs: 24,  filterType: "bandpass", filterFreq: 900,  filterQ: 2.5, noiseMix: 0.15 },
  "combat.spore_launch":       { bus: "sfx_combat",  oscType: "sine",     freqStart: 260,  freqEnd: 520,  attackMs: 5,  decayMs: 70,  sustainLevel: 0.1,  releaseMs: 45,  filterType: "bandpass", filterFreq: 650,  filterQ: 4.5, noiseMix: 0.35 },
  "combat.impact_kinetic":     { bus: "sfx_combat",  oscType: "triangle", freqStart: 190,  freqEnd: 65,   attackMs: 1,  decayMs: 32,  sustainLevel: 0.0,  releaseMs: 22,  filterType: "lowpass",  filterFreq: 1400, filterQ: 1.5, noiseMix: 0.5  },
  "combat.shield_hit":         { bus: "sfx_combat",  oscType: "sine",     freqStart: 1760, freqEnd: 880,  attackMs: 2,  decayMs: 68,  sustainLevel: 0.1,  releaseMs: 40,  filterType: "highpass", filterFreq: 900,  filterQ: 2.8, noiseMix: 0.1  },
  "combat.explosion_large":    { bus: "sfx_combat",  oscType: "sine",     freqStart: 95,   freqEnd: 28,   attackMs: 3,  decayMs: 280, sustainLevel: 0.25, releaseMs: 317, filterType: "lowpass",  filterFreq: 320,  filterQ: 1.6, noiseMix: 0.8  },
  "neural.spike_cluster":      { bus: "neural_sonify", oscType: "sine",   freqStart: 659,  freqEnd: 784,  attackMs: 1,  decayMs: 28,  sustainLevel: 0.0,  releaseMs: 16,  filterType: "bandpass", filterFreq: 1600, filterQ: 3.5, noiseMix: 0.0  },
  "neural.sync_lock":          { bus: "neural_sonify", oscType: "triangle", freqStart: 262, freqEnd: 392, attackMs: 40, decayMs: 180, sustainLevel: 0.5,  releaseMs: 180, filterType: "lowpass",  filterFreq: 2200, filterQ: 1.2, noiseMix: 0.0  },
  "neural.mutation_epoch":     { bus: "sfx_world",   oscType: "sine",     freqStart: 440,  freqEnd: 1760, attackMs: 5,  decayMs: 120, sustainLevel: 0.3,  releaseMs: 115, filterType: "highpass", filterFreq: 500,  filterQ: 1.5, noiseMix: 0.0  },
  "field.beacon_pulse":        { bus: "sfx_world",   oscType: "triangle", freqStart: 110,  freqEnd: 220,  attackMs: 15, decayMs: 110, sustainLevel: 0.2,  releaseMs: 95,  filterType: "lowpass",  filterFreq: 900,  filterQ: 2.0, noiseMix: 0.0  },
  "alert.nexus_under_attack":  { bus: "voice_alert", oscType: "square",   freqStart: 784,  freqEnd: 587,  attackMs: 5,  decayMs: 180, sustainLevel: 0.7,  releaseMs: 115, filterType: "bandpass", filterFreq: 1400, filterQ: 2.0, noiseMix: 0.05 },
};

/** 程序化占位合成器：每次触发即时搭建 振荡器(+噪声) → 滤波 → 包络 → 声像 → 目标 的一次性节点图 */
export class ProceduralPlaceholderSynth {
  private readonly ctx: AudioContext;
  private readonly noiseBuffer: AudioBuffer;

  public constructor(ctx: AudioContext, private readonly recipes: Readonly<Record<string, SynthRecipe>> = PLACEHOLDER_SYNTH_RECIPES) {
    this.ctx = ctx;
    // 预先生成 1 秒确定性粉红噪声，所有触发共享
    this.noiseBuffer = this.createPinkNoiseBuffer(1.0);
  }

  /** 触发一条语义音效；未知事件或音频上下文未运行时静默忽略 */
  public trigger(eventId: string, destination: AudioNode, intensity = 1.0, pan = 0.0): void {
    const r = this.recipes[eventId];
    if (!r || this.ctx.state !== "running") return;

    // 包络时间轴：起音结束 → 衰减结束 → 释音结束
    const now = this.ctx.currentTime;
    const tAttack = now + r.attackMs / 1000;
    const tDecay = tAttack + r.decayMs / 1000;
    const tEnd = tDecay + r.releaseMs / 1000;

    // 音色滤波
    const filter = this.ctx.createBiquadFilter();
    filter.type = r.filterType;
    filter.frequency.setValueAtTime(r.filterFreq, now);
    filter.Q.setValueAtTime(r.filterQ, now);

    // 立体声声像（夹紧到 [-1, 1]）
    const panner = this.ctx.createStereoPanner();
    panner.pan.setValueAtTime(Math.max(-1, Math.min(1, pan)), now);

    // ADSR 包络：峰值 = 强度夹紧到 [0.01, 1.0] 后 × 0.35；指数段不能触零，故以 0.0001 / 0.001 为底
    const env = this.ctx.createGain();
    const peakGain = Math.max(0.01, Math.min(1.0, intensity)) * 0.35;
    env.gain.setValueAtTime(0.0001, now);
    env.gain.linearRampToValueAtTime(peakGain, tAttack);
    env.gain.exponentialRampToValueAtTime(Math.max(0.001, peakGain * r.sustainLevel), tDecay);
    env.gain.exponentialRampToValueAtTime(0.0001, tEnd);

    // 主振荡器：频率在整个时长内指数滑向结束频率（下限 20 Hz）
    const osc = this.ctx.createOscillator();
    osc.type = r.oscType;
    osc.frequency.setValueAtTime(r.freqStart, now);
    osc.frequency.exponentialRampToValueAtTime(Math.max(20, r.freqEnd), tEnd);

    osc.connect(filter);
    if (r.noiseMix > 0) {
      // 噪声分量：共享粉红噪声缓冲按混合比例并入滤波器
      const noise = this.ctx.createBufferSource();
      noise.buffer = this.noiseBuffer;
      const noiseGain = this.ctx.createGain();
      noiseGain.gain.setValueAtTime(r.noiseMix, now);
      noise.connect(noiseGain).connect(filter);
      noise.start(now);
      noise.stop(tEnd);
    }

    filter.connect(env).connect(panner).connect(destination);
    osc.start(now);
    osc.stop(tEnd);
  }

  /** 确定性粉红噪声：以下标为种子的 LCG 白噪声，经 Paul Kellet 三极点经济型滤波近似 1/f 频谱 */
  private createPinkNoiseBuffer(durationSec: number): AudioBuffer {
    const len = Math.floor(this.ctx.sampleRate * durationSec);
    const buf = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
    const out = buf.getChannelData(0);
    let b0 = 0, b1 = 0, b2 = 0;
    for (let i = 0; i < len; i++) {
      const white = ((i * 1664525 + 1013904223) >>> 0) / 0xFFFFFFFF * 2 - 1;
      b0 = 0.99765 * b0 + white * 0.0990460;
      b1 = 0.96300 * b1 + white * 0.2965164;
      b2 = 0.57000 * b2 + white * 1.0526913;
      out[i] = (b0 + b1 + b2 + white * 0.1848) * 0.12;
    }
    return buf;
  }
}
