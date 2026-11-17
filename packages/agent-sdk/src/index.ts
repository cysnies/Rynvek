// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * `@rynvek/agent-sdk`：规范09 §9.1的六项受控工具构造器。
 * 仅生成JSON参数；租约、fencing、控制等级、30 Tick间隔及600 APM均由真实ToolBroker裁决。
 * 不预判迷雾内的实体、可建性或资源，也不提供底层命令旁路。
 */
import { RynvekError, RynvekErrorCode } from "@rynvek/core";

/** 与 protocols/schemas/agent-tools.v1.json 和 Rust ToolName::ALL 逐项一致。 */
export const AGENT_TOOL_NAMES = [
  "query_visible_clusters",
  "sample_regional_fields",
  "issue_strategic_intent",
  "configure_signal_beacon",
  "queue_vat_production",
  "trigger_crucible_breeding",
] as const;
export type AgentToolName = (typeof AGENT_TOOL_NAMES)[number];
/** 本地分组上限，不表示服务端存在批量端点，也不绕过逐调用限速。 */
export const MAX_TOOLS_PER_BATCH = 8;
export const TOOL_MIN_INTERVAL_TICKS = 30;
export const APM_MAX_ACTIONS = 600;
/** @deprecated 仅为600 APM的算术均值，绝不是可即时调用10次的服务端许可。 */
export const MAX_TOOL_CALLS_PER_SECOND = APM_MAX_ACTIONS / 60;

export type StrategicOrderKind = "Move" | "AttackMove" | "Harvest" | "HoldPosition" | "Patrol";
export type BeaconFieldChannel = "field.signal_a" | "field.signal_b";
export interface NeuromodulatorsVector {
  readonly da: number;
  readonly serotonin: number;
  readonly ne: number;
  readonly ach: number;
}
export interface AgentToolCall<TArgs = Record<string, unknown>> {
  readonly name: AgentToolName;
  readonly arguments: TArgs;
}
function invalid(message: string): never {
  throw new RynvekError(RynvekErrorCode.RuleStaticTypeCheckFailed, message);
}
function finiteRange(value: number, min: number, max: number): void {
  if (!Number.isFinite(value) || value < min || value > max) invalid(`数值必须有限且位于[${min}, ${max}]`);
}
function integer(value: number): void {
  if (!Number.isSafeInteger(value)) invalid("坐标或计数必须为可精确表达的整数");
}
function entityId(value: string): void {
  if (typeof value !== "string" || value.length === 0) invalid("实体标识不能为空；归属与存活由权威宿主检查");
}

export function buildQueryVisibleClusters(includeEnemy = true, regionChunk?: readonly [number, number]): AgentToolCall {
  if (typeof includeEnemy !== "boolean") invalid("include_enemy必须为布尔值");
  if (regionChunk) regionChunk.forEach(integer);
  return { name: "query_visible_clusters", arguments: { include_enemy: includeEnemy, ...(regionChunk ? { region_chunk: [...regionChunk] } : {}) } };
}
/** @deprecated 原助手名称不准确；返回可见集群查询，绝不请求完整世界或隐藏场。 */
export function buildQueryWorldObservation(includeEnemy = true): AgentToolCall {
  return buildQueryVisibleClusters(includeEnemy);
}
export function buildSampleRegionalFields(chunkX: number, chunkY: number): AgentToolCall {
  integer(chunkX); integer(chunkY);
  return { name: "sample_regional_fields", arguments: { chunk_x: chunkX, chunk_y: chunkY } };
}

/** 只接受工具协议支持的原型选择，不将实体列表静默扩大为全军。 */
export function buildIssueStrategicIntent(params: {
  readonly unitIds?: readonly string[];
  readonly archetype?: string;
  readonly orderKind: StrategicOrderKind;
  readonly targetX: number;
  readonly targetY: number;
}): AgentToolCall {
  if (params.unitIds !== undefined) invalid("战略工具不支持实体列表；不得静默忽略unitIds");
  const archetypes = ["all", "unit.forager", "unit.striker", "unit.carapace", "unit.spitter", "unit.weaver", "unit.leviathan"];
  if (!params.archetype || !archetypes.includes(params.archetype)) invalid("必须显式指定合法unit_archetype");
  if (!["Move", "AttackMove", "Harvest", "HoldPosition", "Patrol"].includes(params.orderKind)) invalid("未知战略指令");
  finiteRange(params.targetX, 0, 2_147_483_647); finiteRange(params.targetY, 0, 2_147_483_647);
  return { name: "issue_strategic_intent", arguments: { unit_archetype: params.archetype, order_kind: params.orderKind, target_world_x: params.targetX, target_world_y: params.targetY } };
}

