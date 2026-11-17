// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Arena 结构化生产 / 科技 / 信标操作面板。
 *
 * 视图完全由最近一次权威阵营观测与当前选中的己方建筑推导，不维护本地账本或乐观队列。
 * 发出的命令走同一条 `ArenaController.send` 链；回放或不可写入时所有提交控件禁用。
 */

import { defaultGenome } from "@rynvek/arena";
import {
  BUILDING_SPECS,
  TECH_SPECS,
  UNIT_SPECS,
  VAT_QUEUE_LIMIT,
  canAfford,
  techAvailable,
  unitUnlocked,
  type BuildingKind,
  type BuildingView,
  type FactionObservation,
  type FieldId,
  type ResourceAmounts,
  type RtsCommand,
  type TechId,
  type UnitKind,
} from "@rynvek/client";

const BEACON_CHANNELS: readonly FieldId[] = ["field.signal_a", "field.signal_b"];
const STRENGTH_MIN = -50_000;
const STRENGTH_MAX = 50_000;
const MODULATOR_MIN = -1_000;
const MODULATOR_MAX = 1_000;
const MUTATION_MIN = 10;
const MUTATION_MAX = 250;
const QUEUE_COUNT_MIN = 1;
const QUEUE_COUNT_MAX = 5;

/** Q8.8 整型转命令用毫单位：`milli = q8 * 1000 / 256`（向零截断，与服务器整数除法一致）。 */
export function q8ToMilli(q8: number): number {
  if (!Number.isInteger(q8)) {
    throw new RangeError("Q8.8 观测必须是整数");
  }
  return Math.trunc((q8 * 1000) / 256);
}

export function buildingNameZh(kind: BuildingKind): string {
  return BUILDING_SPECS.find((s) => s.kind === kind)?.nameZh ?? kind;
}

export function unitNameZh(kind: UnitKind): string {
  return UNIT_SPECS.find((s) => s.kind === kind)?.nameZh ?? kind;
}

export function techNameZh(id: TechId): string {
  return TECH_SPECS.find((s) => s.id === id)?.nameZh ?? id;
}

