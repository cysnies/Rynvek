// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 实验规格（`POST /api/v1/lab/experiments` 请求体），与 Rust `rynvek_rts::lab::ExperimentSpec`
 * 的 serde 形态与校验规则逐条一致（测试直接解析 Rust 源码比对常量）。
 *
 * 种子在 Rust 侧为 `u64`；JSON 数字在 JavaScript 中只能无损表示到 2^53 − 1，
 * 因此这里要求种子为非负安全整数（超出范围的种子请改用 Python / CLI 提交）。
 */

/** 单个实验的 run 数上限（变体数 × 种子数） */
export const MAX_RUNS = 256;
/** 单个 run 的 Tick 上限（30 Hz 下 1 小时） */
export const MAX_TICKS = 108_000;
/** 缺省采样间隔（Ticks） */
export const DEFAULT_SAMPLE_EVERY = 30;
/** 实验名最大字符数 */
export const MAX_NAME_CHARS = 128;
/** 变体标签最大字符数 */
export const MAX_LABEL_CHARS = 64;

import {validateParameterModel,type ParameterModel} from "./parameters.js";
export type LabMode = "pve" | "duel" | "parameters";
export type PolicyKind = "idle" | "economy" | "ecology";

/** 当前Forager供能/出生实验契约；数值必须显式，不是生物学默认常数。 */
export type EnergyUnitKind = "unit.forager"|"unit.striker"|"unit.carapace"|"unit.spitter"|"unit.weaver"|"unit.leviathan";
export interface RoleEnergyContract {
  readonly basal_cost_micro_per_tick:number;
  readonly reserve_ticks:number;
  readonly birth_energy_milli:number;
  readonly movement_cost_micro_per_tile:number;
  readonly synapse_maintenance_micro_per_tick:number;
  readonly attack_cost_micro_per_shot:number;
  readonly repair_cost_micro_per_milli:number;
  readonly secretion_cost_micro_per_field_raw:number;
  readonly continuous_cost_micro_per_neuron_step:number;
  readonly rule_cost_micro_per_gas:number;
  readonly material_capacity_milli:number;
  readonly material_growth_milli_per_tick:number;
  readonly material_synthesis_micro_per_milli:number;
  readonly birth_synthesis_milli:number;
  readonly activity_reserve_milli:number;

  readonly natural_reproduction:boolean;
  readonly replication_mutation_rate_permille:number;
}
export interface WildRoleContract {readonly energy:RoleEnergyContract;readonly uptake_raw_per_tick:number}
export interface WildSeed {readonly kind:EnergyUnitKind;readonly x:number;readonly y:number}
export interface WildEcologyContract {readonly roles:Readonly<Partial<Record<EnergyUnitKind,WildRoleContract>>>;readonly population_limit:number;readonly seeds:readonly WildSeed[]}
export interface EcologyContract {readonly roles:Readonly<Partial<Record<EnergyUnitKind,RoleEnergyContract>>>;readonly wild:WildEcologyContract}


/** 实验变体（一组对照条件） */
export interface VariantSpec {
  readonly label: string;
  readonly mode?: LabMode;
  /** 玩家阵营数；缺省 PvE 为 1、双人局为 2 */
  readonly players?: number;
  readonly policy?: PolicyKind;
  readonly ecology?: EcologyContract | null;
  readonly model?: ParameterModel | null;
}

/** 实验规格 */
export interface ExperimentSpec {
  readonly name: string;
  readonly seeds: readonly number[];
  readonly ticks: number;
  readonly sample_every?: number;
  /** 为空时服务端使用基线变体（单人 PvE + 经济策略） */
  readonly variants?: readonly VariantSpec[];
}

/** 服务端基线变体 */
export const BASELINE_VARIANT: Required<Omit<VariantSpec, "players" | "ecology" | "model">> = {
  label: "baseline",
  mode: "pve",
  policy: "economy",
};

/** 规格校验错误（与服务端 400 / 0x5001 的拒绝原因同义） */
export class SpecError extends Error {
  override readonly name = "SpecError";
}

const charCount = (s: string): number => [...s].length;

/** 变体的玩家数（含缺省值） */
export function playerCount(v: VariantSpec): number {
  return v.players ?? (v.mode === "parameters" ? 0 : (v.mode ?? "pve") === "pve" ? 1 : 2);
}

/** 实际生效的变体列表 */
export function effectiveVariants(spec: ExperimentSpec): readonly VariantSpec[] {
  return spec.variants && spec.variants.length > 0 ? spec.variants : [BASELINE_VARIANT];
}

/** 实际采样间隔（与服务端缺省一致：`min(30, ticks)`） */
export function effectiveSampleEvery(spec: ExperimentSpec): number {
  return spec.sample_every ?? Math.min(DEFAULT_SAMPLE_EVERY, Math.max(1, spec.ticks));
}

