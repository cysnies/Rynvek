// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `ArenaController`：把 `InputActionRouter` 的 RTS / 摄像机 / 全局逻辑动作翻译成界面状态变化与权威命令。
 *
 * 指针坐标约定为**世界 Tile 浮点坐标**（屏幕 → 世界的反投影由渲染层完成）。命令全部经 `ArenaClient`
 * 发送（附 Fencing Token、计入 600 APM）；被拒绝时把错误码写入 `ui.notices`，不抛出到输入线程。
 *
 * 右键情境指令（动作 19）判定顺序（ERRATA E-036）：
 * 1. 指针 1 Tile 内有可见敌方单位或建筑 → `AttackMove`；
 * 2. 选中的全是采集工蜂且目标为富营养沼泽 → `Harvest`；
 * 3. 其余 → `Move`。
 */

import {
  buildingSpec,
  rawToFloat,
  tileCenterRaw,
  TerrainTileKind,
  type ArenaClient,
  type BuildingKind,
  type CommandAck,
  type FieldId,
  type InputActionRouter,
  type OrderKind,
  type RtsCommand,
  type TechId,
  type UnitKind,
  type UnitView,
} from "@rynvek/client";

import { CAMERA_PAN_TILES, CAMERA_ZOOM_MAX, CAMERA_ZOOM_MIN, nexusCenter, type ArenaUiState } from "./state.js";

/** 点选半径（Tile） */
export const PICK_RADIUS_TILES = 1;

type Point = readonly [number, number];

export class ArenaController {
  private readonly inflight = new Set<Promise<unknown>>();
  private readonly unsubscribers: (() => void)[] = [];

  constructor(
    readonly client: ArenaClient,
    readonly router: InputActionRouter,
    readonly ui: ArenaUiState,
  ) {
    const on = (id: Parameters<InputActionRouter["on"]>[0], f: Parameters<InputActionRouter["on"]>[1]) =>
      this.unsubscribers.push(router.on(id, f));
    on("global.command_palette", () => (ui.commandPaletteOpen = !ui.commandPaletteOpen));
    on("global.cancel_or_menu", () => this.cancel());
    on("global.toggle_inspector", () => (ui.inspectorVisible = !ui.inspectorVisible));
    on("global.toggle_minimap", () => (ui.minimapVisible = !ui.minimapVisible));
    on("global.toggle_neural_overlay", () => (ui.neuralOverlay = !ui.neuralOverlay));
    on("global.toggle_field_overlay", () => (ui.fieldOverlay = !ui.fieldOverlay));
    on("camera.pan_up", () => this.pan(0, (ui.camera.yDown ? -1 : 1) * CAMERA_PAN_TILES));
    on("camera.pan_down", () => this.pan(0, (ui.camera.yDown ? 1 : -1) * CAMERA_PAN_TILES));
    on("camera.pan_left", () => this.pan(-CAMERA_PAN_TILES, 0));
    on("camera.pan_right", () => this.pan(CAMERA_PAN_TILES, 0));
    on("camera.zoom_in", () => this.zoom(2));
    on("camera.zoom_out", () => this.zoom(0.5));
    on("camera.jump_to_nexus", () => this.jumpToNexus());
    on("rts.select_primary", (inv) => this.primaryClick(inv.point!));
    on("rts.box_select", (inv) => this.boxSelect(inv.point!, inv.point2!));
    on("rts.select_add_toggle", (inv) => this.toggleSelect(inv.point!));
    on("rts.select_same_archetype", (inv) => this.selectSameArchetype(inv.point!));
    on("rts.context_order", (inv) => this.contextOrder(inv.point!));
    on("rts.order_attack_move", (inv) => this.order("AttackMove", inv.point!));
    on("rts.order_hold_position", () => this.hold());
    on("rts.order_patrol", (inv) => this.order("Patrol", inv.point!));
    on("rts.open_build_menu", () => (ui.buildMenuOpen = !ui.buildMenuOpen));
    on("rts.rotate_building_cw", () => (ui.buildRotation = (ui.buildRotation + 1) % 4));
    on("rts.demolish_selected", () => this.demolishSelected());
    on("rts.beacon_modulator_panel", () => this.openBeaconPanel());
    on("rts.release_human_lease", () => this.track(this.client.releaseLease().catch((e) => this.fail(e))));
    on("rts.assign_control_group_1_9", (inv) => ui.controlGroups.set(inv.group!, [...ui.selection].sort((a, b) => a - b)));
    on("rts.recall_control_group_1_9", (inv) => (ui.selection = new Set(ui.controlGroups.get(inv.group!) ?? [])));
    client.on((e) => {
      if (e.type === "observation") ui.prune(e.observation);
      if (e.type === "lease_revoked") ui.notify(0x8001, `控制权已被 ${e.by} 接管`);
    });
  }

