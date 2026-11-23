// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
import {readRuleJson,type RuleRequest} from "./private-rule.js";
export const GENERAL_RUN_CONSENT="run_installed_rules_with_bounded_budget";
export interface GeneralRunSpec {submission_id:string;expected_revision:number;expected_root:string;ticks:number;wall_seconds:number;interval_ms:number;max_energy_q:string;consent:typeof GENERAL_RUN_CONSENT}
export interface GeneralRunOperation {operation_id:string;generation:number;action:"pause"|"resume"|"cancel"}
export interface GeneralRun {storage_backend:"memory"|"postgres";id:string;world_id:string;owner:string;spec:GeneralRunSpec;base_tick:number;last_root:string;done:number;energy_used_q:number;status:"queued"|"running"|"paused"|"blocked"|"completed"|"cancelled"|"expired";generation:number;fence:number;attempt:number;node:string|null;lease_until_s:number;deadline_s:number;error:string|null;operations:Record<string,GeneralRunOperation>}
function uuid(v:string,v7=false):string {if(typeof v!=="string"||!(v7?/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/:/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/).test(v))throw new Error("须规范UUID");return v;}
function n(v:number,min=0,max=Number.MAX_SAFE_INTEGER):number {if(!Number.isSafeInteger(v)||v<min||v>max)throw new Error("运行整数范围非法");return v;}
function root(v:string):void{if(typeof v!=="string"||!/^[0-9a-f]{64}$/.test(v))throw new Error("运行根非法");}
function keys(v:any,names:string[]):void {if(!v||typeof v!=="object"||Array.isArray(v)||Object.keys(v).sort().join()!==names.sort().join())throw new Error("运行字段非法");}
export function validateGeneralRunSpec(s:GeneralRunSpec):void {
 keys(s,["submission_id","expected_revision","expected_root","ticks","wall_seconds","interval_ms","max_energy_q","consent"]);
 uuid(s.submission_id,true);n(s.expected_revision);root(s.expected_root);n(s.ticks,1,128);n(s.wall_seconds,10,900);n(s.interval_ms,200,5000);
 if(s.consent!==GENERAL_RUN_CONSENT||typeof s.max_energy_q!=="string"||!/^[1-9][0-9]{0,12}$/.test(s.max_energy_q)||BigInt(s.max_energy_q)>1000000000000n)throw new Error("须明确规则paid_q预算与同意，不是总世界能耗");
}
export function validateGeneralRunOperation(op:GeneralRunOperation):void {keys(op,["operation_id","generation","action"]);uuid(op.operation_id,true);n(op.generation,0,0xffffffff);if(!["pause","resume","cancel"].includes(op.action))throw new Error("运行控制非法");}
export function validateGeneralRun(v:GeneralRun,owner:string,world:string):void {
 keys(v,["storage_backend","id","world_id","owner","spec","base_tick","last_root","done","energy_used_q","status","generation","fence","attempt","node","lease_until_s","deadline_s","error","operations"]);
 uuid(v.id,true);if(v.owner!==owner||v.world_id!==world||!["memory","postgres"].includes(v.storage_backend))throw new Error("运行归属/持久等级非法");
 validateGeneralRunSpec(v.spec);root(v.last_root);n(v.base_tick);n(v.base_tick+v.done);n(v.spec.expected_revision+v.done);n(v.done,0,v.spec.ticks);n(v.energy_used_q,0,Number(v.spec.max_energy_q));n(v.generation,0,0xffffffff);n(v.fence);n(v.attempt,0,5);n(v.lease_until_s);n(v.deadline_s,1);
 if(!["queued","running","paused","blocked","completed","cancelled","expired"].includes(v.status)||v.status==="completed"&&v.done!==v.spec.ticks||v.done===0&&(v.energy_used_q!==0||v.last_root!==v.spec.expected_root))throw new Error("运行进度/费用不符");
 if(v.node!==null&&(typeof v.node!=="string"||v.node.length>128)||v.error!==null&&(typeof v.error!=="string"||v.error.length>4096))throw new Error("运行状态文字非法");
 if(!v.operations||typeof v.operations!=="object"||Array.isArray(v.operations)||Object.keys(v.operations).length>64)throw new Error("运行收据非法");
 for(const [id,op] of Object.entries(v.operations)){validateGeneralRunOperation(op);if(id!==op.operation_id||op.generation>=v.generation)throw new Error("控制收据代次非法");}
}
/** 每次仅一个HTTP动作或只读对账；绝不循环Tick、自动认领/续租/重发。 */
export class GeneralRunClient {
 constructor(readonly owner:string,private readonly request:RuleRequest){if(typeof owner!=="string"||!/^[1-9][0-9]*$/.test(owner)||BigInt(owner)>0xffffffffffffffffn)throw new Error("须精确u64所有者");}
 private path(world:string):string{return `/api/v1/worlds/${uuid(world)}/general`;}
 private async call(world:string,path:string,method:string,body:unknown,signal?:AbortSignal):Promise<GeneralRun>{signal?.throwIfAborted();const v=await readRuleJson(await this.request(path,method,body,signal));signal?.throwIfAborted();validateGeneralRun(v,this.owner,world);return v;}
 async start(world:string,host:number,control:number,spec:GeneralRunSpec,signal?:AbortSignal):Promise<GeneralRun>{
  spec={...spec};validateGeneralRunSpec(spec);const path=this.path(world);n(host,1);n(control,1);
  const v=await this.call(world,path,"POST",{action:"start_run",host_fence:host,control_fence:control,spec},signal);
  if(Object.keys(spec).some(k=>(v.spec as any)[k]!== (spec as any)[k]))throw new Error("启动精确规格不符");return v;
 }
 async get(world:string,id:string,signal?:AbortSignal):Promise<GeneralRun>{const v=await this.call(world,`${this.path(world)}/runs/${uuid(id,true)}`,"GET",undefined,signal);if(v.id!==id)throw new Error("运行ID不符");return v;}
 async lookup(world:string,submission:string,signal?:AbortSignal):Promise<GeneralRun>{const v=await this.call(world,`${this.path(world)}/runs?submission_id=${uuid(submission,true)}`,"GET",undefined,signal);if(v.spec.submission_id!==submission)throw new Error("原提交键不符");return v;}
 async control(world:string,id:string,op:GeneralRunOperation,signal?:AbortSignal):Promise<GeneralRun>{
  op={...op};validateGeneralRunOperation(op);const v=await this.call(world,`${this.path(world)}/runs/${uuid(id,true)}`,"POST",op,signal);
  const receipt=v.operations[op.operation_id];if(v.id!==id||!receipt||receipt.action!==op.action||receipt.generation!==op.generation)throw new Error("控制收据不符；只读对账勿自动重发");return v;
 }
}