/** 校验规格；不合法时抛出 {@link SpecError} */
export function validateSpec(spec: ExperimentSpec): void {
  const nameChars = charCount(spec.name);
  if (spec.name.trim().length === 0 || nameChars > MAX_NAME_CHARS) {
    throw new SpecError(`name 必须为 1–${MAX_NAME_CHARS} 字符`);
  }
  if (spec.seeds.length === 0) throw new SpecError("seeds 不能为空");
  for (const s of spec.seeds) {
    if (!Number.isSafeInteger(s) || s < 0) throw new SpecError(`种子必须是非负安全整数：${s}`);
  }
  if (!Number.isInteger(spec.ticks) || spec.ticks < 1 || spec.ticks > MAX_TICKS) {
    throw new SpecError(`ticks 必须在 1..=${MAX_TICKS}`);
  }
  const every = effectiveSampleEvery(spec);
  if (!Number.isInteger(every) || every < 1 || every > spec.ticks) {
    throw new SpecError("sample_every 必须在 1..=ticks");
  }
  const variants = effectiveVariants(spec);
  const runs = variants.length * spec.seeds.length;
  if (runs > MAX_RUNS) throw new SpecError(`变体数 × 种子数 = ${runs}，超过上限 ${MAX_RUNS}`);
  const labels = new Set<string>();
  let parameterRows=0;
  for (const v of variants) {
    if (v.label.length === 0 || charCount(v.label) > MAX_LABEL_CHARS) {
      throw new SpecError(`变体 label 必须为 1–${MAX_LABEL_CHARS} 字符`);
    }
    if (labels.has(v.label)) throw new SpecError(`变体 label 重复：${v.label}`);
    labels.add(v.label);
    if (v.policy==="ecology" && v.ecology==null) throw new SpecError("ecology策略必须提供显式生态契约");
    if (v.ecology != null) {
      if ((v.mode ?? "pve") !== "pve") throw new SpecError("生态实验只允许显式PvE变体");
      const c=v.ecology;
      if(typeof c!=="object"||Array.isArray(c)||Object.keys(c).some(k=>k!=="roles"&&k!=="wild")||!c.roles||typeof c.roles!=="object"||Array.isArray(c.roles)) throw new SpecError("必须完整提供玩家与野生角色契约");
      for(const [kind,role] of Object.entries(c.roles))validateRole(kind,role);
      const w=c.wild;
      if(!w||typeof w!=="object"||Array.isArray(w)||Object.keys(w).length!==3||Object.keys(w).some(k=>!["roles","population_limit","seeds"].includes(k))||!w.roles||typeof w.roles!=="object"||Array.isArray(w.roles)||!Array.isArray(w.seeds))throw new SpecError("野生契约须完整显式配置");
      const wildRoles=Object.entries(w.roles);
      if(wildRoles.length===0){
        if(w.population_limit!==0||w.seeds.length!==0||Object.keys(c.roles).length===0)throw new SpecError("关闭野生profile须为空种子与0上限，且保留玩家角色");
      }else{
        if(!Number.isInteger(w.population_limit)||w.population_limit<1||w.population_limit>128||w.seeds.length===0||w.seeds.length>16||w.seeds.length>w.population_limit)throw new SpecError("野生种子须1..16且不超过1..128种群上限");
        for(const [kind,role] of wildRoles){
          if(!role||typeof role!=="object"||Array.isArray(role)||Object.keys(role).length!==2||Object.keys(role).some(k=>!["energy","uptake_raw_per_tick"].includes(k)))throw new SpecError("野生角色须显式价格与摄食速率");
          validateRole(kind,role.energy);
          if(!Number.isInteger(role.uptake_raw_per_tick)||role.uptake_raw_per_tick<0||role.uptake_raw_per_tick>256||role.energy.material_capacity_milli<2||role.energy.material_growth_milli_per_tick===0)throw new SpecError("野生结构与局部摄食超出边界");
        }
        const cells=new Set<string>();
        for(const seed of w.seeds){
          if(!seed||typeof seed!=="object"||Array.isArray(seed)||Object.keys(seed).length!==3||Object.keys(seed).some(k=>!["kind","x","y"].includes(k))||!Object.hasOwn(w.roles,seed.kind)||![seed.x,seed.y].every(n=>Number.isInteger(n)&&n>=0&&n<256)||cells.has(`${seed.x},${seed.y}`))throw new SpecError("野生种子须在256地图范围内、有角色且不重叠；实际地形由服务端验证");
          cells.add(`${seed.x},${seed.y}`);
        }
      }
    }
    if(v.mode==="parameters"){
      if((v.policy??"economy")!=="idle"||v.ecology!=null||v.model==null)throw new SpecError("parameters必须提供model、idle且无生态契约");
      validateParameterModel(v.model);
      parameterRows+=v.model.outputs.length*Math.ceil(spec.ticks/every)*spec.seeds.length;
    }else if(v.model!=null)throw new SpecError("model只允许parameters profile");
    const p = playerCount(v);
    const ok = v.mode === "parameters" ? p === 0 : (v.mode ?? "pve") === "pve" ? p === 1 || p === 2 : v.mode === "duel" && p === 2;
    if (!ok) throw new SpecError(`变体 ${v.label} 的 players=${p} 与模式不符`);
  }
  if(parameterRows>200_000)throw new SpecError("参数采样超过200000行；减少种子/变体/输出或增大采样间隔");
}