  dispose(): void {
    for (const u of this.unsubscribers.splice(0)) u();
  }

  /** 等待所有已发出的命令得到回执 */
  async idle(): Promise<void> {
    while (this.inflight.size > 0) await Promise.allSettled([...this.inflight]);
  }

  private track<T>(p: Promise<T>): Promise<T> {
    this.inflight.add(p);
    void p.finally(() => this.inflight.delete(p)).catch(() => undefined);
    return p;
  }

  private fail(e: unknown): null {
    const code = typeof e === "object" && e !== null && "code" in e ? Number((e as { code: number }).code) : null;
    this.ui.notify(code, e instanceof Error ? e.message : String(e));
    return null;
  }

  /** 发送命令；失败写入提示并返回 `null` */
  send(cmd: RtsCommand): Promise<CommandAck | null> {
    return this.track(this.client.command(cmd).catch((e) => this.fail(e)));
  }

  private get obs() {
    return this.client.observation;
  }

  private ownUnits(): UnitView[] {
    const o = this.obs;
    return o ? o.units.filter((u) => u.faction === o.faction) : [];
  }

  private unitAt(p: Point, own: boolean): UnitView | null {
    const o = this.obs;
    if (!o) return null;
    let best: { u: UnitView; d: number } | null = null;
    for (const u of o.units) {
      if ((u.faction === o.faction) !== own) continue;
      const [x, y] = rawToFloat(u.position);
      const d = Math.hypot(x - p[0], y - p[1]);
      if (d <= PICK_RADIUS_TILES && (!best || d < best.d || (d === best.d && u.id < best.u.id))) best = { u, d };
    }
    return best?.u ?? null;
  }

  private buildingAt(p: Point, own: boolean) {
    const o = this.obs;
    if (!o) return null;
    const [x, y] = [Math.floor(p[0]), Math.floor(p[1])];
    return (
      o.buildings.find((b) => {
        if ((b.faction === o.faction) !== own) return false;
        const f = buildingSpec(b.kind).footprint;
        return x >= b.origin[0] && y >= b.origin[1] && x < b.origin[0] + f && y < b.origin[1] + f;
      }) ?? null
    );
  }

  // ---------------------------------------------------------------- 摄像机与全局

  private pan(dx: number, dy: number): void {
    this.ui.camera = { ...this.ui.camera, x: this.ui.camera.x + dx / this.ui.camera.zoom, y: this.ui.camera.y + dy / this.ui.camera.zoom };
  }

  private zoom(k: number): void {
    this.ui.camera = { ...this.ui.camera, zoom: Math.min(CAMERA_ZOOM_MAX, Math.max(CAMERA_ZOOM_MIN, this.ui.camera.zoom * k)) };
  }

  jumpToNexus(): void {
    const c = this.obs && nexusCenter(this.obs);
    if (c) this.ui.camera = { ...this.ui.camera, x: c[0], y: c[1] };
  }

  private cancel(): void {
    const ui = this.ui;
    if (ui.buildMode) ui.buildMode = null;
    else if (ui.buildMenuOpen) ui.buildMenuOpen = false;
    else if (ui.beaconPanelOpen) ui.beaconPanelOpen = false;
    else if (ui.commandPaletteOpen) ui.commandPaletteOpen = false;
    else {
      ui.selection.clear();
      ui.selectedBuilding = null;
    }
  }

  // ---------------------------------------------------------------- 选择

  private primaryClick(p: Point): void {
    if (this.ui.buildMode) {
      void this.placeAt(this.ui.buildMode, [Math.floor(p[0]), Math.floor(p[1])]);
      return;
    }
    const u = this.unitAt(p, true);
    if (u) {
      this.ui.selection = new Set([u.id]);
      this.ui.selectedBuilding = null;
      return;
    }
    const b = this.buildingAt(p, true);
    this.ui.selection.clear();
    this.ui.selectedBuilding = b?.id ?? null;
  }

  private toggleSelect(p: Point): void {
    const u = this.unitAt(p, true);
    if (!u) return;
    if (this.ui.selection.has(u.id)) this.ui.selection.delete(u.id);
    else this.ui.selection.add(u.id);
  }

  private selectSameArchetype(p: Point): void {
    const u = this.unitAt(p, true);
    if (!u) return;
    this.ui.selection = new Set(this.ownUnits().filter((x) => x.kind === u.kind).map((x) => x.id));
    this.ui.selectedBuilding = null;
  }

  private boxSelect(a: Point, b: Point): void {
    const [x0, x1] = [Math.min(a[0], b[0]), Math.max(a[0], b[0])];
    const [y0, y1] = [Math.min(a[1], b[1]), Math.max(a[1], b[1])];
    this.ui.selection = new Set(
      this.ownUnits()
        .filter((u) => {
          const [x, y] = rawToFloat(u.position);
          return x >= x0 && x <= x1 && y >= y0 && y <= y1;
        })
        .map((u) => u.id),
    );
    this.ui.selectedBuilding = null;
  }

