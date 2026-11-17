// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
// 结构化面板视图与提交形状的定向快验，不是浏览器或仿真验收。

import { afterEach, expect, it } from "vitest";
import {
  buildCommandPanelView,
  formatCost,
  mountCommandPanel,
  q8ToMilli,
  type CommandPanelView,
} from "../src/arena-command-panel.js";
import type { BuildingView, FactionObservation, ResourceAmounts, RtsCommand } from "@rynvek/client";

function amounts(biomass = 500, flux = 300, neurogel = 100): ResourceAmounts {
  return { biomass: biomass * 1000, flux: flux * 1000, neurogel: neurogel * 1000 };
}

function building(partial: Partial<BuildingView> & Pick<BuildingView, "id" | "kind">): BuildingView {
  return {
    faction: 0,
    origin: [4, 4],
    hp_milli: 1000,
    max_hp_milli: 1000,
    complete: true,
    powered: true,
    build_progress: 100,
    queue: [],
    research: null,
    beacon: null,
    ...partial,
  };
}

function observation(partial: Partial<FactionObservation> = {}): FactionObservation {
  return {
    tick: 8,
    faction: 0,
    ledger: { balance: amounts(), capacity: amounts(2000, 1000, 500), biomass_harvested_total: 0 },
    supply: [1, 24],
    techs: [],
    genomes: ["seed.forager.v1", "seed.striker.v1", "experimental.forager.v2"],
    units: [],
    buildings: [building({ id: 1, kind: "bld.nexus_core", origin: [0, 0] })],
    ghosts: [],
    visibility: [],
    visibility_width: 0,
    score_milli: 0,
    pve_wave: 1,
    outcome: null,
    ...partial,
  };
}

it("Q8.8 向零截断为毫单位，与服务器整数除法一致", () => {
  expect(q8ToMilli(256)).toBe(1000);
  expect(q8ToMilli(0)).toBe(0);
  expect(q8ToMilli(-256)).toBe(-1000);
  expect(q8ToMilli(1)).toBe(3);
  expect(q8ToMilli(-1)).toBe(-3);
  expect(() => q8ToMilli(1.5)).toThrow(/整数/);
});

it("造价提示使用平衡表毫单位换算，不把账本改成预扣后的值", () => {
  expect(formatCost(amounts(40, 20, 0))).toBe("生物质 40 / 通量 20 / 神经凝胶 0");
  const obs = observation({ ledger: { balance: amounts(500, 300, 100), capacity: amounts(2000, 1000, 500), biomass_harvested_total: 0 } });
  const vat = building({ id: 7, kind: "bld.spawning_vat", queue: ["unit.forager"] });
  const view = buildCommandPanelView({ ...obs, buildings: [...obs.buildings, vat] }, 7, true);
  expect(obs.ledger.balance.biomass).toBe(500_000);
  expect(view.production?.units.find((u) => u.kind === "unit.forager")?.affordable).toBe(true);
});

it("未选中时只有己方建筑名册，不伪造生产或信标状态", () => {
  const view = buildCommandPanelView(observation(), null, true);
  expect(view.selection).toBeNull();
  expect(view.production).toBeNull();
  expect(view.laboratory).toBeNull();
  expect(view.beacon).toBeNull();
  expect(view.roster).toEqual([expect.objectContaining({ id: 1, kind: "bld.nexus_core", selected: false, complete: true })]);
});

it("孵化槽列出队列、解锁与资源提示，并保留观测到的基因组加缺省种子", () => {
  const vat = building({ id: 7, kind: "bld.spawning_vat", queue: ["unit.forager", "unit.striker"] });
  const view = buildCommandPanelView(observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), vat] }), 7, true);
  expect(view.production?.queue.map((j) => j.unit)).toEqual(["unit.forager", "unit.striker"]);
  expect(view.production?.remaining).toBe(3);
  const forager = view.production?.units.find((u) => u.kind === "unit.forager");
  const carapace = view.production?.units.find((u) => u.kind === "unit.carapace");
  expect(forager?.unlocked).toBe(true);
  expect(forager?.defaultGenome).toBe("seed.forager.v1");
  expect(forager?.genomes).toEqual(["seed.forager.v1", "seed.striker.v1", "experimental.forager.v2"]);
  expect(carapace?.unlocked).toBe(false);
  expect(carapace?.affordable).toBe(true);
});

