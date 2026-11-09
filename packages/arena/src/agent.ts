// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `ToolAgent`：只通过规范 9.1 六个受控工具行动的确定性策略代理（LLM Agent 的替身与回归基线）。
 *
 * 每次 `step()` 恰好发出一次工具调用（ToolBroker 要求同一 Principal 至少间隔 30 Ticks），按优先级：
 * 1. 尚未感知 → `query_visible_clusters`；
 * 2. 没有孵化槽 → `queue_vat_production` 以 `bld.spawning_vat` 发起建造请求（落点经 `findPlacement` 确定）；
 * 3. 尚未下达进攻 → `issue_strategic_intent` 让突击刺胞簇进攻推进到镜像出生点；
 * 4. 孵化槽已建成且队列空、人口未满 → 排队 2 只采集工蜂；
 * 5. 其余 → `sample_regional_fields` 采样出生点所在 Chunk。
 *
 * 工具层的拒绝（`ToolResponse.error`）只记录不重试：真正的 LLM 应读取错误码自行纠正。
 */

import type { ArenaClient, ToolResponse } from "@rynvek/client";

import { findPlacement } from "./placement.js";
import { nexusCenter } from "./state.js";

export interface AgentDecision {
  readonly tool: string;
  readonly arguments: Record<string, unknown>;
}

export class ToolAgent {
  private perceived = false;
  private attacked = false;
  readonly log: { decision: AgentDecision; response: ToolResponse }[] = [];

  constructor(readonly client: ArenaClient) {}

  /** 纯决策（不发送），便于单测 */
  decide(): AgentDecision | null {
    const obs = this.client.observation;
    const terrain = this.client.terrain;
    const welcome = this.client.welcome;
    if (!obs || !terrain || !welcome) return null;
    if (!this.perceived) return { tool: "query_visible_clusters", arguments: { include_enemy: true } };
    const own = obs.buildings.filter((b) => b.faction === obs.faction);
    const vat = own.find((b) => b.kind === "bld.spawning_vat");
    const nexus = nexusCenter(obs);
    if (!vat && nexus) {
      const origin = findPlacement("bld.spawning_vat", [Math.floor(nexus[0]), Math.floor(nexus[1])], terrain, obs, { minRing: 5 });
      if (origin) {
        return {
          tool: "queue_vat_production",
          arguments: { vat_entity_id: `${origin[0]},${origin[1]}`, unit_archetype: "bld.spawning_vat", genome_slug: "", count: 1 },
        };
      }
    }
    const spawn = welcome.map.spawn_tiles[welcome.faction === 0 ? 1 : 0];
    if (!this.attacked) {
      return {
        tool: "issue_strategic_intent",
        arguments: { unit_archetype: "unit.striker", order_kind: "AttackMove", target_world_x: spawn[0] + 0.5, target_world_y: spawn[1] + 0.5 },
      };
    }
    if (vat?.complete && vat.queue.length === 0 && obs.supply[0] + 2 <= obs.supply[1]) {
      return {
        tool: "queue_vat_production",
        arguments: { vat_entity_id: String(vat.id), unit_archetype: "unit.forager", genome_slug: "seed.forager.v1", count: 2 },
      };
    }
    const home = welcome.map.spawn_tiles[welcome.faction === 0 ? 0 : 1];
    return { tool: "sample_regional_fields", arguments: { chunk_x: Math.floor(home[0] / 256), chunk_y: Math.floor(home[1] / 256) } };
  }

  async step(): Promise<ToolResponse | null> {
    const decision = this.decide();
    if (!decision) return null;
    const response = await this.client.callTool(decision.tool, decision.arguments);
    this.log.push({ decision, response });
    if (!response.error) {
      if (decision.tool === "query_visible_clusters") this.perceived = true;
      if (decision.tool === "issue_strategic_intent") this.attacked = true;
    }
    return response;
  }
}
