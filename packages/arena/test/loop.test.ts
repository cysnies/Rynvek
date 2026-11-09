// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { TerrainTileKind } from "@rynvek/client";
import { describe, expect, it } from "vitest";

import { factionColor, pixel } from "../src/index.js";
import { LiveHub, ReplayHub, serializeTranscript, type TranscriptStep } from "./hub.js";
import { runScenario, type ScenarioResult } from "./scenario.js";

const FIXTURE = fileURLToPath(new URL("../../../tests/arena/opening.transcript.json", import.meta.url));
/** 可选：指向 `cargo build -p rynvek-engine --example arena_server` 的产物以运行实况闭环 */
const SERVER = process.env.RYNVEK_ARENA_SERVER;

function assertOutcome(r: ScenarioResult): void {
  const hud = r.human.hud()!;
  // ---- 资源 / 建筑 / 单位数值
  expect(hud.tick).toBe(2431);
  expect(hud.buildingCounts["bld.nexus_core"]).toBe(1);
  expect(hud.buildingCounts["bld.spawning_vat"]).toBe(1);
  expect(hud.buildingCounts["bld.biomass_harvester"]).toBe(1);
  expect(hud.underConstruction).toBe(0);
  // 3 只开局工蜂 + 人类排队的 2 只；两军突击刺胞簇在中线交火同归于尽
  expect(hud.unitCounts).toEqual({ "unit.forager": 5 });
  expect(hud.supply).toEqual({ used: 5, cap: 24 });
  expect(hud.biomass).toEqual({ amount: 2000, capacity: 2000 });
  expect(hud.availableTechs).toContain("tech.chemotaxis_amp");
  expect(hud.biomass.capacity).toBe(2000);
  expect(hud.outcome).toBeNull();
  const b = r.agentB.hud()!;
  expect(b.buildingCounts["bld.spawning_vat"]).toBe(1);
  expect(b.unitCounts).toEqual({ "unit.forager": 7 });
  expect(r.human.client.events.filter((e) => e.type === "unit_died").map((e) => "kind" in e && e.kind)).toEqual(["unit.striker"]);

  // ---- 受控工具调用
  const codes = (id: string) => r.toolResponses[id]!.map((x) => x.error?.code ?? 0);
  expect(r.toolResponses["agent:p1"]!.map((x) => x.tool)).toEqual([
    "query_visible_clusters",
    "queue_vat_production",
    "issue_strategic_intent",
    "queue_vat_production",
    "queue_vat_production",
  ]);
  expect(codes("agent:p1")).toEqual([0, 0, 0, 0, 0]);
  expect(r.toolResponses["agent:p0"]!.map((x) => [x.tool, x.tick])).toEqual([
    ["query_visible_clusters", 1],
    ["issue_strategic_intent", 31],
    ["issue_strategic_intent", 631],
  ]);
  expect(codes("agent:p0")).toEqual([0, 0x8001, 0]);
  expect(r.agentA.client.lease?.level).toBe("Produce");
  expect(r.human.client.lease).toBeNull();
  const clusters = r.toolResponses["agent:p1"]![0]!.result as { clusters: { archetype: string; count: number }[] };
  expect(clusters.clusters.map((c) => [c.archetype, c.count])).toEqual([
    ["unit.forager", 3],
    ["unit.striker", 1],
  ]);

  // ---- 界面状态
  const ui = r.human.ui;
  expect(ui.controlGroups.get(1)).toHaveLength(3);
  expect(ui.minimapVisible).toBe(false);
  expect(ui.camera.x).toBe(32);
  expect(ui.notices).toEqual([]);
  expect(r.human.client.apm.used(hud.tick)).toBe(0); // 早于 1800 Tick 窗口的动作已滑出
  expect(r.agentA.ui.notices.map((n) => n.code)).toEqual([0x8001]);

  // ---- 地图与小地图
  const terrain = r.human.client.terrain!;
  expect(terrain.isPointSymmetric()).toBe(true);
  const mm = r.human.minimap(1)!;
  const obs = r.human.client.observation!;
  const nexus = obs.buildings.find((x) => x.kind === "bld.nexus_core" && x.faction === 0)!;
  expect(pixel(mm, nexus.origin[0], nexus.origin[1]).slice(0, 3)).toEqual(factionColor(0));
  expect(pixel(mm, 250, 250)).toEqual([0, 0, 0, 255]); // 敌方出生点未探索
  expect(terrain.at(nexus.origin[0], nexus.origin[1])).not.toBe(TerrainTileKind.ImperviousWall);
}

describe("Arena 可玩闭环（回放 Rust 权威会话夹具）", () => {
  it("人类键鼠 + 两个受控工具 Agent 的开局与 Rust ArenaSession 逐消息一致", async () => {
    const transcript = JSON.parse(readFileSync(FIXTURE, "utf8")) as TranscriptStep[];
    const hub = new ReplayHub(transcript);
    const result = await runScenario(hub);
    expect(hub.remaining).toBe(0);
    assertOutcome(result);
  });
});

describe.runIf(SERVER !== undefined && existsSync(SERVER))("Arena 可玩闭环（实况 arena_server 进程）", () => {
  it("实况会话与夹具一致；RYNVEK_ARENA_RECORD=1 时重新录制夹具", async () => {
    const hub = new LiveHub(SERVER!, ["--seed", "42"]);
    try {
      const result = await runScenario(hub);
      if (process.env.RYNVEK_ARENA_RECORD === "1") writeFileSync(FIXTURE, serializeTranscript(hub.steps));
      assertOutcome(result);
      const transcript = JSON.parse(readFileSync(FIXTURE, "utf8")) as TranscriptStep[];
      expect(hub.steps).toEqual(transcript);
    } finally {
      hub.close();
    }
  });
});
