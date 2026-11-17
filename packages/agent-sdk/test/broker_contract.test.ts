// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { RynvekErrorCode } from "@rynvek/core";
import { AGENT_TOOL_NAMES, buildQueryVisibleClusters, buildSampleRegionalFields, buildQueueVatProduction, buildTriggerCrucibleBreeding, buildWriteMemoryJournal, buildConfigureSignalBeacon, buildIssueStrategicIntent, buildPlaceBuilding, buildQueryWorldObservation, validateToolBatch } from "../src/index.js";

const registry = JSON.parse(readFileSync(new URL("../../../protocols/schemas/agent-tools.v1.json", import.meta.url), "utf8"));
const beacon = { beaconId: "11", channel: "field.signal_b" as const, strength: -12.5, modulators: { da: 0.5, serotonin: 0, ne: -0.25, ach: 1 } };

describe("SDK 与真实 ToolBroker 的冻结契约", () => {
  it("逐项匹配规范09与Rust共用Schema的六工具，而非不存在的九工具", () => {
    expect([...AGENT_TOOL_NAMES]).toEqual(registry.map((tool: {name: string}) => tool.name));
  });
  it("既有查询与建造助手只构造权威宿主真实接受的工具", () => {
    expect(buildQueryWorldObservation(true)).toEqual({ name: "query_visible_clusters", arguments: { include_enemy: true } });
    expect(buildPlaceBuilding({ buildingKind: "bld.signal_beacon", tileX: 36, tileY: 36 })).toEqual({ name: "queue_vat_production", arguments: { vat_entity_id: "36,36", unit_archetype: "bld.signal_beacon", genome_slug: "", count: 1 } });
  });
  it("战略意图字段匹配服务端Schema，不把实体选择静默扩大为全军", () => {
    expect(buildIssueStrategicIntent({ archetype: "unit.striker", orderKind: "Move", targetX: 40, targetY: 40 })).toEqual({ name: "issue_strategic_intent", arguments: { unit_archetype: "unit.striker", order_kind: "Move", target_world_x: 40, target_world_y: 40 } });
    expect(() => buildIssueStrategicIntent({ unitIds: ["2"], orderKind: "Move", targetX: 40, targetY: 40 })).toThrow();
  });
  it("信标浮点参数原样交给Rust只量化一次，不以number触发bigint异常", () => {
    expect(buildConfigureSignalBeacon(beacon)).toEqual({ name: "configure_signal_beacon", arguments: { beacon_entity_id: "11", signal_channel: "field.signal_b", signal_strength: -12.5, neuromodulators: [0.5, 0, -0.25, 1] } });
  });
  it.each([NaN, Infinity, -Infinity])("拒绝不可JSON表达的坐标与调质 %s", value => {
    expect(() => buildIssueStrategicIntent({ archetype: "all", orderKind: "Move", targetX: value, targetY: 0 })).toThrow();
    expect(() => buildConfigureSignalBeacon({ ...beacon, strength: value })).toThrow();
    expect(() => buildConfigureSignalBeacon({ ...beacon, modulators: { ...beacon.modulators, da: value } })).toThrow();
  });
  it("六工具输出逐字段等于真实ArenaSession回放的共享夹具", () => {
    const fixture = JSON.parse(readFileSync(new URL("../../../tests/arena/agent-sdk-tools.json", import.meta.url), "utf8"));
    expect([
      buildQueryVisibleClusters(true, [0, 0]),
      buildSampleRegionalFields(0, 0),
      buildPlaceBuilding({ buildingKind: "bld.signal_beacon", tileX: 36, tileY: 36 }),
      buildConfigureSignalBeacon(beacon),
      buildIssueStrategicIntent({ archetype: "all", orderKind: "Move", targetX: 40, targetY: 40 }),
      buildQueueVatProduction({ vatId: "11", unitArchetype: "unit.forager", genomeSlug: "seed.forager.v1", count: 1 }),
      buildTriggerCrucibleBreeding({ crucibleId: "1", action: "research_tech", techOrArchetypeId: "tech.hebbian_plasticity" }),
      buildConfigureSignalBeacon({ ...beacon, strength: -0.0005, modulators: { da: -0.0005, serotonin: 0.0005, ne: -1, ach: 1 } }),
    ]).toEqual(fixture);
  });
  it("不存在的记忆写工具显式拒绝，不伪造服务端接受或已执行结果", () => {
    expect(() => buildWriteMemoryJournal({ summary: "未发生的操作" })).toThrowError(expect.objectContaining({ code: RynvekErrorCode.PluginCapabilityDenied }));
  });
  it("生产数量、坐标精度和繁育率在本地只做无状态参数校验", () => {
    for (const count of [0, 6, 1.5, NaN, Infinity]) {
      expect(() => buildQueueVatProduction({ vatId: "13", unitArchetype: "unit.forager", genomeSlug: "seed.forager.v1", count })).toThrow();
    }
    for (const mutationRate of [0, 0.251, NaN, Infinity]) {
      expect(() => buildTriggerCrucibleBreeding({ crucibleId: "14", action: "breed_elite_genome", techOrArchetypeId: "unit.forager", mutationRate })).toThrow();
    }
    expect(() => buildSampleRegionalFields(Number.MAX_SAFE_INTEGER + 1, 0)).toThrow();
    expect(() => buildIssueStrategicIntent({ orderKind: "Move", targetX: 40, targetY: 40 })).toThrow();
    expect(buildConfigureSignalBeacon({ ...beacon, strength: -0.0005 }).arguments).toMatchObject({ signal_strength: -0.0005 });
  });
  it("错误码必须是跨端冻结码，不得为undefined或自行新增别名", () => {
    expect(() => buildPlaceBuilding({ buildingKind: "bld.signal_beacon", tileX: -1, tileY: 0 })).toThrowError(expect.objectContaining({ code: RynvekErrorCode.InvalidBuildingPlacementGrid }));
    expect(() => validateToolBatch([])).toThrowError(expect.objectContaining({ code: RynvekErrorCode.CommandRateLimitExceeded }));
    expect(() => validateToolBatch([{ name: "unknown" as never, arguments: {} }])).toThrowError(expect.objectContaining({ code: RynvekErrorCode.RuleStaticTypeCheckFailed }));
  });
});
