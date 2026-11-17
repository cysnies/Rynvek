// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `rynvek-arena/1` 线上协议类型：与 Rust `rynvek_engine::arena` / `rynvek_rts` / `rynvek_agent` 的 serde 形态逐字段一致
 * （`tests/arena/opening.transcript.json` 夹具双向断言）。
 *
 * - 所有资源、生命、强度等数值均为 **milli 定点整数**；
 * - 坐标 `Vec2Raw` 为 Q32.32 原始值二元组（`|raw| < 2^53`，JavaScript `number` 无损）；
 * - 实体 ID 为十进制序号 `number`（工具调用中使用其文本形态）。
 */

export const ARENA_PROTOCOL = "rynvek-arena/1";

export type BuildingKind =
  | "bld.nexus_core"
  | "bld.biomass_harvester"
  | "bld.flux_reactor"
  | "bld.synapse_refinery"
  | "bld.spawning_vat"
  | "bld.signal_beacon"
  | "bld.pulse_turret"
  | "bld.mutation_crucible";

export type UnitKind =
  | "unit.forager"
  | "unit.striker"
  | "unit.carapace"
  | "unit.spitter"
  | "unit.weaver"
  | "unit.leviathan";

export type TechId =
  | "tech.chemotaxis_amp"
  | "tech.flux_capacitance"
  | "tech.kinetic_resonance"
  | "tech.multicellular_chitin"
  | "tech.hebbian_plasticity"
  | "tech.gap_junction_sync"
  | "tech.morphogenesis_apex"
  | "tech.metabolic_superconduct"
  | "tech.directed_speciation";

export type FieldId = "field.nutrient" | "field.energy" | "field.signal_a" | "field.signal_b" | "field.hazard";
export type OrderKind = "Move" | "AttackMove" | "Harvest" | "HoldPosition" | "Patrol";
export type PrincipalKind = "Human" | "Script" | "Agent";
export type ControlLevel = "Spectate" | "Perceive" | "Modulate" | "Produce" | "Sovereign";
export type MatchMode = "PvP_Ranked" | "PvE_Survival";
export type ArenaMapSize = "Duel_1v1" | "Skirmish_2v2" | "Arena_4P" | "Persistent_Sector";

/** 五级控制等级的序数（与 Rust `repr(u8)` 一致） */
export const CONTROL_LEVEL_ORDER: Readonly<Record<ControlLevel, number>> = {
  Spectate: 0,
  Perceive: 1,
  Modulate: 2,
  Produce: 3,
  Sovereign: 4,
};

/** Q32.32 原始值坐标 */
export type Vec2Raw = readonly [number, number];

export interface ResourceAmounts {
  readonly biomass: number;
  readonly flux: number;
  readonly neurogel: number;
}

// ------------------------------------------------------------------ 命令

export type UnitSelector =
  | { readonly kind: "ids"; readonly value: readonly number[] }
  | { readonly kind: "archetype"; readonly value: UnitKind | null };

export type CrucibleAction =
  | { readonly research_tech: TechId }
  | { readonly breed_elite_genome: { readonly unit: UnitKind; readonly mutation_rate_permille: number } };

export type RtsCommand =
  | { readonly type: "place_building"; readonly kind: BuildingKind; readonly origin: readonly [number, number] }
  | { readonly type: "demolish"; readonly building: number }
  | {
      readonly type: "queue_unit";
      readonly vat: number;
      readonly unit: UnitKind;
      readonly genome_slug: string;
      readonly count: number;
    }
  | { readonly type: "cancel_queue"; readonly vat: number }
  | { readonly type: "issue_order"; readonly units: UnitSelector; readonly order: OrderKind; readonly target: Vec2Raw }
  | {
      readonly type: "configure_beacon";
      readonly beacon: number;
      readonly channel: FieldId;
      readonly strength_milli: number;
      readonly modulators_milli: readonly [number, number, number, number];
    }
  | { readonly type: "research_tech"; readonly site: number; readonly tech: TechId }
  | { readonly type: "crucible"; readonly crucible: number; readonly action: CrucibleAction }
  | { readonly type: "surrender" };

export type CommandAck =
  | { readonly type: "building_placed"; readonly id: number }
  | { readonly type: "demolished"; readonly id: number }
  | { readonly type: "queued"; readonly vat: number; readonly count: number }
  | { readonly type: "cancelled"; readonly vat: number }
  | { readonly type: "order_issued"; readonly units: number }
  | { readonly type: "beacon_configured"; readonly id: number }
  | { readonly type: "research_started"; readonly site: number; readonly tech: TechId }
  | { readonly type: "breeding_started"; readonly crucible: number; readonly unit: UnitKind }
  | { readonly type: "surrendered" };

/** 命令所需控制等级（与 Rust `command_level` 一致） */
export function commandLevel(cmd: RtsCommand): ControlLevel {
  switch (cmd.type) {
    case "issue_order":
    case "configure_beacon":
      return "Modulate";
    case "demolish":
    case "surrender":
      return "Sovereign";
    default:
      return "Produce";
  }
}

// ------------------------------------------------------------------ 观测

export interface UnitView {
  readonly id: number;
  readonly faction: number;
  readonly kind: UnitKind;
  readonly position: Vec2Raw;
  readonly hp_milli: number;
  readonly max_hp_milli: number;
  readonly shield_milli: number;
  readonly order: OrderKind | null;
  readonly modulators: readonly [number, number, number, number] | null;
}

export interface BeaconConfig {
  readonly channel: FieldId;
  readonly strength_q8: number;
  readonly modulators: readonly [number, number, number, number];
}