export function formatMilli(milli: number): string {
  const value = milli / 1000;
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

export function formatCost(cost: ResourceAmounts): string {
  return `生物质 ${formatMilli(cost.biomass)} / 通量 ${formatMilli(cost.flux)} / 神经凝胶 ${formatMilli(cost.neurogel)}`;
}

export function fieldLabel(field: FieldId): string {
  if (field === "field.signal_a") return "信号场 A";
  if (field === "field.signal_b") return "信号场 B";
  return field;
}

/** 观测到的基因组 slug 加上该兵种缺省种子。无法仅凭 slug 过滤原型，错配由服务器拒绝。 */
export function genomesForUnit(unit: UnitKind, observed: readonly string[]): string[] {
  const seed = defaultGenome(unit, observed);
  const out = [seed];
  for (const slug of observed) {
    if (slug !== seed) out.push(slug);
  }
  return out;
}

export interface HatchableRow {
  readonly kind: UnitKind;
  readonly nameZh: string;
  readonly cost: ResourceAmounts;
  readonly unlocked: boolean;
  readonly affordable: boolean;
  readonly genomes: readonly string[];
  readonly defaultGenome: string;
}

export interface TechRow {
  readonly id: TechId;
  readonly nameZh: string;
  readonly tier: 1 | 2 | 3;
  readonly cost: ResourceAmounts;
  readonly available: boolean;
  readonly affordable: boolean;
  readonly researched: boolean;
}

export interface RosterBuilding {
  readonly id: number;
  readonly kind: BuildingKind;
  readonly nameZh: string;
  readonly complete: boolean;
  readonly selected: boolean;
}

export interface CommandPanelView {
  readonly writable: boolean;
  readonly roster: readonly RosterBuilding[];
  readonly selection: null | {
    readonly id: number;
    readonly kind: BuildingKind;
    readonly nameZh: string;
    readonly complete: boolean;
    readonly powered: boolean;
    readonly hpMilli: number;
    readonly maxHpMilli: number;
    readonly buildProgress: number;
    readonly canDemolish: boolean;
    readonly foreign: boolean;
  };
  readonly production: null | {
    readonly queue: readonly { readonly unit: UnitKind; readonly nameZh: string }[];
    readonly limit: number;
    readonly remaining: number;
    readonly units: readonly HatchableRow[];
  };
  readonly laboratory: null | {
    readonly site: "refinery" | "crucible";
    readonly current: null | { readonly tech: TechId; readonly nameZh: string; readonly remainingTicks: number | null };
    readonly breeding: null | { readonly unit: UnitKind; readonly nameZh: string; readonly remainingTicks: number };
    readonly busy: boolean;
    readonly techs: readonly TechRow[];
    readonly breedUnits: readonly { readonly kind: UnitKind; readonly nameZh: string }[];
  };
  readonly beacon: null | {
    readonly field: FieldId;
    readonly fieldLabel: string;
    readonly committedStrengthMilli: number;
    readonly committedModulatorsMilli: readonly [number, number, number, number];
  };
}

export function buildCommandPanelView(
  observation: FactionObservation | null,
  selectedId: number | null,
  writable: boolean,
): CommandPanelView {
  const roster = ownRoster(observation, selectedId);
  if (!observation || selectedId === null) {
    return { writable, roster, selection: null, production: null, laboratory: null, beacon: null };
  }
  const building = observation.buildings.find((b) => b.id === selectedId) ?? null;
  if (!building) {
    return { writable, roster, selection: null, production: null, laboratory: null, beacon: null };
  }
  const foreign = building.faction !== observation.faction;
  const selection = {
    id: building.id,
    kind: building.kind,
    nameZh: buildingNameZh(building.kind),
    complete: building.complete,
    powered: building.powered,
    hpMilli: building.hp_milli,
    maxHpMilli: building.max_hp_milli,
    buildProgress: building.build_progress,
    canDemolish: !foreign && building.kind !== "bld.nexus_core",
    foreign,
  };
  if (foreign) {
    return { writable: false, roster, selection, production: null, laboratory: null, beacon: null };
  }
  return {
    writable,
    roster,
    selection,
    production: productionView(observation, building),
    laboratory: laboratoryView(observation, building),
    beacon: beaconView(building),
  };
}

function completedKinds(observation: FactionObservation): BuildingKind[] {
  return observation.buildings.filter((b) => b.complete && b.faction === observation.faction).map((b) => b.kind);
}


function ownRoster(observation: FactionObservation | null, selectedId: number | null): RosterBuilding[] {
  if (!observation) return [];
  return observation.buildings
    .filter((b) => b.faction === observation.faction)
    .map((b) => ({
      id: b.id,
      kind: b.kind,
      nameZh: buildingNameZh(b.kind),
      complete: b.complete,
      selected: b.id === selectedId,
    }));
}

function productionView(observation: FactionObservation, building: BuildingView): CommandPanelView["production"] {
  if (building.kind !== "bld.spawning_vat") return null;
  const remaining = Math.max(0, VAT_QUEUE_LIMIT - building.queue.length);
  const units: HatchableRow[] = UNIT_SPECS.map((spec) => {
    const unlocked = unitUnlocked(spec.kind, observation.techs);
    const genomes = genomesForUnit(spec.kind, observation.genomes);
    return {
      kind: spec.kind,
      nameZh: spec.nameZh,
      cost: spec.cost,
      unlocked,
      affordable: canAfford(observation.ledger.balance, spec.cost),
      genomes,
      defaultGenome: defaultGenome(spec.kind, observation.genomes),
    };
  });
  return {
    queue: building.queue.map((unit) => ({ unit, nameZh: unitNameZh(unit) })),
    limit: VAT_QUEUE_LIMIT,
    remaining,
    units,
  };
}

function laboratoryView(observation: FactionObservation, building: BuildingView): CommandPanelView["laboratory"] {
  const site = building.kind === "bld.synapse_refinery" ? "refinery" : building.kind === "bld.mutation_crucible" ? "crucible" : null;
  if (!site) return null;
  const completed = completedKinds(observation);
  const techs: TechRow[] = TECH_SPECS.filter((t) => t.researchSite === building.kind).map((t) => {
    const researched = observation.techs.includes(t.id);
    return {
      id: t.id,
      nameZh: t.nameZh,
      tier: t.tier,
      cost: t.cost,
      available: techAvailable(t.id, observation.techs, completed),
      affordable: canAfford(observation.ledger.balance, t.cost),
      researched,
    };
  });
  const remaining = building.research_remaining_ticks ?? null;
  const current = building.research
    ? { tech: building.research, nameZh: techNameZh(building.research), remainingTicks: remaining }
    : null;
  const breeding = building.breeding
    ? {
        unit: building.breeding.unit,
        nameZh: unitNameZh(building.breeding.unit),
        remainingTicks: building.breeding.remaining_ticks,
      }
    : null;
  const breedUnits =
    site === "crucible"
      ? UNIT_SPECS.map((s) => ({ kind: s.kind, nameZh: s.nameZh }))
      : [];
  return {
    site,
    current,
    breeding,
    busy: current !== null || breeding !== null,
    techs,
    breedUnits,
  };
}

function beaconView(building: BuildingView): CommandPanelView["beacon"] {
  if (building.kind !== "bld.signal_beacon") return null;
  const beacon = building.beacon;
  const field: FieldId = beacon?.channel && isBeaconChannel(beacon.channel) ? beacon.channel : "field.signal_a";
  const mods = beacon?.modulators ?? [0, 0, 0, 0];
  return {
    field,
    fieldLabel: fieldLabel(field),
    committedStrengthMilli: q8ToMilli(beacon?.strength_q8 ?? 0),
    committedModulatorsMilli: [
      q8ToMilli(mods[0] ?? 0),
      q8ToMilli(mods[1] ?? 0),
      q8ToMilli(mods[2] ?? 0),
      q8ToMilli(mods[3] ?? 0),
    ],
  };
}

export function isBeaconChannel(field: string): field is FieldId {
  return (BEACON_CHANNELS as readonly string[]).includes(field);
}

export interface CommandPanelHooks {
  send: (cmd: RtsCommand) => Promise<unknown>;
  onSelect?: (buildingId: number) => void;
  onStatus?: (text: string, ok: boolean) => void;
}

export interface CommandPanelHandle {
  update(view: CommandPanelView): void;
}

interface FormStash {
  genomes?: Partial<Record<UnitKind, string>> | undefined;
  count?: string | undefined;
  mutation?: string | undefined;
  breedUnit?: string | undefined;
  channel?: string | undefined;
  strength?: string | undefined;
  modulators?: [string, string, string, string] | undefined;
}

export function mountCommandPanel(root: HTMLElement, hooks: CommandPanelHooks): CommandPanelHandle {
  let view: CommandPanelView = { writable: false, roster: [], selection: null, production: null, laboratory: null, beacon: null };
  let lastBuildingId: number | null = null;
  let sending = false;

  root.addEventListener("click", (event) => {
    const btn = (event.target as HTMLElement | null)?.closest<HTMLElement>("[data-action]");
    if (!btn || btn.hasAttribute("disabled")) return;
    event.preventDefault();
    if (btn.dataset.action === "select-building") {
      const id = Number(btn.dataset.buildingId);
      if (Number.isInteger(id)) hooks.onSelect?.(id);
      return;
    }
    if (sending) return;
    void submit(btn);
  });

  root.addEventListener("focusout", () => queueMicrotask(() => {
    if (!root.contains(document.activeElement)) render(true);
  }));

  function status(text: string, ok: boolean): void {
    hooks.onStatus?.(text, ok);
  }

  function readStash(): FormStash {
    const genomes: Partial<Record<UnitKind, string>> = {};
    for (const select of root.querySelectorAll<HTMLSelectElement>("[data-field='genome'][data-unit]")) {
      genomes[select.dataset.unit as UnitKind] = select.value;
    }
    const c = root.querySelector<HTMLInputElement>("[data-field='count']");
    const m = root.querySelector<HTMLInputElement>("[data-field='mutation']");
    const u = root.querySelector<HTMLSelectElement>("[data-field='breed-unit']");
    const ch = root.querySelector<HTMLSelectElement>("[data-field='channel']");
    const s = root.querySelector<HTMLInputElement>("[data-field='strength']");
    const mods: [string, string, string, string] = [
      root.querySelector<HTMLInputElement>("[data-field='modulator-0']")?.value ?? "",
      root.querySelector<HTMLInputElement>("[data-field='modulator-1']")?.value ?? "",
      root.querySelector<HTMLInputElement>("[data-field='modulator-2']")?.value ?? "",
      root.querySelector<HTMLInputElement>("[data-field='modulator-3']")?.value ?? "",
    ];
    return {
      genomes,
      count: c?.value,
      mutation: m?.value,
      breedUnit: u?.value,
      channel: ch?.value,
      strength: s?.value,
      modulators: mods,
    };
  }

  function render(keepStash: boolean): void {
    const stash = keepStash ? readStash() : {};
    const { selection, writable } = view;
    const disable = !writable || sending;
    const parts: string[] = [];
    parts.push(`<h2>生产 / 科技 / 信标</h2>`);
    parts.push(
      `<p class="muted">根据当前阵营观测操作选中的己方建筑。造价与解锁仅作提示，不预扣资源；服务器回执为准。</p>`,
    );
    if (view.roster.length) {
      parts.push(`<div class="cmd-roster" id="building-roster">${view.roster.map((b) => {
        const state = b.complete ? "完工" : "建造中";
        const sel = b.selected ? " selected" : "";
        return `<button type="button" class="cmd-roster-item${sel}" data-action="select-building" data-building-id="${b.id}">${escapeHtml(b.nameZh)} #${b.id} · ${state}</button>`;
      }).join("")}</div>`);
    }
    if (!selection) {
      parts.push(`<p id="command-selection" class="muted">点击地图或左侧建筑列表选中己方建筑。</p>`);
    } else {
      const state = selection.complete ? (selection.powered ? "完工·通电" : "完工·未通电") : `建造中 ${selection.buildProgress}%`;
      const foreign = selection.foreign ? "（非己方，只读）" : "";
      parts.push(
        `<p id="command-selection"><strong>${escapeHtml(selection.nameZh)}</strong> #${selection.id} · ${escapeHtml(state)}${foreign}</p>`,
      );
    }
    if (view.production) parts.push(renderProduction(view.production, disable, !selection?.complete, stash));
    if (view.laboratory) parts.push(renderLaboratory(view.laboratory, disable, !selection?.complete, stash));
    if (view.beacon) parts.push(renderBeacon(view.beacon, disable, stash));
    if (selection && !selection.foreign) {
      const demoOff = disable || !selection.canDemolish;
      const demoHint = selection.kind === "bld.nexus_core" ? "主脑不可拆除" : "拆除返还 50% 造价，队列项全额退款";
      parts.push(`<div class="cmd-demolish">
        <button type="button" data-action="demolish" ${demoOff ? "disabled" : ""}>拆除选中建筑</button>
        <span class="muted">${escapeHtml(demoHint)}</span>
      </div>`);
    }
    root.innerHTML = parts.join("");
  }

  async function submit(btn: HTMLElement): Promise<void> {
    const action = btn.dataset.action;
    const sel = view.selection;
    if (!action || !sel || !view.writable) {
      status("当前不可写入命令（未连接、回放或未选中己方建筑）", false);
      return;
    }
    const cmd = buildCommand(action, btn, sel.id);
    if (!cmd) return;
    sending = true;
    render(true);
    try {
      const ack = await hooks.send(cmd);
      if (ack && typeof ack === "object" && "type" in ack && (ack as { type: unknown }).type) {
        status(`已提交：${String((ack as { type: unknown }).type)}`, true);
      } else {
        status("命令未确认成功；请查看权威拒绝或连接状态", false);
      }
    } catch (err) {
      status(err instanceof Error ? err.message : String(err), false);
    } finally {
      sending = false;
      render(true);
    }
  }

  function buildCommand(action: string, btn: HTMLElement, id: number): RtsCommand | null {
    if (action === "queue") {
      const unit = btn.dataset.unit as UnitKind | undefined;
      const prod = view.production;
      if (!unit || !prod) return null;
      const count = integerInput(root, "count", QUEUE_COUNT_MIN, Math.min(QUEUE_COUNT_MAX, prod.remaining), "排队数量");
      if (count === null) return null;
      if (prod.remaining < count) {
        status(`队列剩余空位 ${prod.remaining}，无法一次排入 ${count} 个`, false);
        return null;
      }
      const genome = root.querySelector<HTMLSelectElement>(`[data-field='genome'][data-unit='${unit}']`)?.value
        || prod.units.find((u) => u.kind === unit)?.defaultGenome
        || defaultGenome(unit, []);
      return { type: "queue_unit", vat: id, unit, genome_slug: genome, count };
    }
    if (action === "cancel-queue") {
      if (!view.production || view.production.queue.length === 0) {
        status("队列为空，无需取消", false);
        return null;
      }
      return { type: "cancel_queue", vat: id };
    }
    if (action === "research") {
      const tech = btn.dataset.tech as TechId | undefined;
      const lab = view.laboratory;
      if (!tech || !lab) return null;
      if (lab.busy) {
        status("该站点正在科研或繁育，需等待完成", false);
        return null;
      }
      if (lab.site === "refinery") {
        return { type: "research_tech", site: id, tech };
      }
      return { type: "crucible", crucible: id, action: { research_tech: tech } };
    }
    if (action === "breed") {
      const lab = view.laboratory;
      if (!lab || lab.site !== "crucible") return null;
      if (lab.busy) {
        status("该站点正在科研或繁育，需等待完成", false);
        return null;
      }
      const unit = root.querySelector<HTMLSelectElement>("[data-field='breed-unit']")?.value as UnitKind | undefined;
      if (!unit) {
        status("请选择要定向繁育的兵种", false);
        return null;
      }
      const mutation = integerInput(root, "mutation", MUTATION_MIN, MUTATION_MAX, "突变率（‰）");
      if (mutation === null) return null;
      return { type: "crucible", crucible: id, action: { breed_elite_genome: { unit, mutation_rate_permille: mutation } } };
    }
    if (action === "configure-beacon") {
      if (!view.beacon) return null;
      const channelRaw = root.querySelector<HTMLSelectElement>("[data-field='channel']")?.value ?? view.beacon.field;
      if (!isBeaconChannel(channelRaw)) {
        status("信标只能注入 field.signal_a / field.signal_b", false);
        return null;
      }
      const strength = integerInput(root, "strength", STRENGTH_MIN, STRENGTH_MAX, "场强（毫）");
      if (strength === null) return null;
      const modulators: [number, number, number, number] = [0, 0, 0, 0];
      for (let i = 0; i < 4; i += 1) {
        const v = integerInput(root, `modulator-${i}`, MODULATOR_MIN, MODULATOR_MAX, `调制器 ${i + 1}（毫）`);
        if (v === null) return null;
        modulators[i] = v;
      }
      return {
        type: "configure_beacon",
        beacon: id,
        channel: channelRaw,
        strength_milli: strength,
        modulators_milli: modulators,
      };
    }
    if (action === "demolish") {
      if (!view.selection?.canDemolish) {
        status("主脑不可拆除", false);
        return null;
      }
      return { type: "demolish", building: id };
    }
    status(`未知操作：${action}`, false);
    return null;
  }

  function integerInput(rootEl: HTMLElement, field: string, min: number, max: number, label: string): number | null {
    const raw = rootEl.querySelector<HTMLInputElement>(`[data-field='${field}']`)?.value ?? "";
    const n = Number(raw);
    if (!Number.isInteger(n) || n < min || n > max) {
      status(`${label}须为 [${min}, ${max}] 内的整数`, false);
      return null;
    }
    return n;
  }

  return {
    update(next: CommandPanelView): void {
      const same = next.selection?.id === lastBuildingId;
      const editing = same && next.writable === view.writable && root.contains(document.activeElement)
        && document.activeElement?.matches("input, select");
      view = next;
      lastBuildingId = next.selection?.id ?? null;
      // 高频观察不能抢走正在编辑/展开的表单焦点；提交仍使用上面更新后的权威视图。
      if (!editing) render(same);
    },
  };
}

