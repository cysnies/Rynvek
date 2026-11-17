// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { LabClient, lineageForest, type EcologyContract, type EnergyRow, type EnergyUnitKind, type ExperimentSpec, type LineageRow, type RoleEnergyContract, type LabTable, validateSpec } from "@rynvek/lab";
export const ROLE_KINDS:readonly EnergyUnitKind[]=["unit.forager","unit.striker","unit.carapace","unit.spitter","unit.weaver","unit.leviathan"];
export interface RoleDraft {enabled:boolean;rule:RoleEnergyContract}
export function suggestedRoles():Record<EnergyUnitKind,RoleDraft> {
  return Object.fromEntries(ROLE_KINDS.map(kind=>[kind,{enabled:kind==="unit.forager"||kind==="unit.striker",rule:{
    basal_cost_micro_per_tick:100,reserve_ticks:4,birth_energy_milli:1000,movement_cost_micro_per_tile:100,
    synapse_maintenance_micro_per_tick:1,
    attack_cost_micro_per_shot:100,repair_cost_micro_per_milli:1,secretion_cost_micro_per_field_raw:1,birth_synthesis_milli:10,activity_reserve_milli:10,natural_reproduction:kind==="unit.forager",replication_mutation_rate_permille:50
  }}])) as Record<EnergyUnitKind,RoleDraft>;
}
export interface LabDraft {name:string;seeds:string;ticks:number;sampleEvery:number;players:number;policy:"ecology"|"economy"|"idle";rates:string;ablation:boolean;roles:Record<EnergyUnitKind,RoleDraft>}
export function makeExperiment(d:LabDraft):ExperimentSpec {
  const parse=(s:string)=>s.split(",").map(v=>{if(!v.trim())throw new Error("种子/扫描值不能空缺");return Number(v.trim());});
  const rates=[...new Set([...parse(d.rates),...(d.ablation?[0]:[])])];
  if(rates.some(v=>!Number.isInteger(v)||v<0||v>1000))throw new Error("W3复制突变率必须在0..1000‰");
  const variants=rates.map(rate=>{
    const roles=Object.fromEntries(ROLE_KINDS.filter(k=>d.roles[k].enabled).map(k=>[k,{
      ...d.roles[k].rule,...(d.roles[k].rule.natural_reproduction?{replication_mutation_rate_permille:rate}:{})
    }])) as EcologyContract["roles"];
    return {label:`w3-m${rate}`,mode:"pve" as const,players:d.players,policy:d.policy,ecology:{roles}};
  });
  const spec={name:d.name,seeds:parse(d.seeds),ticks:d.ticks,sample_every:d.sampleEvery,variants};
  validateSpec(spec);return spec;
}
export type JsonRow=Readonly<Record<string,unknown>>;
function integer(row:JsonRow,key:string):number {const v=row[key];if(typeof v!=="number"||!Number.isSafeInteger(v)||v<0)throw new Error(`非法整数列${key}`);return v;}
function text(row:JsonRow,key:string):string {const v=row[key];if(typeof v!=="string")throw new Error(`非法文本列${key}`);return v;}
function exact(row:JsonRow,key:string):bigint {const v=text(row,key);if(!/^(0|[1-9][0-9]*)$/.test(v))throw new Error(`非法精确整数${key}`);return BigInt(v);}
export function lineageRows(rows:readonly JsonRow[]):LineageRow[] {
  const result=rows.map((r):LineageRow=>{
    const origin=text(r,"origin");if(origin!=="initial"&&origin!=="spawn"&&origin!=="mitosis")throw new Error("未知出生来源");
    return {run:integer(r,"run"),faction:integer(r,"faction"),entity:integer(r,"entity"),parent:r["parent"]===null?null:integer(r,"parent"),
      slug:text(r,"slug"),unit:text(r,"unit"),generation:integer(r,"generation"),origin,
      born_tick:exact(r,"born_tick"),genome:text(r,"genome"),parent_genome:r["parent_genome"]===null?null:text(r,"parent_genome"),allocated_q:exact(r,"allocated_q")};
  });
  lineageForest(result);return result;
}
export interface EnergySample extends EnergyRow {readonly live_bodies:number;readonly remains:number;readonly state_root:string}
export function energyRows(rows:readonly JsonRow[]):EnergySample[] {
  return rows.map(r=>{
    if(typeof r["enabled"]!=="boolean")throw new Error("缺少生态启用事实");
    return {enabled:r["enabled"],run:integer(r,"run"),tick:exact(r,"tick"),live_bodies:integer(r,"live_bodies"),remains:integer(r,"remains"),state_root:text(r,"state_root"),
      received_q:exact(r,"received_q"),stored_q:exact(r,"stored_q"),dissipated_q:exact(r,"dissipated_q"),exported_q:exact(r,"exported_q"),returned_q:exact(r,"returned_q"),transferred_q:exact(r,"transferred_q"),external_received_q:exact(r,"external_received_q"),attack_q:exact(r,"attack_q"),repair_q:exact(r,"repair_q"),secretion_q:exact(r,"secretion_q"),synthesis_q:exact(r,"synthesis_q")};
  });
}
/** 完整分析有明确内存上限；超限仍可分页/导出，不拿残缺谱系冒充完整森林。 */
export async function analysisTable(client:LabClient,id:string,table:LabTable,signal:AbortSignal):Promise<JsonRow[]> {
  const out:JsonRow[]=[];let offset=0;let cid:string|undefined;
  for(;;) {
    const result=await client.rows(id,table,offset,500,signal);
    if(result.page.total_rows>5000)throw new Error("该表超过5000行工作台完整分析上限；请使用分页或Parquet/Python分析");
    if(cid!==undefined&&cid!==result.cid)throw new Error("分页期间CAS身份改变，停止混合数据");cid=result.cid;
    out.push(...result.page.rows);
    if(result.page.next_offset===null)return out;
    if(result.page.next_offset<=offset)throw new Error("分页未前进");offset=result.page.next_offset;
  }
}