export interface BuildingView {
  readonly id: number;
  readonly faction: number;
  readonly kind: BuildingKind;
  readonly origin: readonly [number, number];
  readonly hp_milli: number;
  readonly max_hp_milli: number;
  readonly complete: boolean;
  readonly powered: boolean;
  readonly build_progress: number;
  readonly queue: readonly UnitKind[];
  readonly research: TechId | null;
  /** 当前科研剩余 tick；缺省或 null 表示该建筑没有进行中的科研。 */
  readonly research_remaining_ticks?: number | null;
  /** 定向繁育进度。只含兵种与剩余 tick，不含子代基因组。 */
  readonly breeding?: BreedingView | null;
  readonly beacon: BeaconConfig | null;
}

/** 与 Rust `BreedingView` 对齐的公开繁育观测。 */
export interface BreedingView {
  readonly unit: UnitKind;
  readonly remaining_ticks: number;
}

export interface BuildingGhost {
  readonly entity_id: number;
  readonly faction: number;
  readonly kind: BuildingKind;
  readonly origin: readonly [number, number];
  readonly last_seen_tick: number;
}

export type VictoryReason = "elimination" | "surrender" | "tie_breaker_score" | "sudden_death" | "waves_cleared";

export type MatchOutcome =
  | { readonly result: "victory"; readonly winner: number; readonly reason: VictoryReason; readonly tick: number }
  | { readonly result: "draw"; readonly tick: number }
  | { readonly result: "defeat"; readonly tick: number; readonly waves_cleared: number };

export interface FactionObservation {
  readonly tick: number;
  readonly faction: number;
  readonly ledger: {
    readonly balance: ResourceAmounts;
    readonly capacity: ResourceAmounts;
    readonly biomass_harvested_total: number;
  };
  readonly supply: readonly [number, number];
  readonly techs: readonly TechId[];
  readonly genomes: readonly string[];
  readonly units: readonly UnitView[];
  readonly buildings: readonly BuildingView[];
  readonly ghosts: readonly BuildingGhost[];
  /** `4×4` 聚合可见性格：`1` 可见、`2` 已探索、`0` 未探索 */
  readonly visibility: readonly number[];
  readonly visibility_width: number;
  readonly score_milli: number;
  readonly pve_wave: number | null;
  readonly outcome: MatchOutcome | null;
}

export type RtsEvent =
  | { readonly type: "building_placed" | "building_completed" | "building_destroyed"; readonly id: number; readonly faction: number; readonly kind: BuildingKind }
  | { readonly type: "unit_spawned" | "unit_died"; readonly id: number; readonly faction: number; readonly kind: UnitKind }
  | { readonly type: "research_completed"; readonly faction: number; readonly tech: TechId }
  | { readonly type: "genome_bred"; readonly faction: number; readonly slug: string; readonly unit: UnitKind }
  | { readonly type: "wave_spawned"; readonly wave: number; readonly count: number }
  | { readonly type: "sudden_death_started"; readonly tick: number }
  | { readonly type: "match_ended"; readonly outcome: MatchOutcome };

// ------------------------------------------------------------------ 控制协议

export interface Principal {
  readonly id: string;
  readonly kind: PrincipalKind;
}

export interface ControlLease {
  readonly holder: Principal;
  readonly faction: number;
  readonly level: ControlLevel;
  readonly fencing_token: number;
  readonly issued_tick: number;
  readonly expires_tick: number;
}

export interface ErrorBody {
  readonly code: number;
  readonly name: string;
  readonly message: string;
}

export interface ToolSchema {
  readonly name: string;
  readonly description: string;
  readonly parameters: unknown;
}

export interface ToolResponse {
  readonly tool: string;
  readonly tick: number;
  readonly result?: unknown;
  readonly error?: ErrorBody;
}

export interface MapInfo {
  readonly size: ArenaMapSize;
  readonly width: number;
  readonly height: number;
  readonly seed: number;
  readonly tiles_hash: string;
  readonly terrain_b64: string;
  readonly spawn_tiles: readonly [readonly [number, number], readonly [number, number]];
}

// ------------------------------------------------------------------ 消息

export type ArenaClientMessage =
  | {
      readonly type: "hello";
      readonly protocol: string;
      readonly principal_id: string;
      readonly kind: PrincipalKind;
      readonly faction: number;
    }
  | { readonly type: "acquire_lease"; readonly seq: number; readonly level: ControlLevel }
  | { readonly type: "release_lease"; readonly seq: number; readonly fencing_token: number }
  | { readonly type: "command"; readonly seq: number; readonly fencing_token: number; readonly command: RtsCommand }
  | {
      readonly type: "tool_call";
      readonly seq: number;
      readonly fencing_token: number;
      readonly tool: string;
      readonly arguments: unknown;
    }
  | { readonly type: "advance"; readonly ticks: number };

export type ArenaServerMessage =
  | {
      readonly type: "welcome";
      readonly protocol: string;
      readonly faction: number;
      readonly mode: MatchMode;
      readonly tick: number;
      readonly map: MapInfo;
      readonly tools: readonly ToolSchema[];
    }
  | { readonly type: "lease"; readonly seq: number | null; readonly lease?: ControlLease; readonly error?: ErrorBody }
  | { readonly type: "lease_revoked"; readonly faction: number; readonly by: string }
  | { readonly type: "ack"; readonly seq: number; readonly tick: number; readonly ack?: CommandAck; readonly error?: ErrorBody }
  | { readonly type: "tool_result"; readonly seq: number; readonly response: ToolResponse }
  | { readonly type: "observation"; readonly observation: FactionObservation; readonly events: readonly RtsEvent[] }
  | { readonly type: "error"; readonly error: ErrorBody };

/** 带收件人的服务端消息 */
export interface Envelope {
  readonly to: string;
  readonly message: ArenaServerMessage;
}
