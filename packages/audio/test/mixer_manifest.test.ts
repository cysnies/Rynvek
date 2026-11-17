// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  AUDIO_ASSET_LICENSES,
  AUDIO_ASSET_SRC_PATTERN,
  AUDIO_BUS_NAMES,
  AUDIO_EVENT_ID_PATTERN,
  AUDIO_MANIFEST_SCHEMA,
  AudioManifestError,
  AudioMixer,
  DEFAULT_BUS_GAINS,
  STANDARD_AUDIO_EVENT_IDS,
  createPlaceholderManifest,
  parseAudioManifest,
} from "../src/index.js";
import { FakeAudioContext, FakeGainNode } from "../src/testing.js";

describe("AudioMixer", () => {
  it("五条总线 → master → destination，缺省电平生效", () => {
    const ctx = new FakeAudioContext();
    const mixer = new AudioMixer(ctx.asAudioContext());
    const master = ctx.nodesOfType<FakeGainNode>("gain")[0];
    expect(master?.outputs).toEqual([ctx.destination]);
    expect(AUDIO_BUS_NAMES).toEqual(["sfx_ui", "sfx_world", "sfx_combat", "voice_alert", "neural_sonify"]);
    for (const name of AUDIO_BUS_NAMES) {
      const bus = mixer.bus(name) as unknown as FakeGainNode;
      expect(bus.outputs).toEqual([master]);
      expect(bus.gain.valueAt(0)).toBe(DEFAULT_BUS_GAINS[name]);
    }
  });

  it("静音循环 none → sfx（仅保留告警）→ all → none，电平平滑过渡", () => {
    const ctx = new FakeAudioContext();
    const mixer = new AudioMixer(ctx.asAudioContext());
    ctx.currentTime = 3;
    expect(mixer.cycleMute()).toBe("sfx");
    const ui = mixer.bus("sfx_ui") as unknown as FakeGainNode;
    const alert = mixer.bus("voice_alert") as unknown as FakeGainNode;
    expect(ui.gain.valueAt(3.9)).toBeCloseTo(0, 6);
    expect(alert.gain.valueAt(3.9)).toBeCloseTo(1, 6);
    expect(mixer.isAudible("voice_alert")).toBe(true);
    expect(mixer.isAudible("sfx_combat")).toBe(false);
    ctx.currentTime = 4;
    expect(mixer.cycleMute()).toBe("all");
    expect(alert.gain.valueAt(4.9)).toBeCloseTo(0, 6);
    ctx.currentTime = 5;
    expect(mixer.cycleMute()).toBe("none");
    expect(ui.gain.valueAt(6)).toBeCloseTo(DEFAULT_BUS_GAINS.sfx_ui, 6);
    // 指数逼近：一个时间常数后完成约 63%
    expect(ui.gain.valueAt(5 + 0.015)).toBeCloseTo(DEFAULT_BUS_GAINS.sfx_ui * (1 - Math.exp(-1)), 6);
  });

  it("电平夹紧到 [0, 1]，静音期间修改电平在解除后生效", () => {
    const ctx = new FakeAudioContext();
    const mixer = new AudioMixer(ctx.asAudioContext(), { busGains: { sfx_world: 3 } });
    expect(mixer.busLevel("sfx_world")).toBe(1);
    mixer.setMuteState("all");
    mixer.setBusGain("sfx_world", 0.4);
    const world = mixer.bus("sfx_world") as unknown as FakeGainNode;
    expect(world.gain.valueAt(10)).toBeCloseTo(0, 6);
    mixer.setMuteState("none");
    expect(world.gain.valueAt(10)).toBeCloseTo(0.4, 6);
    mixer.setMasterGain(Number.NaN);
    expect(mixer.masterGain).toBe(0);
    expect(() => mixer.bus("music" as never)).toThrow(RangeError);
  });
});