function renderProduction(prod: NonNullable<CommandPanelView["production"]>, disable: boolean, incomplete: boolean, stash: FormStash): string {
  const queueOff = disable || prod.queue.length === 0;
  const countMax = Math.max(QUEUE_COUNT_MIN, Math.min(QUEUE_COUNT_MAX, prod.remaining || QUEUE_COUNT_MAX));
  const count = stash.count ?? "1";
  const queueText = prod.queue.length
    ? prod.queue.map((j, i) => `${i + 1}. ${escapeHtml(j.nameZh)}`).join("、")
    : "空";
  const rows = prod.units
    .map((u) => {
      const hints: string[] = [];
      if (!u.unlocked) hints.push("科技未解锁");
      if (!u.affordable) hints.push("资源不足（提示）");
      if (prod.remaining <= 0) hints.push("队列已满");
      const off = disable || incomplete || prod.remaining <= 0;
      const selectedGenome = stash.genomes?.[u.kind] ?? u.defaultGenome;
      const options = u.genomes.map(g => `<option value="${escapeHtml(g)}" ${g === selectedGenome ? "selected" : ""}>${escapeHtml(g)}</option>`).join("");
      return `<tr>
        <td>${escapeHtml(u.nameZh)}<label>基因组 <select data-field="genome" data-unit="${escapeHtml(u.kind)}" aria-label="${escapeHtml(u.nameZh)}基因组" ${disable ? "disabled" : ""}>${options}</select></label></td>
        <td>${escapeHtml(formatCost(u.cost))}</td>
        <td>${hints.length ? escapeHtml(hints.join(" · ")) : "可排"}</td>
        <td><button type="button" data-action="queue" data-unit="${escapeHtml(u.kind)}" ${off ? "disabled" : ""}>排队生产</button></td>
      </tr>`;
    })
    .join("");
  const incompleteHint = incomplete ? `<p class="cmd-warn">孵化槽尚未完工，无法排队。</p>` : "";
  return `<fieldset class="cmd-block" data-panel="production">
    <legend>生产队列（${prod.queue.length}/${prod.limit}）</legend>
    <p>当前队列：${queueText}</p>
    ${incompleteHint}
    <div class="cmd-row">
      <label>数量 <input data-field="count" type="number" min="${QUEUE_COUNT_MIN}" max="${countMax}" value="${escapeHtml(count)}" ${disable ? "disabled" : ""}></label>
      <button type="button" data-action="cancel-queue" ${queueOff ? "disabled" : ""}>取消队尾（全额退款）</button>
    </div>
    <table class="cmd-table"><thead><tr><th>兵种</th><th>单件造价</th><th>提示</th><th></th></tr></thead><tbody>${rows}</tbody></table>
  </fieldset>`;
}

