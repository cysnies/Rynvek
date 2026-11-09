// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import {
  AUDIO_MANIFEST_SCHEMA,
  AdaptiveMusicController,
  AudioDirector,
  BAR_DURATION_SEC,
  BUS_VOICE_LIMITS,
  MUSIC_STATES,
  MUSIC_STATE_STEMS,
  MUSIC_STEMS,
  STEM_FADE_SEC,
  STEM_LEVELS,
  nextBarBoundary,
  planStemTransition,
  recipeDurationSec,
  type AudioManifest,
} from "../src/index.js";
import { FakeAudioBufferSourceNode, FakeAudioContext, FakeGainNode } from "../src/testing.js";

function director(manifest?: AudioManifest, loader?: (src: string) => Promise<AudioBuffer>) {
  const ctx = new FakeAudioContext();
  const opts: ConstructorParameters<typeof AudioDirector>[1] = {};
  if (manifest) opts.manifest = manifest;
  if (loader) opts.loader = loader;
  return { ctx, d: new AudioDirector(ctx.asAudioContext(), opts) };
}

describe("AudioDirector", () => {
  it("占位事件经合成器路由到对应总线", () => {
    const { ctx, d } = director();
    const r = d.play("combat.weapon_pulse", { intensity: 0.5 });
    expect(r).toEqual({ played: true, source: "synth", endTime: recipeDurationSec("combat.weapon_pulse") });
    const panner = ctx.nodesOfType("stereoPanner")[0];
    expect(panner?.outputs[0]).toBe(d.mixer.bus("sfx_combat"));
  });

  it("冷却、单事件发声上限与总线发声上限", () => {
    const { ctx, d } = director();
    expect(d.play("ui.click").played).toBe(true);
    expect(d.play("ui.click")).toEqual({ played: false, reason: "cooldown" });
    ctx.currentTime = 0.026;
    expect(d.play("ui.click").played).toBe(true);

    // 告警总线最多 2 个并发
    const alerts = director();
    expect(alerts.d.play("alert.nexus_under_attack").played).toBe(true);
    expect(alerts.d.play("ui.error").played).toBe(true);
    alerts.ctx.currentTime = 0.1;
    expect(BUS_VOICE_LIMITS.voice_alert).toBe(2);
    expect(alerts.d.activeVoiceCount("voice_alert")).toBe(2);
    alerts.ctx.currentTime = 2;
    expect(alerts.d.activeVoiceCount("voice_alert")).toBe(0);

    // 同一事件缺省最多 4 个并发
    const many = director();
    let played = 0;
    for (let i = 0; i < 10; i++) {
      many.ctx.currentTime = i * 0.021;
      if (many.d.play("building.destroyed").played) played++;
    }
    expect(played).toBe(4);
  });

  it("静音、未运行与未知事件", async () => {
    const { ctx, d } = director();
    d.mixer.setMuteState("sfx");
    expect(d.play("ui.click")).toEqual({ played: false, reason: "muted" });
    expect(d.play("ui.error").played).toBe(true);
    expect(d.play("nope.nope")).toEqual({ played: false, reason: "unknown_event" });
    ctx.state = "suspended";
    expect(d.play("alert.nexus_under_attack")).toEqual({ played: false, reason: "context_not_running" });
    await d.unlock();
    expect(ctx.state).toBe("running");
  });

  it(".ogg 资产加载后播放采样，失败时回退到合成器", async () => {
    const manifest: AudioManifest = {
      schema: AUDIO_MANIFEST_SCHEMA,
      events: {
        "ui.click": { status: "asset", bus: "sfx_ui", src: "click.ogg", license: "CC0-1.0", gain: 0.5 },
        "ui.hover": { status: "asset", bus: "sfx_ui", src: "missing.ogg", license: "CC0-1.0" },
        "custom.only_file": { status: "asset", bus: "sfx_world", src: "missing.ogg", license: "CC0-1.0" },
      },
    };
    const { ctx, d } = director(manifest, async (src) => {
      if (src !== "click.ogg") throw new Error("404");
      return ctx.createBuffer(1, 24_000, 48_000) as unknown as AudioBuffer;
    });
    expect(await d.preload()).toEqual(["missing.ogg"]);
    expect(d.play("ui.click", { intensity: 0.8 })).toEqual({ played: true, source: "sample", endTime: 0.5 });
    const src = ctx.nodesOfType<FakeAudioBufferSourceNode>("bufferSource").at(-1);
    expect((src?.outputs[0] as FakeGainNode).gain.events[0]?.value).toBeCloseTo(0.4, 12);
    expect(d.play("ui.hover")).toMatchObject({ played: true, source: "synth" });
    expect(d.play("custom.only_file")).toEqual({ played: false, reason: "unknown_event" });

    d.replaceManifest({ schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "placeholder", bus: "sfx_ui" } } });
    ctx.currentTime = 1;
    expect(d.play("ui.click")).toMatchObject({ source: "synth" });
  });
});