/** 建造通过宿主已有的生产工具bld.*分支下发，不伪造place_building工具。 */
export function buildPlaceBuilding(params: { readonly buildingKind: string; readonly tileX: number; readonly tileY: number }): AgentToolCall {
  if (!Number.isSafeInteger(params.tileX) || !Number.isSafeInteger(params.tileY) || params.tileX < 0 || params.tileY < 0) {
    throw new RynvekError(RynvekErrorCode.InvalidBuildingPlacementGrid, "建筑网格坐标必须是非负安全整数");
  }
  if (!params.buildingKind.startsWith("bld.")) invalid("建造请求必须使用bld.*原型");
  return buildQueueVatProduction({ vatId: `${params.tileX},${params.tileY}`, unitArchetype: params.buildingKind, genomeSlug: "", count: 1 });
}

/** 浮点只在Rust权威边界量化一次，保留负半毫单位的远离零舍入语义。 */
export function buildConfigureSignalBeacon(params: {
  readonly beaconId: string;
  readonly channel: BeaconFieldChannel;
  readonly strength: number;
  readonly modulators: NeuromodulatorsVector;
}): AgentToolCall {
  entityId(params.beaconId);
  if (!["field.signal_a", "field.signal_b"].includes(params.channel)) invalid("未知信息素通道");
  finiteRange(params.strength, -50, 50);
  const vec = [params.modulators.da, params.modulators.serotonin, params.modulators.ne, params.modulators.ach];
  vec.forEach(v => finiteRange(v, -1, 1));
  return { name: "configure_signal_beacon", arguments: { beacon_entity_id: params.beaconId, signal_channel: params.channel, signal_strength: params.strength, neuromodulators: vec } };
}
export function buildQueueVatProduction(params: { readonly vatId: string; readonly unitArchetype: string; readonly genomeSlug: string; readonly count: number }): AgentToolCall {
  entityId(params.vatId); integer(params.count); finiteRange(params.count, 1, 5);
  return { name: "queue_vat_production", arguments: { vat_entity_id: params.vatId, unit_archetype: params.unitArchetype, genome_slug: params.genomeSlug, count: params.count } };
}
export function buildTriggerCrucibleBreeding(params: { readonly crucibleId: string; readonly action: "research_tech" | "breed_elite_genome"; readonly techOrArchetypeId: string; readonly mutationRate?: number }): AgentToolCall {
  entityId(params.crucibleId);
  if (!["research_tech", "breed_elite_genome"].includes(params.action)) invalid("未知坩埚动作");
  if (params.mutationRate !== undefined) finiteRange(params.mutationRate, 0.01, 0.25);
  return { name: "trigger_crucible_breeding", arguments: { crucible_entity_id: params.crucibleId, action: params.action, tech_or_archetype_id: params.techOrArchetypeId, ...(params.mutationRate === undefined ? {} : { mutation_rate: params.mutationRate }) } };
}

/** @deprecated 记忆由ToolBroker记录真实执行结果；没有公开的记忆写入工具。 */
export function buildWriteMemoryJournal(_params: { readonly summary: string; readonly tags?: readonly string[] }): never {
  throw new RynvekError(RynvekErrorCode.PluginCapabilityDenied, "ToolBroker未开放记忆写入工具；不能伪造已执行结果");
}
/** 仅校验本地调用分组和工具白名单；不是鉴权、限速器或服务端接受证明。 */
export function validateToolBatch(batch: readonly AgentToolCall[]): void {
  if (batch.length === 0 || batch.length > MAX_TOOLS_PER_BATCH) {
    throw new RynvekError(RynvekErrorCode.CommandRateLimitExceeded, `本地分组数量必须在1..=${MAX_TOOLS_PER_BATCH}之间`);
  }
  for (const call of batch) {
    if (!(AGENT_TOOL_NAMES as readonly string[]).includes(call.name)) invalid(`未知智能体工具：${call.name}`);
  }
}

export * from "./private-agent.js";
