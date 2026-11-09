// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 无头视图层单测：以夹具中第一帧真实观测为输入，验证 HUD 数值、建造预览与小地图迷雾。

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { NEXUS_STORAGE_CAP, STARTER_RESOURCES, TerrainGrid, milliToWhole, type FactionObservation, type MapInfo } from "@rynvek/client";
import { describe, expect, it } from "vitest";

import { factionColor, findPlacement, hudView, nexusCenter, pixel, placementIssues, renderMinimap } from "../src/index.js";

const FIXTURE = fileURLToPath(new URL("../../../tests/arena/opening.transcript.json", import.meta.url));
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const steps: any[] = JSON.parse(readFileSync(FIXTURE, "utf8"));
const map: MapInfo = steps[0].recv[0].message.map;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const firstObs = (to: string): FactionObservation => steps[5].recv.find((e: any) => e.to === to).message.observation;

describe("视图层（首帧观测）", () => {
  const terrain = TerrainGrid.decode(map);
  const p0 = firstObs("human:p0");
  const p1 = firstObs("agent:p1");

  it("HUD：Starter Kit 资源、存储上限、单位与人口", () => {
    const hud = hudView(p0);
    expect(hud.tick).toBe(1);
    expect(hud.biomass).toEqual({ amount: milliToWhole(STARTER_RESOURCES.biomass), capacity: milliToWhole(NEXUS_STORAGE_CAP.biomass) });
    expect(hud.flux.capacity).toBe(1000);
    expect(hud.neurogel.capacity).toBe(500);
    expect(hud.unitCounts).toEqual({ "unit.forager": 3, "unit.striker": 1 });
    expect(hud.buildingCounts).toEqual({ "bld.nexus_core": 1 });
    expect(hud.supply).toEqual({ used: 5, cap: 0 });
    expect(hud.hatchable).toEqual(["unit.forager", "unit.striker"]);
  });

  it("建造预览：主脑重叠 / 越界 / 不可建造 / 迷雾", () => {
    const [cx, cy] = nexusCenter(p0)!;
    expect([cx, cy]).toEqual([32, 32]);
    expect(placementIssues("bld.spawning_vat", [33, 33], terrain, p0)).toContain("overlap");
    expect(placementIssues("bld.spawning_vat", [254, 254], terrain, p0)).toEqual(["out_of_bounds"]);
    expect(placementIssues("bld.nexus_core", [40, 40], terrain, p0)).toContain("not_buildable");
    expect(placementIssues("bld.signal_beacon", [200, 200], terrain, p0)).toContain("not_visible");
  });

  it("确定性选址：结果唯一，且与夹具中 Agent 实际落下的孵化槽一致", () => {
    const [x1, y1] = nexusCenter(p1)!;
    const c1: [number, number] = [Math.floor(x1), Math.floor(y1)];
    const a = findPlacement("bld.spawning_vat", c1, terrain, p1, { minRing: 5 });
    expect(a).toEqual(findPlacement("bld.spawning_vat", c1, terrain, p1, { minRing: 5 }));
    expect(a).toEqual([219, 219]);
    expect(placementIssues("bld.spawning_vat", a!, terrain, p1)).toEqual([]);
  });

  it("小地图：己方主脑着色、未探索区域全黑", () => {
    const m = renderMinimap(terrain, p0);
    expect([m.width, m.height]).toEqual([128, 128]);
    expect(pixel(m, 16, 16).slice(0, 3)).toEqual(factionColor(0));
    expect(pixel(m, 120, 120)).toEqual([0, 0, 0, 255]);
  });
});