/** 单个 run 的计划 */
export interface RunPlan {
  readonly run: number;
  readonly variant: VariantSpec;
  readonly seed: number;
}

/** 展开为 run 列表（变体优先、种子次之；编号与服务端 `runs` 表的 `run` 列一致） */
export function planRuns(spec: ExperimentSpec): RunPlan[] {
  const out: RunPlan[] = [];
  for (const variant of effectiveVariants(spec)) {
    for (const seed of spec.seeds) out.push({ run: out.length, variant, seed });
  }
  return out;
}

/** 规范化为请求体（去掉未设置的可选字段，字段顺序固定） */
export function toRequestBody(spec: ExperimentSpec): Record<string, unknown> {
  validateSpec(spec);
  const body: Record<string, unknown> = { name: spec.name, seeds: [...spec.seeds], ticks: spec.ticks };
  if (spec.sample_every !== undefined) body["sample_every"] = spec.sample_every;
  if (spec.variants && spec.variants.length > 0) {
    body["variants"] = spec.variants.map((v) => {
      const o: Record<string, unknown> = { label: v.label };
      if (v.mode !== undefined) o["mode"] = v.mode;
      if (v.players !== undefined) o["players"] = v.players;
      if (v.policy !== undefined) o["policy"] = v.policy;
      if (v.ecology !== undefined) o["ecology"] = v.ecology===null?null:{...v.ecology};
      if(v.model!==undefined)o["model"]=v.model;
      return o;
    });
  }
  return body;
}

/** `[start, start + count)` 的连续种子 */
export function seedRange(start: number, count: number): number[] {
  return Array.from({ length: count }, (_, i) => start + i);
}

const ENERGY_KINDS=["unit.forager","unit.striker","unit.carapace","unit.spitter","unit.weaver","unit.leviathan"];
function validateRole(kind:string,role:RoleEnergyContract|undefined):void {
        if(!ENERGY_KINDS.includes(kind)||!role||typeof role!=="object"||Array.isArray(role)) throw new SpecError("未知角色或非法生命成本对象");
        const keys=["basal_cost_micro_per_tick","reserve_ticks","birth_energy_milli","movement_cost_micro_per_tile","synapse_maintenance_micro_per_tick","natural_reproduction","attack_cost_micro_per_shot","repair_cost_micro_per_milli","secretion_cost_micro_per_field_raw","continuous_cost_micro_per_neuron_step","rule_cost_micro_per_gas","material_capacity_milli","material_growth_milli_per_tick","material_synthesis_micro_per_milli","birth_synthesis_milli","activity_reserve_milli","replication_mutation_rate_permille"];
        if(keys.some(k=>!Object.prototype.hasOwnProperty.call(role,k))||Object.keys(role).some(k=>!keys.includes(k))) throw new SpecError("角色成本必须完整显式配置");
        if(!Number.isInteger(role.replication_mutation_rate_permille)||role.replication_mutation_rate_permille<0||role.replication_mutation_rate_permille>1000)throw new SpecError("复制突变率必须在0..=1000‰");
        if(role.natural_reproduction&&role.material_capacity_milli===1)throw new SpecError("自然复制至少需要2 milli材料容量");
        if(role.material_capacity_milli===0&&(role.material_growth_milli_per_tick!==0||role.material_synthesis_micro_per_milli!==0))throw new SpecError("关闭材料容量时须显式关闭材料摄取与合成价");
        if(typeof role.natural_reproduction!=="boolean") throw new SpecError("自然繁殖许可必须为boolean");
        for(const [key,value] of Object.entries(role)) {
          if(key==="natural_reproduction")continue;
          if(!Number.isInteger(value)||value<0||value>0xffff_ffff||((key==="birth_energy_milli"||key==="basal_cost_micro_per_tick")&&value===0))throw new SpecError("生命成本超出u32合法范围");
        }
        if(BigInt(role.basal_cost_micro_per_tick)*BigInt(role.reserve_ticks)*4n>(1n<<64n)-1n)throw new SpecError("代谢预留超出精确范围");
}
