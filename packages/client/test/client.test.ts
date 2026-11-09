// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import {
  ApmBudget,
  ArenaClient,
  ArenaError,
  MemoryTransport,
  commandLevel,
  type ArenaClientMessage,
  type ArenaServerMessage,
  type ControlLease,
} from "../src/index.js";

const lease = (token: number, level: ControlLease["level"] = "Produce"): ControlLease => ({
  holder: { id: "agent:a", kind: "Agent" },
  faction: 0,
  level,
  fencing_token: token,
  issued_tick: 0,
  expires_tick: 39600,
});

/** 只回放预置应答的极简服务端 */
function harness(reply: (m: ArenaClientMessage) => ArenaServerMessage[]) {
  const t: MemoryTransport = new MemoryTransport((m) => queueMicrotask(() => reply(m).forEach((r) => t.deliver(r))));
  return { t, client: new ArenaClient(t, { principalId: "agent:a", kind: "Agent", faction: 0 }) };
}

describe("ArenaClient", () => {
  it("按 seq 关联回执并附带 Fencing Token", async () => {
    const { t, client } = harness((m) => {
      if (m.type === "acquire_lease") return [{ type: "lease", seq: m.seq, lease: lease(7, m.level) }];
      if (m.type === "command") return [{ type: "ack", seq: m.seq, tick: 12, ack: { type: "order_issued", units: 3 } }];
      return [];
    });
    await client.acquireLease("Produce");
    const ack = await client.command({ type: "issue_order", units: { kind: "archetype", value: null }, order: "Move", target: [0, 0] });
    expect(ack).toEqual({ type: "order_issued", units: 3 });
    expect(t.sent[1]).toMatchObject({ type: "command", seq: 2, fencing_token: 7 });
    expect(client.apm.used(12)).toBe(1);
  });

  it("本地预检与服务端同码：无租约 0x8001、等级不足 0x5006、APM 用尽 0x8004", async () => {
    const { t, client } = harness((m) => (m.type === "acquire_lease" ? [{ type: "lease", seq: m.seq, lease: lease(1, "Modulate") }] : []));
    const surrender = { type: "surrender" } as const;
    await expect(client.command(surrender)).rejects.toMatchObject({ code: 0x8001 });
    await client.acquireLease("Modulate");
    await expect(client.command({ type: "cancel_queue", vat: 9 })).rejects.toMatchObject({ code: 0x5006 });
    for (let i = 0; i < 600; i++) client.apm.record(0);
    await expect(client.command({ type: "issue_order", units: { kind: "ids", value: [1] }, order: "HoldPosition", target: [0, 0] })).rejects.toMatchObject({ code: 0x8004 });
    expect(t.sent.map((m) => m.type)).toEqual(["acquire_lease"]);
    expect(commandLevel(surrender)).toBe("Sovereign");
  });

  it("服务端拒绝以 ArenaError 抛出，被接管后租约作废", async () => {
    const events: string[] = [];
    const { t, client } = harness((m) => {
      if (m.type === "acquire_lease") return [{ type: "lease", seq: m.seq, lease: lease(3) }];
      if (m.type === "command")
        return [
          { type: "ack", seq: m.seq, tick: 5, error: { code: 0x8005, name: "InsufficientFactionResources", message: "资源不足" } },
          { type: "lease_revoked", faction: 0, by: "human:h" },
        ];
      return [];
    });
    client.on((e) => events.push(e.type));
    await client.acquireLease("Produce");
    const err = await client.command({ type: "place_building", kind: "bld.mutation_crucible", origin: [40, 40] }).catch((e) => e);
    expect(err).toBeInstanceOf(ArenaError);
    expect(err.code).toBe(0x8005);
    await Promise.resolve();
    expect(client.lease).toBeNull();
    expect(events).toEqual(["lease", "lease_revoked"]);
    expect(t.sent).toHaveLength(2);
  });

  it("工具调用返回 ToolResponse；advance 等待下一份观测", async () => {
    const obs = { tick: 31 } as never;
    const { client } = harness((m) => {
      if (m.type === "tool_call") return [{ type: "tool_result", seq: m.seq, response: { tool: m.tool, tick: 1, error: { code: 0x8001, name: "UnauthorizedCapabilityToken", message: "x" } } }];
      if (m.type === "advance") return [{ type: "observation", observation: obs, events: [{ type: "sudden_death_started", tick: 31 }] }];
      return [];
    });
    const r = await client.callTool("query_visible_clusters", { include_enemy: true });
    expect(r.error?.code).toBe(0x8001);
    expect(await client.advance(30)).toBe(obs);
    expect(client.tick).toBe(31);
    expect(client.events).toHaveLength(1);
  });
});

describe("ApmBudget", () => {
  it("与 Rust ApmLimiter 同一滑动窗口", () => {
    const b = new ApmBudget();
    for (let i = 0; i < 600; i++) b.record(i);
    expect(b.remaining(1799)).toBe(0);
    expect(b.used(1800)).toBe(599);
  });
});
