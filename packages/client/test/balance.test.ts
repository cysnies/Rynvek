// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

import {
  BUILDING_SPECS,
  NEXUS_STORAGE_CAP,
  STARTER_RESOURCES,
  TECH_SPECS,
  TerrainTileKind,
  UNIT_SPECS,
  VAT_QUEUE_LIMIT,
  VAT_SUPPLY_BONUS,
  allowsBuilding,
  canAfford,
  techAvailable,
  unitUnlocked,
} from "../src/index.js";

/** Rust 侧生成并断言的平衡表夹具 */
const fixture = JSON.parse(readFileSync(new URL("../../../tests/arena/balance.json", import.meta.url), "utf8"));

const TERRAINS = [0, 1, 2, 3, 4] as const;

describe("平衡表与 Rust 冻结表逐项一致", () => {
  it("3 资源开局与存储上限", () => {
    expect(STARTER_RESOURCES).toEqual(fixture.starter_resources);
    expect(NEXUS_STORAGE_CAP).toEqual(fixture.nexus_storage_cap);
    expect(VAT_SUPPLY_BONUS).toBe(fixture.vat_supply_bonus);
    expect(VAT_QUEUE_LIMIT).toBe(fixture.vat_queue_limit);
  });

  it("8 建筑", () => {
    expect(BUILDING_SPECS).toHaveLength(8);
    expect(
      BUILDING_SPECS.map((b) => ({
        kind: b.kind,
        name_zh: b.nameZh,
        footprint: b.footprint,
        cost: b.cost,
        build_ticks: b.buildTicks,
        hp_milli: b.hpMilli,
        armor: b.armor,
        flux_per_s_milli: b.fluxPerSecondMilli,
        vision_tiles: b.visionTiles,
        allowed_terrain: TERRAINS.map((t) => allowsBuilding(t as TerrainTileKind, b.kind)),
      })),
    ).toEqual(fixture.buildings);
  });

  it("6 神经单位", () => {
    expect(
      UNIT_SPECS.map((u) => ({
        kind: u.kind,
        name_zh: u.nameZh,
        supply: u.supply,
        cost: u.cost,
        hatch_ticks: u.hatchTicks,
        hp_milli: u.hpMilli,
        shield_milli: u.shieldMilli,
        armor: u.armor,
        speed_milli_per_s: u.speedMilliPerSecond,
        vision_tiles: u.visionTiles,
        unlocked_by: u.unlockedBy,
      })),
    ).toEqual(fixture.units);
  });

  it("9 节点科技树", () => {
    expect(
      TECH_SPECS.map((t) => ({
        id: t.id,
        name_zh: t.nameZh,
        tier: t.tier,
        research_site: t.researchSite,
        requires_buildings: t.requiresBuildings,
        requires_techs: t.requiresTechs,
        cost: t.cost,
        research_ticks: t.researchTicks,
      })),
    ).toEqual(fixture.techs);
  });

  it("前置与解锁判定", () => {
    expect(techAvailable("tech.chemotaxis_amp", [], [])).toBe(false);
    expect(techAvailable("tech.chemotaxis_amp", [], ["bld.spawning_vat"])).toBe(true);
    expect(techAvailable("tech.gap_junction_sync", ["tech.hebbian_plasticity"], [])).toBe(true);
    expect(techAvailable("tech.gap_junction_sync", ["tech.hebbian_plasticity", "tech.gap_junction_sync"], [])).toBe(false);
    expect(unitUnlocked("unit.striker", [])).toBe(true);
    expect(unitUnlocked("unit.leviathan", ["tech.gap_junction_sync"])).toBe(false);
    expect(canAfford(STARTER_RESOURCES, BUILDING_SPECS[7]!.cost)).toBe(false);
    expect(canAfford(STARTER_RESOURCES, BUILDING_SPECS[4]!.cost)).toBe(true);
  });
});
