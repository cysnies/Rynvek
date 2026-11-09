// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { AUDIO_BUS_NAMES, PLACEHOLDER_SYNTH_RECIPES, ProceduralPlaceholderSynth, recipeDurationSec } from "../src/index.js";
import {
  FakeAudioBufferSourceNode,
  FakeAudioContext,
  FakeBiquadFilterNode,
  FakeGainNode,
  FakeOscillatorNode,
  FakeStereoPannerNode,
} from "../src/testing.js";

const SPEC = new URL("../../../docs/Rynvek_Development_Specification_v1.0.md", import.meta.url);
const SOURCE = new URL("../src/placeholder_synth.ts", import.meta.url);

/** 去掉注释与空行，只比较代码本体 */
function codeOnly(text: string): string {
  return text
    .split("\n")
    .map((l) => l.replace(/\s*\/\/.*$/, "").replace(/\/\*\*.*\*\//, ""))
    .filter((l) => l.trim() !== "")
    .join("\n");
}

function setup(now = 1.0) {
  const ctx = new FakeAudioContext(48_000);
  ctx.currentTime = now;
  const synth = new ProceduralPlaceholderSynth(ctx.asAudioContext());
  const out = ctx.createGain();
  return { ctx, synth, out };
}

describe("规范 12.2 源码逐字一致", () => {
  it("去除注释后与规范代码块完全相同", () => {
    const spec = readFileSync(SPEC, "utf8");
    const start = spec.indexOf("// packages/audio/src/placeholder_synth.ts");
    const end = spec.indexOf("```", start);
    expect(start).toBeGreaterThan(0);
    expect(codeOnly(readFileSync(SOURCE, "utf8"))).toBe(codeOnly(spec.slice(start, end)));
  });

  it("24 类配方，总线全部属于五大总线", () => {
    const ids = Object.keys(PLACEHOLDER_SYNTH_RECIPES);
    expect(ids).toHaveLength(24);
    expect(new Set(Object.values(PLACEHOLDER_SYNTH_RECIPES).map((r) => r.bus))).toEqual(new Set(AUDIO_BUS_NAMES));
    for (const id of ids) expect(id).toMatch(/^(ui|building|unit|combat|neural|field|alert)\.[a-z_]+$/);
  });
});

describe("ProceduralPlaceholderSynth 节点图与包络", () => {
  it("ui.confirm：振荡器 → 滤波 → 包络 → 声像 → 目标，参数精确", () => {
    const { ctx, synth, out } = setup(1.0);
    synth.trigger("ui.confirm", out as unknown as AudioNode, 1.0, -0.25);

    const [osc] = ctx.nodesOfType<FakeOscillatorNode>("oscillator");
    const [filter] = ctx.nodesOfType<FakeBiquadFilterNode>("biquadFilter");
    const [panner] = ctx.nodesOfType<FakeStereoPannerNode>("stereoPanner");
    const env = ctx.nodesOfType<FakeGainNode>("gain").find((g) => g !== out);
    expect(osc && filter && panner && env).toBeTruthy();
    if (!osc || !filter || !panner || !env) return;

    expect(osc.outputs).toEqual([filter]);
    expect(filter.outputs).toEqual([env]);
    expect(env.outputs).toEqual([panner]);
    expect(panner.outputs).toEqual([out]);
    expect(ctx.nodesOfType("bufferSource")).toHaveLength(0); // noiseMix = 0

    expect(filter.type).toBe("lowpass");
    expect(filter.frequency.events).toEqual([{ kind: "set", value: 3200, time: 1.0 }]);
    expect(filter.Q.events).toEqual([{ kind: "set", value: 1.0, time: 1.0 }]);
    expect(panner.pan.events).toEqual([{ kind: "set", value: -0.25, time: 1.0 }]);

    const tAttack = 1.0 + 3 / 1000;
    const tDecay = tAttack + 80 / 1000;
    const tEnd = tDecay + 60 / 1000;
    expect(env.gain.events).toEqual([
      { kind: "set", value: 0.0001, time: 1.0 },
      { kind: "linear", value: 0.35, time: tAttack },
      { kind: "exponential", value: 0.35 * 0.2, time: tDecay },
      { kind: "exponential", value: 0.0001, time: tEnd },
    ]);
    expect(osc.type).toBe("triangle");
    expect(osc.frequency.events).toEqual([
      { kind: "set", value: 587, time: 1.0 },
      { kind: "exponential", value: 880, time: tEnd },
    ]);
    expect(osc.startTime).toBe(1.0);
    expect(osc.stopTime).toBe(tEnd);

    // 按 Web Audio 插值公式求值
    expect(env.gain.valueAt(1.0)).toBeCloseTo(0.0001, 12);
    expect(env.gain.valueAt(tAttack)).toBeCloseTo(0.35, 12);
    expect(env.gain.valueAt(1.0 + 1.5 / 1000)).toBeCloseTo((0.0001 + 0.35) / 2, 12);
    expect(env.gain.valueAt(tDecay)).toBeCloseTo(0.07, 12);
    expect(env.gain.valueAt((tAttack + tDecay) / 2)).toBeCloseTo(Math.sqrt(0.35 * 0.07), 12);
    expect(env.gain.valueAt(tEnd + 1)).toBeCloseTo(0.0001, 12);
  });

  it("sustainLevel = 0 时衰减终点取 0.001 下限；强度与声像被夹紧", () => {
    const { ctx, synth, out } = setup(0);
    synth.trigger("ui.hover", out as unknown as AudioNode, 5, 9);
    const env = ctx.nodesOfType<FakeGainNode>("gain").find((g) => g !== out);
    expect(env?.gain.events[1]).toMatchObject({ kind: "linear", value: 0.35 });
    expect(env?.gain.events[2]).toMatchObject({ kind: "exponential", value: 0.001 });
    expect(ctx.nodesOfType<FakeStereoPannerNode>("stereoPanner")[0]?.pan.events[0]?.value).toBe(1);

    const quiet = setup(0);
    quiet.synth.trigger("ui.hover", quiet.out as unknown as AudioNode, 0, -9);
    const qEnv = quiet.ctx.nodesOfType<FakeGainNode>("gain").find((g) => g !== quiet.out);
    expect(qEnv?.gain.events[1]?.value).toBeCloseTo(0.01 * 0.35, 15);
    expect(quiet.ctx.nodesOfType<FakeStereoPannerNode>("stereoPanner")[0]?.pan.events[0]?.value).toBe(-1);
  });

  it("noiseMix > 0 时并入共享粉红噪声，噪声与振荡器同时启停", () => {
    const { ctx, synth, out } = setup(2.0);
    synth.trigger("building.destroyed", out as unknown as AudioNode);
    synth.trigger("building.destroyed", out as unknown as AudioNode);
    const [n1, n2] = ctx.nodesOfType<FakeAudioBufferSourceNode>("bufferSource");
    const [filter] = ctx.nodesOfType<FakeBiquadFilterNode>("biquadFilter");
    expect(n1?.buffer).not.toBeNull();
    expect(n1?.buffer).toBe(n2?.buffer); // 共享同一缓冲
    const noiseGain = n1?.outputs[0] as FakeGainNode;
    expect(noiseGain.gain.events).toEqual([{ kind: "set", value: 0.65, time: 2.0 }]);
    expect(noiseGain.outputs).toEqual([filter]);
    const tEnd = 2.0 + recipeDurationSec("building.destroyed");
    expect(n1?.startTime).toBe(2.0);
    expect(n1?.stopTime).toBeCloseTo(tEnd, 12);
  });

  it("全部 24 类配方都能触发，指数斜坡目标均为正，时长与配方一致", () => {
    for (const [id, r] of Object.entries(PLACEHOLDER_SYNTH_RECIPES)) {
      const { ctx, synth, out } = setup(0.5);
      expect(() => synth.trigger(id, out as unknown as AudioNode)).not.toThrow();
      const [osc] = ctx.nodesOfType<FakeOscillatorNode>("oscillator");
      expect(osc?.type).toBe(r.oscType);
      expect(osc?.stopTime).toBeCloseTo(0.5 + (r.attackMs + r.decayMs + r.releaseMs) / 1000, 12);
      expect(ctx.nodesOfType("bufferSource")).toHaveLength(r.noiseMix > 0 ? 1 : 0);
    }
  });

  it("上下文未运行或事件未知时不创建任何节点", () => {
    const ctx = new FakeAudioContext(48_000, "suspended");
    const synth = new ProceduralPlaceholderSynth(ctx.asAudioContext());
    const before = ctx.nodes.length;
    synth.trigger("ui.click", ctx.destination as unknown as AudioNode);
    expect(ctx.nodes.length).toBe(before);
    ctx.state = "running";
    synth.trigger("ui.does_not_exist", ctx.destination as unknown as AudioNode);
    expect(ctx.nodes.length).toBe(before);
  });
});

describe("确定性粉红噪声", () => {
  /** 独立参考实现 */
  function reference(n: number): number[] {
    const out: number[] = [];
    let b0 = 0, b1 = 0, b2 = 0;
    for (let i = 0; i < n; i++) {
      const white = ((i * 1664525 + 1013904223) >>> 0) / 0xffffffff * 2 - 1;
      b0 = 0.99765 * b0 + white * 0.099046;
      b1 = 0.963 * b1 + white * 0.2965164;
      b2 = 0.57 * b2 + white * 1.0526913;
      out.push(Math.fround((b0 + b1 + b2 + white * 0.1848) * 0.12));
    }
    return out;
  }

  function noiseOf(sampleRate: number): Float32Array {
    const ctx = new FakeAudioContext(sampleRate);
    const synth = new ProceduralPlaceholderSynth(ctx.asAudioContext());
    synth.trigger("ui.click", ctx.destination as unknown as AudioNode);
    const src = ctx.nodesOfType<FakeAudioBufferSourceNode>("bufferSource")[0];
    if (!src?.buffer) throw new Error("缺少噪声缓冲");
    return src.buffer.getChannelData(0);
  }

  it("1 秒单声道缓冲，逐样本与参考实现一致", () => {
    const data = noiseOf(48_000);
    expect(data.length).toBe(48_000);
    const ref = reference(48_000);
    for (let i = 0; i < data.length; i++) {
      if (data[i] !== ref[i]) throw new Error(`样本 ${i} 不一致：${data[i]} ≠ ${ref[i]}`);
    }
  });

  it("首样本固定值、跨上下文可复现、峰值为已知常数", () => {
    const white0 = (1013904223 >>> 0) / 0xffffffff * 2 - 1;
    const first = (white0 * (0.099046 + 0.2965164 + 1.0526913 + 0.1848)) * 0.12;
    const a = noiseOf(44_100);
    expect(a.length).toBe(44_100);
    expect(a[0]).toBe(Math.fround(first));
    expect(Array.from(noiseOf(44_100))).toEqual(Array.from(a));
    let peak = 0;
    for (const v of a) peak = Math.max(peak, Math.abs(v));
    // 规范的“白噪声”以下标为种子，相邻样本相差 1664525 / 2^32，实为周期约 2580 样本的锯齿波，
    // 经三极点漏积分后峰值超过 1（见 ERRATA E-018）。源码逐字照搬，这里锁定其确定性峰值。
    expect(peak).toBeCloseTo(4.6924014, 6);
  });
});
