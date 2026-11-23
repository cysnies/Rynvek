// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import type {VariantSpec} from "./spec.js";
/** Same typed World/Rules source registry. The server remains the authoritative compiler. */
export type ParameterType="I32"|"Q8_8"|"Bool";
export interface ParameterModel {
 readonly schema:"rynvek.lab-parameter-model.v2";
 readonly templates:Readonly<Record<string,{readonly schema:"rynvek.parameter-template.v1";readonly components:Readonly<Record<string,{readonly ty:ParameterType;readonly default:number}>>}>>;
 readonly instances:Readonly<Record<string,{readonly template_id:string;readonly parameters:Readonly<Record<string,number>>}>>;
 readonly tier1:Readonly<Record<string,{readonly template_id:string;readonly package_hex:string}>>;
 readonly rules:Readonly<Record<string,string>>;
 readonly bindings:Readonly<Record<string,{readonly module_id:string;readonly template_id:string}>>;
 readonly outputs:readonly {readonly instance_id:string;readonly component:string}[];
}
export interface ParameterModelCapture {readonly world_id:string;readonly revision:string;readonly model_digest:string;readonly model:ParameterModel}
const key=(s:string)=>s.length>0&&s.length<=128&&/^[a-z0-9]+(?:[-_.][a-z0-9]+)*$/.test(s);
const record=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==="object"&&!Array.isArray(v);
const exact=(v:Record<string,unknown>,names:readonly string[])=>Object.keys(v).length===names.length&&names.every(k=>Object.hasOwn(v,k));
function value(ty:ParameterType,v:number):boolean{return Number.isInteger(v)&&(ty==="Bool"?(v===0||v===1):ty==="Q8_8"?(v>=-32768&&v<=32767):ty==="I32"&&v>=-2147483648&&v<=2147483647)}
/** Structural/value bounds only: RuleGraph compilation, write conflicts and gas are verified by the original server compiler. */
export function validateParameterModel(input:unknown):asserts input is ParameterModel {
 if(!record(input)||!exact(input,["schema","templates","instances","rules","bindings","tier1","outputs"])||input.schema!=="rynvek.lab-parameter-model.v2"||new TextEncoder().encode(JSON.stringify(input)).byteLength>256*1024)throw new Error("无效参数模型schema/256KiB边界");
 for(const [field,min,max] of [["templates",1,16],["instances",1,128],["rules",0,32],["bindings",0,16],["tier1",0,4]] as const){const map=input[field];if(!record(map)||Object.keys(map).length<min||Object.keys(map).length>max||!Object.keys(map).every(key))throw new Error(`模型${field}必须在${min}..${max}且ID有效`);}
 const m=input as unknown as ParameterModel;
 if(Object.keys(m.bindings).length===0&&Object.keys(m.tier1).length===0)throw new Error("模型须有Tier0绑定或Tier1规则");
 for(const i of Object.values(m.tier1)){if(!record(i)||!exact(i,["template_id","package_hex"])||!Object.hasOwn(m.templates,i.template_id)||typeof i.package_hex!=="string"||i.package_hex.length>24576||! /^(?:[0-9a-f]{2})+$/.test(i.package_hex))throw new Error("Tier1制品/模板非法；原RYPK/声明和冲突由服务端复验");}
 for(const t of Object.values(m.templates)){if(!record(t)||!exact(t,["schema","components"])||t.schema!=="rynvek.parameter-template.v1"||!record(t.components)||!Object.keys(t.components).length||Object.keys(t.components).length>16)throw new Error("无效参数模板");for(const [n,c] of Object.entries(t.components)){if(!key(n)||! /^(state|param)\./.test(n)||!record(c)||!exact(c,["ty","default"])||!value(c.ty,c.default))throw new Error("无效参数组件/精确整数");}}
 for(const i of Object.values(m.instances)){if(!record(i)||!exact(i,["template_id","parameters"])||typeof i.template_id!=="string"||!Object.hasOwn(m.templates,i.template_id)||!record(i.parameters))throw new Error("无效参数实例");for(const [n,v] of Object.entries(i.parameters)){const c=m.templates[i.template_id]!.components[n];if(!n.startsWith("param.")||!c||!value(c.ty,v))throw new Error("实例只允许类型正确的param覆盖");}}
 if(!Object.values(m.rules).every(s=>typeof s==="string"))throw new Error("规则必须为原始RuleGraph JSON字符串");
 for(const b of Object.values(m.bindings))if(!record(b)||!exact(b,["module_id","template_id"])||typeof b.module_id!=="string"||typeof b.template_id!=="string"||!Object.hasOwn(m.rules,b.module_id)||!Object.hasOwn(m.templates,b.template_id))throw new Error("无效规则绑定");
 if(!Array.isArray(m.outputs)||m.outputs.length<1||m.outputs.length>16)throw new Error("需要1..16个明确采样输出");
 const seen=new Set<string>();for(const o of m.outputs){if(!record(o)||!exact(o,["instance_id","component"])||typeof o.instance_id!=="string"||typeof o.component!=="string"||!Object.hasOwn(m.instances,o.instance_id)||!Object.hasOwn(m.templates[m.instances[o.instance_id]!.template_id]!.components,o.component))throw new Error("输出实例/组件不存在");const id=JSON.stringify([o.instance_id,o.component]);if(seen.has(id))throw new Error("重复采样输出");seen.add(id);}
}
/** Single-axis exact raw-integer sweep: each immutable variant travels through the normal persistent job. */
export function parameterSweep(model:ParameterModel,instanceId:string,component:string,values:readonly number[]):VariantSpec[]{
 validateParameterModel(model);
 const instance=model.instances[instanceId],c=instance&&model.templates[instance.template_id]?.components[component];
 if(!instance||!c||!component.startsWith("param.")||!values.length||values.length>256||new Set(values).size!==values.length||!values.every(v=>value(c.ty,v)))throw new Error("扫描必须为已存在param的1..256个不重复精确合法整数");
 return values.map((v,index)=>{const copy=structuredClone(model);return {label:`parameter-${index}-${v}`,mode:"parameters",players:0,policy:"idle",model:{...copy,instances:{...copy.instances,[instanceId]:{...copy.instances[instanceId]!,parameters:{...copy.instances[instanceId]!.parameters,[component]:v}}}}};});
}
/** Exact decoded CAS metrics. RTS rows are not relabeled as parameter measurements. */
export interface ParameterMetric {readonly run:number;readonly tick:bigint;readonly instance_id:string;readonly component:string;readonly ty:ParameterType;readonly raw:number}
export function parameterMetrics(rows:readonly Record<string,unknown>[]):ParameterMetric[]{return rows.filter(r=>r.parameter_entity!=null).map(r=>{
 const tick=typeof r.tick==="string"&&/^\d+$/.test(r.tick)?BigInt(r.tick):typeof r.tick==="bigint"?r.tick:typeof r.tick==="number"&&Number.isSafeInteger(r.tick)?BigInt(r.tick):-1n;
 if(typeof r.run!=="number"||!Number.isInteger(r.run)||r.run<0||r.run>0xffff_ffff||tick<0n||tick>(1n<<64n)-1n||typeof r.parameter_entity!=="string"||!key(r.parameter_entity)||typeof r.parameter_component!=="string"||!key(r.parameter_component)||!["I32","Q8_8","Bool"].includes(String(r.parameter_type))||typeof r.parameter_value!=="number"||!value(r.parameter_type as ParameterType,r.parameter_value)||r.faction!==null||r.score_milli!==null)throw new Error("参数指标身份/整数/非RTS语义无效");
 return {run:r.run,tick,instance_id:r.parameter_entity,component:r.parameter_component,ty:r.parameter_type as ParameterType,raw:r.parameter_value};
});}
