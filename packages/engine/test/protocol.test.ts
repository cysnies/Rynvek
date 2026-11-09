// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  ALL_LAYERS,
  ENGINE_COMMAND_TYPES,
  ENGINE_EVENT_TYPES,
  FIELD_CHANNELS,
  RenderLayer,
  layerMask,
  parseEngineEvent,
  parseEngineEvents,
} from "../src/index.js";

const fixture = JSON.parse(
  readFileSync(new URL("../../../tests/engine/protocol_messages.json", import.meta.url), "utf8"),
) as { commands: { type: string }[]; events: unknown[] };

describe("跨语言协议夹具（与 rynvek-engine serde 对称）", () => {
  it("命令类型集合与 Rust EngineCommand 完全一致", () => {
    expect(fixture.commands.map((c) => c.type)).toEqual([...ENGINE_COMMAND_TYPES]);
  });

  it("每条夹具事件都通过运行时校验且类型集合完整", () => {
    for (const ev of fixture.events) expect(parseEngineEvent(ev)).toEqual(ev);
    const kinds = new Set(fixture.events.map((e) => (e as { type: string }).type));
    expect([...kinds].sort()).toEqual([...ENGINE_EVENT_TYPES].sort());
  });

  it("热力图通道名与 FieldChannelId::as_str 一致", () => {
    const heat = fixture.commands.find((c) => c.type === "SET_HEATMAP_CHANNEL") as unknown as { channel: string };
    expect(FIELD_CHANNELS).toContain(heat.channel);
    expect(FIELD_CHANNELS).toEqual(["nutrient", "energy", "signal_a", "signal_b", "hazard"]);
  });
});

describe("事件校验", () => {
  it("拒绝未知类型、缺字段、错类型与非法状态根", () => {
    expect(parseEngineEvent(null)).toBeNull();
    expect(parseEngineEvent({ type: "NOPE" })).toBeNull();
    expect(parseEngineEvent({ type: "TICK_COMMITTED", tick: 1 })).toBeNull();
    expect(parseEngineEvent({ type: "TICK_COMMITTED", tick: 1, stateRoot: "XYZ" })).toBeNull();
    expect(parseEngineEvent({ type: "TICK_COMMITTED", tick: -1, stateRoot: "a".repeat(64) })).toBeNull();
    expect(parseEngineEvent({ type: "PICK_RESULT", requestId: 1.5, denseIndex: null })).toBeNull();
    expect(parseEngineEvent({ type: "ENGINE_READY", backend: "metal", profile: null, adapter: null, generation: 0 })).toBeNull();
    expect(parseEngineEvent({ type: "toString" })).toBeNull();
  });

  it("单条与数组负载都可解析，非法项单独计入 rejected", () => {
    const ok = { type: "FRAME_RENDERED", tick: 3 };
    expect(parseEngineEvents(ok)).toEqual({ events: [ok], rejected: [] });
    expect(parseEngineEvents([ok, { type: "X" }, ok])).toEqual({ events: [ok, ok], rejected: [{ type: "X" }] });
  });
});

describe("图层位集", () => {
  it("L0–L7 与 rynvek-render 位序一致", () => {
    expect(RenderLayer.Terrain).toBe(0);
    expect(RenderLayer.Overlay).toBe(7);
    expect(layerMask([RenderLayer.Terrain, RenderLayer.Organisms, RenderLayer.Overlay])).toBe(0b1000_1001);
    expect(layerMask(Object.values(RenderLayer))).toBe(ALL_LAYERS);
  });
});