describe("AudioManifest", () => {
  it("缺省清单：24 条全部为 placeholder，总线与配方一致，可往返解析", () => {
    const m = createPlaceholderManifest();
    expect(m.schema).toBe(AUDIO_MANIFEST_SCHEMA);
    expect(Object.keys(m.events)).toEqual(STANDARD_AUDIO_EVENT_IDS);
    expect(Object.values(m.events).every((e) => e.status === "placeholder")).toBe(true);
    expect(parseAudioManifest(JSON.parse(JSON.stringify(m)))).toEqual(m);
  });

  it("把占位替换为 .ogg 资产", () => {
    const m = parseAudioManifest({
      schema: AUDIO_MANIFEST_SCHEMA,
      events: {
        "ui.click": { status: "asset", bus: "sfx_ui", src: "sfx/ui/click_01.ogg", license: "CC0-1.0", gain: 0.8 },
        "custom.drone": { status: "asset", bus: "sfx_world", src: "drone.ogg", license: "CC-BY-4.0", max_voices: 1 },
      },
    });
    expect(m.events["ui.click"]).toEqual({ status: "asset", bus: "sfx_ui", src: "sfx/ui/click_01.ogg", license: "CC0-1.0", gain: 0.8 });
  });

  const bad: [string, unknown, string][] = [
    ["schema 错误", { schema: "x", events: {} }, "$.schema"],
    ["未知顶层字段", { schema: AUDIO_MANIFEST_SCHEMA, events: {}, extra: 1 }, "$.extra"],
    ["事件名格式", { schema: AUDIO_MANIFEST_SCHEMA, events: { Click: { status: "placeholder", bus: "sfx_ui" } } }, '$.events["Click"]'],
    ["未知总线", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "placeholder", bus: "music" } } }, '$.events["ui.click"].bus'],
    ["占位总线与配方不符", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "placeholder", bus: "sfx_world" } } }, '$.events["ui.click"].bus'],
    ["占位必须有配方", { schema: AUDIO_MANIFEST_SCHEMA, events: { "custom.x": { status: "placeholder", bus: "sfx_ui" } } }, '$.events["custom.x"]'],
    ["路径穿越", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "asset", bus: "sfx_ui", src: "../a.ogg", license: "CC0-1.0" } } }, '$.events["ui.click"].src'],
    ["URL", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "asset", bus: "sfx_ui", src: "https://x/a.ogg", license: "CC0-1.0" } } }, '$.events["ui.click"].src'],
    ["非支持音频", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "asset", bus: "sfx_ui", src: "a.mp3", license: "CC0-1.0" } } }, '$.events["ui.click"].src'],
    ["许可证", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "asset", bus: "sfx_ui", src: "a.ogg", license: "MIT" } } }, '$.events["ui.click"].license'],
    ["状态", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "wav", bus: "sfx_ui" } } }, '$.events["ui.click"].status'],
    ["占位不得带 src", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "placeholder", bus: "sfx_ui", src: "a.ogg" } } }, '$.events["ui.click"].src'],
    ["max_voices 非整数", { schema: AUDIO_MANIFEST_SCHEMA, events: { "ui.click": { status: "placeholder", bus: "sfx_ui", max_voices: 1.5 } } }, '$.events["ui.click"].max_voices'],
  ];
  for (const [name, input, path] of bad) {
    it(`拒绝：${name}`, () => {
      try {
        parseAudioManifest(input);
        expect.unreachable();
      } catch (e) {
        expect(e).toBeInstanceOf(AudioManifestError);
        expect((e as AudioManifestError).path).toBe(path);
      }
    });
  }

  it("JSON Schema 与 TS 常量一致", () => {
    const schema = JSON.parse(
      readFileSync(new URL("../../../protocols/schemas/audio-manifest.schema.json", import.meta.url), "utf8"),
    );
    expect(schema.properties.schema.const).toBe(AUDIO_MANIFEST_SCHEMA);
    expect(schema.$defs.standardEventId.enum).toEqual(STANDARD_AUDIO_EVENT_IDS);
    expect(schema.$defs.placeholderEntry.properties.bus.enum).toEqual(AUDIO_BUS_NAMES);
    expect(schema.$defs.assetEntry.properties.license.enum).toEqual(AUDIO_ASSET_LICENSES);
    expect(new RegExp(schema.$defs.assetEntry.properties.src.pattern).source).toBe(AUDIO_ASSET_SRC_PATTERN.source);
    expect(schema.properties.events.propertyNames.pattern).toBe(AUDIO_EVENT_ID_PATTERN.source);
  });
});