it("资源不足只作提示，不在视图里改账本或隐藏兵种", () => {
  const vat = building({ id: 7, kind: "bld.spawning_vat" });
  const obs = observation({
    ledger: { balance: amounts(0, 0, 0), capacity: amounts(2000, 1000, 500), biomass_harvested_total: 0 },
    buildings: [building({ id: 1, kind: "bld.nexus_core" }), vat],
  });
  const view = buildCommandPanelView(obs, 7, true);
  expect(view.production?.units.every((u) => !u.affordable)).toBe(true);
  expect(view.production?.units.length).toBeGreaterThan(0);
  expect(obs.ledger.balance).toEqual(amounts(0, 0, 0));
});

it("精炼厂只列出本站点科技，前置与已研由观测判定", () => {
  const refinery = building({ id: 3, kind: "bld.synapse_refinery" });
  const vat = building({ id: 7, kind: "bld.spawning_vat" });
  const bare = buildCommandPanelView(
    observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), refinery] }),
    3,
    true,
  );
  expect(bare.laboratory?.site).toBe("refinery");
  expect(bare.laboratory?.techs.every((t) => t.tier === 1)).toBe(true);
  expect(bare.laboratory?.techs.find((t) => t.id === "tech.chemotaxis_amp")?.available).toBe(false);
  expect(bare.laboratory?.breedUnits).toEqual([]);

  const ready = buildCommandPanelView(
    observation({
      techs: ["tech.chemotaxis_amp"],
      buildings: [building({ id: 1, kind: "bld.nexus_core" }), vat, refinery],
    }),
    3,
    true,
  );
  const chemo = ready.laboratory?.techs.find((t) => t.id === "tech.chemotaxis_amp");
  const flux = ready.laboratory?.techs.find((t) => t.id === "tech.flux_capacitance");
  expect(chemo?.researched).toBe(true);
  expect(chemo?.available).toBe(false);
  expect(flux?.available).toBe(false);
});

it("坩埚暴露科研剩余 tick 与繁育进度，且视图不含子代基因组字段", () => {
  const crucible = building({
    id: 9,
    kind: "bld.mutation_crucible",
    research: "tech.hebbian_plasticity",
    research_remaining_ticks: 88,
    breeding: { unit: "unit.striker", remaining_ticks: 12 },
  });
  const view = buildCommandPanelView(
    observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), crucible] }),
    9,
    true,
  );
  expect(view.laboratory?.busy).toBe(true);
  expect(view.laboratory?.current).toEqual({
    tech: "tech.hebbian_plasticity",
    nameZh: "四因子可塑性激活",
    remainingTicks: 88,
  });
  expect(view.laboratory?.breeding).toEqual({
    unit: "unit.striker",
    nameZh: "突击刺胞簇",
    remainingTicks: 12,
  });
  expect(view.laboratory?.breedUnits.some((u) => u.kind === "unit.forager")).toBe(true);
  expect(JSON.stringify(view.laboratory?.breeding)).not.toMatch(/genome|child/i);
});

it("信标把观测 Q8.8 换成毫单位，频道只认 signal_a/b", () => {
  const beacon = building({
    id: 5,
    kind: "bld.signal_beacon",
    beacon: { channel: "field.signal_b", strength_q8: 256, modulators: [256, 0, -256, 1] },
  });
  const view = buildCommandPanelView(
    observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), beacon] }),
    5,
    true,
  );
  expect(view.beacon).toEqual({
    field: "field.signal_b",
    fieldLabel: "信号场 B",
    committedStrengthMilli: 1000,
    committedModulatorsMilli: [1000, 0, -1000, 3],
  });
});

it("主脑不可拆；非己方建筑只读且不展开操作区", () => {
  const nexus = buildCommandPanelView(observation(), 1, true);
  expect(nexus.selection?.canDemolish).toBe(false);
  const enemy = building({ id: 8, faction: 1, kind: "bld.spawning_vat", queue: ["unit.forager"] });
  const view = buildCommandPanelView(observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), enemy] }), 8, true);
  expect(view.selection?.foreign).toBe(true);
  expect(view.writable).toBe(false);
  expect(view.production).toBeNull();
});