function renderLaboratory(lab: NonNullable<CommandPanelView["laboratory"]>, disable: boolean, incomplete: boolean, stash: FormStash): string {
  const siteName = lab.site === "refinery" ? "突触精炼厂 · 科研" : "演化坩埚 · 科研与定向繁育";
  const busy: string[] = [];
  if (lab.current) {
    const ticks = lab.current.remainingTicks === null ? "进行中" : `剩余 ${lab.current.remainingTicks} tick`;
    busy.push(`正在研发 ${escapeHtml(lab.current.nameZh)}（${ticks}）`);
  }
  if (lab.breeding) {
    busy.push(`正在定向繁育 ${escapeHtml(lab.breeding.nameZh)}（剩余 ${lab.breeding.remainingTicks} tick）`);
  }
  const rows = lab.techs
    .map((t) => {
      const hints: string[] = [];
      if (t.researched) hints.push("已研发");
      else if (!t.available) hints.push("前置未满足");
      if (!t.affordable && !t.researched) hints.push("资源不足（提示）");
      if (lab.busy) hints.push("站点忙碌");
      const off = disable || incomplete || lab.busy || t.researched || !t.available;
      return `<tr>
        <td>${escapeHtml(t.nameZh)}</td>
        <td>T${t.tier}</td>
        <td>${escapeHtml(formatCost(t.cost))}</td>
        <td>${hints.length ? escapeHtml(hints.join(" · ")) : "可研发"}</td>
        <td><button type="button" data-action="research" data-tech="${escapeHtml(t.id)}" ${off ? "disabled" : ""}>开始科研</button></td>
      </tr>`;
    })
    .join("");
  let breed = "";
  if (lab.site === "crucible") {
    const unitVal = stash.breedUnit ?? lab.breedUnits[0]?.kind ?? "";
    const opts = lab.breedUnits
      .map((u) => `<option value="${escapeHtml(u.kind)}" ${u.kind === unitVal ? "selected" : ""}>${escapeHtml(u.nameZh)}</option>`)
      .join("");
    const mutation = stash.mutation ?? "50";
    const breedOff = disable || incomplete || lab.busy;
    breed = `<div class="cmd-row">
      <label>定向繁育兵种 <select data-field="breed-unit" ${breedOff ? "disabled" : ""}>${opts}</select></label>
      <label>突变率（‰，${MUTATION_MIN}–${MUTATION_MAX}） <input data-field="mutation" type="number" min="${MUTATION_MIN}" max="${MUTATION_MAX}" value="${escapeHtml(mutation)}" ${breedOff ? "disabled" : ""}></label>
      <button type="button" data-action="breed" ${breedOff ? "disabled" : ""}>开始定向繁育</button>
    </div>
    <p class="muted">突变率沿用既有定向繁育范围，不是自然繁殖覆盖。子代基因组只存在于服务器，观测只显示兵种与剩余 tick。</p>`;
  }
  const incompleteHint = incomplete ? `<p class="cmd-warn">建筑尚未完工，无法开始科研或繁育。</p>` : "";
  return `<fieldset class="cmd-block" data-panel="laboratory">
    <legend>${siteName}</legend>
    ${busy.length ? `<p>${busy.join("；")}</p>` : "<p>站点空闲</p>"}
    ${incompleteHint}
    <table class="cmd-table"><thead><tr><th>科技</th><th>层级</th><th>造价</th><th>提示</th><th></th></tr></thead><tbody>${rows}</tbody></table>
    ${breed}
  </fieldset>`;
}

