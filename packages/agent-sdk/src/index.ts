// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * `@rynvek/agent-sdk`：Rynvek 智能体客户端 SDK（规范 09 §9.1 / 审计报告 §12.2）。
 *
 * 为外部 LLM / 强化学习智能体提供强类型的 9 项标准 ToolBroker / MCP 工具构造器、
 * 速率预算检查器（10 次/秒，单次最多 8 项操作）与迷雾裁剪观测辅助方法。
 */

import { Q32_32, RynvekError, RynvekErrorCode } from "@rynvek/core";

/** 九项冻结的 ToolBroker 工具名称 */
export const AGENT_TOOL_NAMES = [
  "query_world_observation",
  "issue_strategic_intent",
  "place_building",
  "demolish_building",
  "queue_unit_production",
  "cancel_production_queue",
  "configure_signal_beacon",
  "research_or_evolve",
  "write_agent_memory_journal",
] as const;

export type AgentToolName = (typeof AGENT_TOOL_NAMES)[number];

/** 单次批量调用的最大工具数（规范 09 冻结上限） */
export const MAX_TOOLS_PER_BATCH = 8;
/** 每秒最大工具调用次数（规范 09 冻结上限） */
export const MAX_TOOL_CALLS_PER_SECOND = 10;

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

/** 构造 `query_world_observation` 工具调用 */
export function buildQueryWorldObservation(includeFieldsDownsample = true): AgentToolCall<{
  readonly include_fields: boolean;
}> {
  return {
    name: "query_world_observation",
    arguments: { include_fields: includeFieldsDownsample },
  };
}

/** 构造 `issue_strategic_intent` 战略意图工具调用（坐标自动校验非负） */
export function buildIssueStrategicIntent(params: {
  readonly unitIds?: readonly string[];
  readonly archetype?: string;
  readonly orderKind: StrategicOrderKind;
  readonly targetX: number;
  readonly targetY: number;
}): AgentToolCall {
  if (params.targetX < 0 || params.targetY < 0) {
    throw new RynvekError(RynvekErrorCode.CoordinateOutOfCanonicalRange, "目标坐标不能为负");
  }
  return {
    name: "issue_strategic_intent",
    arguments: {
      unit_ids: params.unitIds ? [...params.unitIds] : undefined,
      archetype: params.archetype,
      order_kind: params.orderKind,
      target: [params.targetX, params.targetY],
    },
  };
}

/** 构造 `place_building` 建造工具调用 */
export function buildPlaceBuilding(params: {
  readonly buildingKind: string;
  readonly tileX: number;
  readonly tileY: number;
}): AgentToolCall {
  if (!Number.isInteger(params.tileX) || !Number.isInteger(params.tileY) || params.tileX < 0 || params.tileY < 0) {
    throw new RynvekError(RynvekErrorCode.InvalidBuildingPlacementLocation, "建筑网格坐标必须是非负整数");
  }
  return {
    name: "place_building",
    arguments: {
      building_kind: params.buildingKind,
      origin: [params.tileX, params.tileY],
    },
  };
}

/** 构造 `configure_signal_beacon` L3 神经调制信标配置调用（严格钳制并校验强度与四维调质范围） */
export function buildConfigureSignalBeacon(params: {
  readonly beaconId: string;
  readonly channel: BeaconFieldChannel;
  readonly strength: number;
  readonly modulators: NeuromodulatorsVector;
}): AgentToolCall {
  if (params.strength < -50 || params.strength > 50) {
    throw new RynvekError(RynvekErrorCode.DimensionMismatch, "信标信号强度必须在 [-50, 50] 区间内");
  }
  const vec = [params.modulators.da, params.modulators.serotonin, params.modulators.ne, params.modulators.ach];
  for (const v of vec) {
    if (v < -1 || v > 1) {
      throw new RynvekError(RynvekErrorCode.DimensionMismatch, "神经调质分量必须在 [-1.0, 1.0] 区间内");
    }
  }
  // 同时验证可通过 Q32_32.fromMilli 安全转换
  void Q32_32.fromMilli(Math.round(params.strength * 1000));
  return {
    name: "configure_signal_beacon",
    arguments: {
      beacon_id: params.beaconId,
      channel: params.channel,
      signal_strength: params.strength,
      neuromodulators: vec,
    },
  };
}

/** 构造 `write_agent_memory_journal` 情景记忆日志写入调用 */
export function buildWriteMemoryJournal(params: {
  readonly summary: string;
  readonly tags?: readonly string[];
}): AgentToolCall {
  return {
    name: "write_agent_memory_journal",
    arguments: {
      summary: params.summary,
      tags: params.tags ? [...params.tags] : [],
    },
  };
}

/** 校验一批工具调用是否符合单批上限（`<= 8`）与合法工具名白名单 */
export function validateToolBatch(batch: readonly AgentToolCall[]): void {
  if (batch.length === 0 || batch.length > MAX_TOOLS_PER_BATCH) {
    throw new RynvekError(
      RynvekErrorCode.AgentToolCallRateLimitExceeded,
      `单批工具调用数必须在 1..=${MAX_TOOLS_PER_BATCH} 之间，当前为 ${batch.length}`,
    );
  }
  for (const call of batch) {
    if (!(AGENT_TOOL_NAMES as readonly string[]).includes(call.name)) {
      throw new RynvekError(RynvekErrorCode.CapabilityScopeDenied, `未知的智能体工具名：${call.name}`);
    }
  }
}