  // ---------------------------------------------------------------- 指令

  private selectedIds(): number[] {
    return [...this.ui.selection].sort((a, b) => a - b);
  }

  order(order: OrderKind, p: Point): Promise<CommandAck | null> | null {
    const ids = this.selectedIds();
    if (ids.length === 0) return null;
    return this.send({
      type: "issue_order",
      units: { kind: "ids", value: ids },
      order,
      target: tileCenterRaw(Math.floor(p[0]), Math.floor(p[1])),
    });
  }

  private contextOrder(p: Point): void {
    if (this.ui.buildMode) {
      this.ui.buildMode = null; // 右键取消建造
      return;
    }
    const ids = this.selectedIds();
    if (ids.length === 0) return;
    const enemy = this.unitAt(p, false) ?? this.buildingAt(p, false);
    const terrain = this.client.terrain;
    const own = this.ownUnits().filter((u) => this.ui.selection.has(u.id));
    const allForagers = own.length > 0 && own.every((u) => u.kind === "unit.forager");
    const tx = Math.floor(p[0]);
    const ty = Math.floor(p[1]);
    const marsh = terrain?.inBounds(tx, ty) && terrain.at(tx, ty) === TerrainTileKind.NutrientMarsh;
    void this.order(enemy ? "AttackMove" : allForagers && marsh ? "Harvest" : "Move", p);
  }

  private hold(): void {
    const own = this.ownUnits().filter((u) => this.ui.selection.has(u.id));
    if (own.length === 0) return;
    // 驻守点取选中单位的质心所在 Tile
    const cx = own.reduce((s, u) => s + rawToFloat(u.position)[0], 0) / own.length;
    const cy = own.reduce((s, u) => s + rawToFloat(u.position)[1], 0) / own.length;
    void this.order("HoldPosition", [cx, cy]);
  }

  // ---------------------------------------------------------------- 建造与经济

  /** 底部建造抽屉中选择建筑 → 进入建造模式 */
  chooseBuilding(kind: BuildingKind): void {
    this.ui.buildMode = kind;
    this.ui.buildMenuOpen = false;
  }

  placeAt(kind: BuildingKind, origin: readonly [number, number]): Promise<CommandAck | null> {
    this.ui.buildMode = null;
    return this.send({ type: "place_building", kind, origin: [origin[0], origin[1]] });
  }

  private demolishSelected(): void {
    if (this.ui.selectedBuilding !== null) void this.send({ type: "demolish", building: this.ui.selectedBuilding });
  }

  private openBeaconPanel(): void {
    const b = this.obs?.buildings.find((x) => x.id === this.ui.selectedBuilding);
    this.ui.beaconPanelOpen = b?.kind === "bld.signal_beacon";
  }

  /** 信标调制面板提交（强度与调质均为 milli） */
  configureBeacon(
    beacon: number,
    channel: FieldId,
    strengthMilli: number,
    modulatorsMilli: readonly [number, number, number, number],
  ): Promise<CommandAck | null> {
    return this.send({ type: "configure_beacon", beacon, channel, strength_milli: strengthMilli, modulators_milli: modulatorsMilli });
  }

  /** 孵化槽排队；缺省基因组为该原型的赛季种子 `seed.<名>.v1` */
  queueUnit(vat: number, unit: UnitKind, count = 1, genomeSlug?: string): Promise<CommandAck | null> {
    const slug = genomeSlug ?? defaultGenome(unit, this.obs?.genomes ?? []);
    return this.send({ type: "queue_unit", vat, unit, genome_slug: slug, count });
  }

  /** 研发科技：Tier 1 在突触精炼厂，Tier 2/3 经演化坩埚 */
  research(site: number, tech: TechId, tier: 1 | 2 | 3): Promise<CommandAck | null> {
    return this.send(
      tier === 1 ? { type: "research_tech", site, tech } : { type: "crucible", crucible: site, action: { research_tech: tech } },
    );
  }

  breed(crucible: number, unit: UnitKind, mutationRatePermille = 50): Promise<CommandAck | null> {
    return this.send({ type: "crucible", crucible, action: { breed_elite_genome: { unit, mutation_rate_permille: mutationRatePermille } } });
  }
}

/** `unit.forager` → `seed.forager.v1`（若观测中有更新的同原型基因组，仍以种子为缺省，避免误用实验谱系） */
export function defaultGenome(unit: UnitKind, genomes: readonly string[]): string {
  const seed = `seed.${unit.slice("unit.".length)}.v1`;
  return genomes.includes(seed) || genomes.length === 0 ? seed : (genomes.find((g) => g.includes(unit.slice(5))) ?? seed);
}