function renderBeacon(beacon: NonNullable<CommandPanelView["beacon"]>, disable: boolean, stash: FormStash): string {
  const channel = stash.channel && isBeaconChannel(stash.channel) ? stash.channel : beacon.field;
  const opts = BEACON_CHANNELS.map(
    (c) => `<option value="${c}" ${c === channel ? "selected" : ""}>${escapeHtml(fieldLabel(c))}</option>`,
  ).join("");
  const strength = stash.strength ?? String(beacon.committedStrengthMilli);
  const modValues = [0, 1, 2, 3].map((i) => {
    const stashed = stash.modulators?.[i];
    return stashed !== undefined && stashed !== "" ? stashed : String(beacon.committedModulatorsMilli[i]);
  });
  const inputs = modValues
    .map((v, i) => `<label>M${i + 1} <input data-field="modulator-${i}" type="number" min="${MODULATOR_MIN}" max="${MODULATOR_MAX}" value="${escapeHtml(v)}" ${disable ? "disabled" : ""}></label>`)
    .join("");
  return `<fieldset class="cmd-block" data-panel="beacon">
    <legend>神经调制信标</legend>
    <p class="muted">已提交场强 ${beacon.committedStrengthMilli} 毫（${escapeHtml(beacon.fieldLabel)}）；表单按 Q8.8 观测换算预填，提交仍以服务器回执为准。</p>
    <div class="cmd-row">
      <label>注入通道 <select data-field="channel" ${disable ? "disabled" : ""}>${opts}</select></label>
      <label>场强（毫，${STRENGTH_MIN}–${STRENGTH_MAX}） <input data-field="strength" type="number" min="${STRENGTH_MIN}" max="${STRENGTH_MAX}" value="${escapeHtml(strength)}" ${disable ? "disabled" : ""}></label>
    </div>
    <div class="cmd-row">${inputs}</div>
    <button type="button" data-action="configure-beacon" ${disable ? "disabled" : ""}>提交信标配置</button>
  </fieldset>`;
}

function escapeHtml(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
