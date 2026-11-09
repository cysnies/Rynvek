// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 开局闭环脚本（PvP 1v1，地图种子 42）：
 *
 * - `human:p0`（人类，阵营 0）全部通过 `InputActionRouter` 的键鼠事件操作：建造、框选、右键采集、
 *   双击同类、`Q` + 左键进攻、`Ctrl+1` 编队、`Backspace` 回主脑、`M` 小地图、`Alt+A` 交还控制；
 * - `agent:p0`（Agent，阵营 0）开局持有 `Produce` 租约，被人类无感接管后旧 Token 失效（`0x8001`），
 *   人类交还后自动恢复并继续生产；
 * - `agent:p1`（Agent，阵营 1）全程只用六个受控工具对战。
 */

import { buildingSpec, rawToFloat, TerrainTileKind, type ToolResponse } from "@rynvek/client";
import { expect } from "vitest";

import { createArenaApp, findPlacement, nexusCenter, ToolAgent, type ArenaApp } from "../src/index.js";
import type { SessionHub } from "./hub.js";

export interface ScenarioResult {
  readonly human: ArenaApp;
  readonly agentA: ArenaApp;
  readonly agentB: ArenaApp;
  readonly botA: ToolAgent;
  readonly botB: ToolAgent;
  readonly toolResponses: Record<string, ToolResponse[]>;
}

const key = (k: string, mods: Partial<{ ctrl: boolean; shift: boolean; alt: boolean }> = {}) => ({
  key: k,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
  metaKey: false,
});