describe("6 轨自适应音乐", () => {
  it("120 BPM、4/4：一拍 0.5 秒、一小节 2 秒；六轨名称", () => {
    expect(BAR_DURATION_SEC).toBe(2);
    expect(STEM_FADE_SEC).toBe(0.5);
    expect(MUSIC_STEMS.map((s) => `${s.id}:${s.name}`)).toEqual([
      "A:Ambient Pad",
      "B:Sub Bass",
      "C:Industrial Percussion",
      "D:Logic Arpeggio",
      "E:Combat Synth",
      "F:High Intensity Lead",
    ]);
    // 状态逐级累加
    for (let i = 1; i < MUSIC_STATES.length; i++) {
      const prev = MUSIC_STATE_STEMS[MUSIC_STATES[i - 1]!];
      const cur = MUSIC_STATE_STEMS[MUSIC_STATES[i]!];
      expect(prev.every((s) => cur.includes(s))).toBe(true);
      expect(cur.length).toBeGreaterThan(prev.length);
    }
  });

  it("nextBarBoundary 对齐小节线并留出提前量", () => {
    expect(nextBarBoundary(0, 1)).toBe(1);
    expect(nextBarBoundary(0.5, 0)).toBe(2);
    expect(nextBarBoundary(1.95, 0)).toBe(2);
    expect(nextBarBoundary(1.96, 0)).toBe(4);
    expect(nextBarBoundary(3.95, 0, 0)).toBe(4);
    expect(nextBarBoundary(2000.3, 0.25)).toBe(2002.25);
  });

  it("planStemTransition 只包含电平变化的音轨", () => {
    expect(planStemTransition(null, "quiet_observation", 4)).toEqual([
      { stem: "A", from: 0, to: STEM_LEVELS.A, startTime: 4, endTime: 4.5 },
    ]);
    expect(planStemTransition("final_alert", "base_building", 6).map((r) => [r.stem, r.to])).toEqual([
      ["C", 0],
      ["D", 0],
      ["E", 0],
      ["F", 0],
    ]);
    expect(planStemTransition("skirmish", "skirmish", 0)).toEqual([]);
  });

  it("控制器在小节线调度淡变，分轨缓冲同时循环起播", () => {
    const ctx = new FakeAudioContext();
    const out = ctx.createGain();
    const music = new AdaptiveMusicController(ctx.asAudioContext(), out as unknown as AudioNode);
    const buf = ctx.createBuffer(2, 96_000 * 4, 48_000) as unknown as AudioBuffer;
    const origin = music.start({ A: buf, B: buf, F: buf }, 0.1);
    const sources = ctx.nodesOfType<FakeAudioBufferSourceNode>("bufferSource");
    expect(sources.map((s) => [s.startTime, s.loop])).toEqual([[0.1, true], [0.1, true], [0.1, true]]);
    expect(origin).toBe(0.1);

    ctx.currentTime = 0.7;
    const ramps = music.setState("mass_production");
    expect(ramps.every((r) => r.startTime === 2.1)).toBe(true);
    const c = music.stemGain("C") as unknown as FakeGainNode;
    expect(c.gain.valueAt(2.0)).toBe(0);
    expect(c.gain.valueAt(2.35)).toBeCloseTo(STEM_LEVELS.C / 2, 12);
    expect(c.gain.valueAt(3)).toBeCloseTo(STEM_LEVELS.C, 12);
    expect((music.stemGain("E") as unknown as FakeGainNode).gain.valueAt(3)).toBe(0);

    // 同一小节内改变主意：以最后一次为准
    ctx.currentTime = 1.0;
    music.setState("quiet_observation");
    expect(c.gain.valueAt(3)).toBe(0);
    expect((music.stemGain("A") as unknown as FakeGainNode).gain.valueAt(3)).toBeCloseTo(STEM_LEVELS.A, 12);
    expect(music.state).toBe("quiet_observation");
    expect(music.setState("quiet_observation")).toEqual([]);

    music.stop();
    expect(sources.every((s) => s.stopTime === 1.0)).toBe(true);
    expect(music.barOrigin).toBeNull();
  });
});