it("挂载后按观测提交真实命令，失败校验不发，回放禁用写入", async () => {
  const sent: RtsCommand[] = [];
  const root = document.createElement("section");
  document.body.append(root);
  const panel = mountCommandPanel(root, { send: async (cmd) => { sent.push(cmd); return { type: "queued", vat: 7, count: 1 }; } });
  const vat = building({ id: 7, kind: "bld.spawning_vat" });
  const obs = observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), vat] });
  let view: CommandPanelView = buildCommandPanelView(obs, 7, true);
  panel.update(view);
  (root.querySelector('[data-action="queue"][data-unit="unit.forager"]') as HTMLButtonElement).click();
  await Promise.resolve();
  await Promise.resolve();
  expect(sent).toEqual([{ type: "queue_unit", vat: 7, unit: "unit.forager", genome_slug: "seed.forager.v1", count: 1 }]);
  expect(obs.ledger.balance.biomass).toBe(500_000);
  expect(view.production?.queue).toEqual([]);

  sent.length = 0;
  const crucible = building({ id: 9, kind: "bld.mutation_crucible" });
  panel.update(buildCommandPanelView(observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), crucible] }), 9, true));
  (root.querySelector("[data-field='mutation']") as HTMLInputElement).value = "9";
  (root.querySelector('[data-action="breed"]') as HTMLButtonElement).click();
  await Promise.resolve();
  expect(sent).toEqual([]);
  (root.querySelector("[data-field='mutation']") as HTMLInputElement).value = "50";
  (root.querySelector('[data-action="breed"]') as HTMLButtonElement).click();
  await Promise.resolve();
  await Promise.resolve();
  expect(sent).toEqual([
    { type: "crucible", crucible: 9, action: { breed_elite_genome: { unit: "unit.forager", mutation_rate_permille: 50 } } },
  ]);

  sent.length = 0;
  const beacon = building({
    id: 5,
    kind: "bld.signal_beacon",
    beacon: { channel: "field.signal_a", strength_q8: 0, modulators: [0, 0, 0, 0] },
  });
  panel.update(buildCommandPanelView(observation({ buildings: [building({ id: 1, kind: "bld.nexus_core" }), beacon] }), 5, true));
  (root.querySelector("[data-field='strength']") as HTMLInputElement).value = "2000";
  (root.querySelector("[data-field='modulator-0']") as HTMLInputElement).value = "100";
  (root.querySelector("[data-field='channel']") as HTMLSelectElement).value = "field.signal_b";
  (root.querySelector('[data-action="configure-beacon"]') as HTMLButtonElement).click();
  await Promise.resolve();
  await Promise.resolve();
  expect(sent).toEqual([
    {
      type: "configure_beacon",
      beacon: 5,
      channel: "field.signal_b",
      strength_milli: 2000,
      modulators_milli: [100, 0, 0, 0],
    },
  ]);

  panel.update(buildCommandPanelView(observation(), 1, false));
  for (const btn of root.querySelectorAll<HTMLButtonElement>("[data-action]")) {
    if (btn.dataset.action !== "select-building") expect(btn.disabled).toBe(true);
  }
});

afterEach(() => {
  document.body.innerHTML = "";
});

it("排位生产按兵种选择缺省基因组，观察刷新不打断表单输入",async()=>{
 const sent:RtsCommand[]=[];const root=document.createElement("section");document.body.append(root);
 const panel=mountCommandPanel(root,{send:async cmd=>{sent.push(cmd);return {type:"queued"};}});
 const obs=observation({genomes:["seed.forager.v1","seed.striker.v1"],buildings:[building({id:7,kind:"bld.spawning_vat"})]});
 panel.update(buildCommandPanelView(obs,7,true));
 const count=root.querySelector<HTMLInputElement>("[data-field='count']")!;count.focus();count.value="2";
 panel.update(buildCommandPanelView({...obs,tick:obs.tick+1},7,true));expect(document.activeElement).toBe(count);expect(count.value).toBe("2");
 (root.querySelector('[data-action="queue"][data-unit="unit.striker"]') as HTMLButtonElement).click();await Promise.resolve();await Promise.resolve();
 expect(sent).toEqual([{type:"queue_unit",vat:7,unit:"unit.striker",genome_slug:"seed.striker.v1",count:2}]);
});