export async function runScenario(hub: SessionHub): Promise<ScenarioResult> {
  const human = createArenaApp(hub.transportFor("human:p0"), { principalId: "human:p0", kind: "Human", faction: 0 });
  const agentA = createArenaApp(hub.transportFor("agent:p0"), { principalId: "agent:p0", kind: "Agent", faction: 0 });
  const agentB = createArenaApp(hub.transportFor("agent:p1"), { principalId: "agent:p1", kind: "Agent", faction: 1 });
  const botA = new ToolAgent(agentA.client);
  const botB = new ToolAgent(agentB.client);
  const toolResponses: Record<string, ToolResponse[]> = { "agent:p0": [], "agent:p1": [] };
  const stepA = async () => toolResponses["agent:p0"]!.push((await botA.step())!);
  const stepB = async () => toolResponses["agent:p1"]!.push((await botB.step())!);

  await agentA.client.connect();
  await agentB.client.connect();
  await human.client.connect();
  await agentA.client.acquireLease("Produce");
  await agentB.client.acquireLease("Produce");
  await human.client.advance(1);

  await stepB(); // 感知
  await stepA(); // 感知

  // ---- 人类无感接管阵营 0
  await human.client.acquireLease("Sovereign");
  await Promise.resolve();
  expect(agentA.client.lease).toBeNull();

  const r = human.router;
  const c = human.controller;
  const obs0 = human.client.observation!;
  const terrain = human.client.terrain!;
  const nexus = nexusCenter(obs0)!;
  const home: [number, number] = [Math.floor(nexus[0]), Math.floor(nexus[1])];

  // B → 建造抽屉 → 孵化槽 → 左键落点
  r.handleKey(key("b"));
  expect(human.ui.buildMenuOpen).toBe(true);
  c.chooseBuilding("bld.spawning_vat");
  const vatAt = findPlacement("bld.spawning_vat", home, terrain, obs0, { minRing: 5 })!;
  r.handlePointer({ kind: "click", button: "left", x: vatAt[0] + 0.5, y: vatAt[1] + 0.5 });
  await c.idle();

  // 生物质萃取器：优先覆盖富营养沼泽
  const obs1 = human.client.observation!;
  const marshIn = (o: readonly [number, number], f: number) => {
    for (let y = o[1]; y < o[1] + f; y++) for (let x = o[0]; x < o[0] + f; x++) if (terrain.at(x, y) === TerrainTileKind.NutrientMarsh) return true;
    return false;
  };
  r.handleKey(key("b"));
  c.chooseBuilding("bld.biomass_harvester");
  const harvesterAt = findPlacement("bld.biomass_harvester", home, terrain, obs1, {
    minRing: 4,
    accept: (o) => marshIn(o, buildingSpec("bld.biomass_harvester").footprint),
  })!;
  r.handlePointer({ kind: "click", button: "left", x: harvesterAt[0] + 0.5, y: harvesterAt[1] + 0.5 });
  await c.idle();

  // 框选三只工蜂（框住后用 Shift+左键去掉误选的突击兵）→ 右键最近的沼泽 → Harvest
  const foragers = obs1.units.filter((u) => u.faction === 0 && u.kind === "unit.forager").map((u) => rawToFloat(u.position));
  const xs = foragers.map((p) => p[0]);
  const ys = foragers.map((p) => p[1]);
  r.handlePointer({ kind: "drag", button: "left", x: Math.min(...xs) - 0.2, y: Math.min(...ys) - 0.2, x2: Math.max(...xs) + 0.2, y2: Math.max(...ys) + 0.2 });
  const striker = obs1.units.find((u) => u.faction === 0 && u.kind === "unit.striker")!;
  const sp = rawToFloat(striker.position);
  if (human.ui.selection.has(striker.id)) r.handlePointer({ kind: "click", button: "left", x: sp[0], y: sp[1], shift: true });
  expect(human.ui.selection.size).toBe(3);
  // 采集区（2.5 Tile）内神经脑会自由游走：避开周围 4 Tile 内有热液裂隙（灼烧）的沼泽格
  const fissureNear = (x: number, y: number) => {
    for (let dy = -4; dy <= 4; dy++)
      for (let dx = -4; dx <= 4; dx++) if (terrain.inBounds(x + dx, y + dy) && terrain.at(x + dx, y + dy) === TerrainTileKind.AcidicFissure) return true;
    return false;
  };
  let marsh: [number, number] | null = null;
  for (let ring = 1; ring < 24 && !marsh; ring++)
    for (let dy = -ring; dy <= ring && !marsh; dy++)
      for (let dx = -ring; dx <= ring && !marsh; dx++)
        if (terrain.at(home[0] + dx, home[1] + dy) === TerrainTileKind.NutrientMarsh && !fissureNear(home[0] + dx, home[1] + dy))
          marsh = [home[0] + dx, home[1] + dy];
  r.handlePointer({ kind: "click", button: "right", x: marsh![0] + 0.5, y: marsh![1] + 0.5 });
  r.handleKey(key("1", { ctrl: true }));
  await c.idle();

  // 双击突击兵选同类 → Q + 左键进攻地图中心
  r.handlePointer({ kind: "click", button: "left", x: sp[0], y: sp[1], double: true });
  expect([...human.ui.selection]).toEqual([striker.id]);
  r.handleKey(key("q"));
  r.handlePointer({ kind: "click", button: "left", x: 128.5, y: 128.5 });
  await c.idle();
  r.handleKey(key("Backspace"));
  r.handleKey(key("m"));

  await human.client.advance(30);
  await stepB(); // 发起孵化槽建造请求
  await stepA(); // 已被接管：0x8001

  await human.client.advance(300);
  // 左键选中自家孵化槽 → HUD 排队 2 只工蜂
  r.handlePointer({ kind: "click", button: "left", x: vatAt[0] + 1.5, y: vatAt[1] + 1.5 });
  const vatId = human.ui.selectedBuilding!;
  await c.queueUnit(vatId, "unit.forager", 2);
  await stepB(); // 排队工蜂

  // Alt+A 交还控制 → agent:p0 自动恢复租约
  r.handleKey(key("a", { alt: true }));
  await c.idle();
  await Promise.resolve();

  await human.client.advance(300);
  await stepB(); // 进攻意图
  await stepA(); // 恢复后继续生产
  await human.client.advance(900);
  await stepB();
  await human.client.advance(900);

  return { human, agentA, agentB, botA, botB, toolResponses };
}
