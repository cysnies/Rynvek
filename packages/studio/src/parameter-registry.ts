// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import type {WorldPatchOp} from "./patch.js";
export type ParameterRegistryKind="templates"|"instances"|"bindings";
export interface ParameterTemplate {schema:"rynvek.parameter-template.v1";components:Record<string,{ty:"I32"|"Q8_8"|"Bool";default:number}>}
export interface ParameterInstance {template_id:string;parameters:Record<string,number>}
export interface ParameterRuleBinding {module_id:string;template_id:string}
export interface ParameterState {next_serial:number;tier1_gas_charged:number;entities:Record<string,{serial:number;values:Record<string,number>}>;fault:[string,number]|null}
const key=(s:string)=>s.length>0&&s.length<=128&&/^[a-z0-9]+(?:[-_.][a-z0-9]+)*$/.test(s);
export function parameterRegistryOp(kind:ParameterRegistryKind,id:string,value:ParameterTemplate|ParameterInstance|ParameterRuleBinding|null):WorldPatchOp {
 if(!["templates","instances","bindings"].includes(kind)||!key(id))throw new Error("参数registry类别/ID非法");const json=JSON.stringify(value);if(new TextEncoder().encode(json).length>32768)throw new Error("参数registry单值超过32KiB");return {SetWorldParam:{key_path:`rules.${kind}.${id}`,new_value_json:json}};
}
export function checkParameterState(input:ParameterState):ParameterState {
 if(!input||!Number.isInteger(input.tier1_gas_charged)||input.tier1_gas_charged<0||input.tier1_gas_charged>500000||!Number.isInteger(input.next_serial)||input.next_serial<0||input.next_serial>0xffffffff||!input.entities||Object.keys(input.entities).length>128||input.fault!==null&&(!Array.isArray(input.fault)||input.fault.length!==2||typeof input.fault[0]!=="string"||!Number.isInteger(input.fault[1])))throw new Error("权威参数实体状态非法");
 for(const [id,e]of Object.entries(input.entities)){if(!key(id)||!e||!Number.isInteger(e.serial)||e.serial<1||e.serial>input.next_serial||!e.values||Object.keys(e.values).length>16||Object.values(e.values).some(v=>!Number.isInteger(v)||v< -2147483648||v>2147483647))throw new Error("参数实体组件/序号非法");}return input;
}
export const PARAMETER_RULE_EXAMPLE=JSON.stringify({format:"rynvek.rule-graph.v1",name:"独立参数实体积分器",reads:[{component:"state.total",ty:"I32"},{component:"param.rate",ty:"I32"}],writes:[{component:"state.total",ty:"I32"}],nodes:[{op:"LoadComponent",component:"state.total"},{op:"LoadComponent",component:"param.rate"},{op:"Add",args:[0,1]},{op:"EmitComponentWrite",component:"state.total",args:[2]}]},null,2);
export const PARAMETER_TEMPLATE_EXAMPLE:ParameterTemplate={schema:"rynvek.parameter-template.v1",components:{"state.total":{ty:"I32",default:0},"param.rate":{ty:"I32",default:3}}};

export interface Tier1Install {readonly template_id:string;readonly package_hex:string}
export interface Tier1Draft {readonly package_id:string;readonly version:string;readonly template_id:string;readonly reads:readonly string[];readonly writes:readonly string[];readonly source_js:string;readonly source_ts:string|null}
export function tier1RegistryOp(id:string,value:Tier1Install|null):WorldPatchOp {
 if(!key(id)||value!==null&&(!key(value.template_id)||value.package_hex.length>24576||! /^(?:[0-9a-f]{2})+$/.test(value.package_hex)))throw new Error("Tier1 ID/模板/制品越界");
 return {SetWorldParam:{key_path:`rules.tier1.${id}`,new_value_json:JSON.stringify(value)}};
}
export const TIER1_EXAMPLE:Tier1Draft={package_id:"local.counter",version:"0.1.0",template_id:"model.counter",reads:["state.total","param.rate"],writes:["state.total"],source_js:'function tick(input) { return { writes: input.entities.map(e => ({ id: e.id, values: { "state.total": e.values["state.total"] + e.values["param.rate"] } })) }; }',source_ts:null};
