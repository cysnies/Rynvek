// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
/** Pi设计借鉴：应用上下文→provider候选→顺序工具→真实结果→事件/续跑。
 * 未依赖/复制Pi代码；无外部模型默认值、密钥、付费、文件/bash或消息发送工具。 */
import type {AgentToolCall,AgentToolName} from "./index.js";
import type {PrivateAgentHubClient,PrivateAgentVersion} from "./private-agent.js";
export interface AgentRunBudget {turns:number;tool_calls:number;wall_seconds:number}
export interface AgentRunEvent {seq:number;kind:string;data:unknown}
export interface AgentRunRecord {
 run_id:string;owner_id:string;submission_id:string;world_id:string;host_fence:string;control_fence:string;faction:number;
 package_id:string;version:string;artifact_cid:string;tools:AgentToolName[];budget:AgentRunBudget;
 turns_used:number;calls_used:number;revision:number;fence:number;status:"ready"|"running"|"paused"|"cancelling"|"cancelled"|"completed"|"unknown";
 deadline_unix_s:number;lease:{controller_id:string;fence:number;expires_at_unix_s:number}|null;
 pending:{operation_id:string;command_id:string;call:AgentToolCall}|null;memory:Record<string,unknown>;last_result:unknown;events:AgentRunEvent[];
}
export interface AgentRunContext {run:AgentRunRecord;observation:{tick:number;[key:string]:unknown};tools:readonly unknown[];external_provider_configured:false;current_host_fence:string;execution_authorization_valid:boolean}
export interface AgentRunCreate {submission_id:string;world_id:string;package_id:string;version:string;artifact_cid:string;control_fence:string;tools:readonly AgentToolName[];budget:AgentRunBudget;consent:"private-pve-tools/1"}
export interface RunControl {operation_id:string;expected_revision:number;action:"claim"|"heartbeat"|"pause"|"cancel"|"recover"|"ack_unknown"|"resume"|"finish";controller_id?:string;host_fence?:string;control_fence?:string}
export interface ProviderDecision {message:string;memory:Record<string,unknown>;call:AgentToolCall|null}
export interface RunProposal extends ProviderDecision {operation_id:string;controller_id:string;fence:number;provider:string}
export type AgentRunTransport=(path:string,method:string,body?:unknown,signal?:AbortSignal)=>Promise<Response>;
const path=(id:string)=>`/api/v1/agent-runs/${encodeURIComponent(id)}`;
export class AgentRunClient {
 constructor(readonly ownerId:string,private transport:AgentRunTransport){}
 private checked(run:AgentRunRecord):AgentRunRecord {if(!run||run.owner_id!==this.ownerId||typeof run.run_id!=="string"||!Number.isSafeInteger(run.revision)||!Array.isArray(run.events)||!run.tools||!run.budget)throw new Error("持久运行身份/结构非法");return run;}
 async create(body:AgentRunCreate,signal?:AbortSignal):Promise<AgentRunRecord>{return this.checked(await (await this.transport("/api/v1/agent-runs","POST",body,signal)).json() as AgentRunRecord);}
 async list(signal?:AbortSignal):Promise<AgentRunRecord[]>{const r=await (await this.transport("/api/v1/agent-runs","GET",undefined,signal)).json() as {runs:AgentRunRecord[]};if(!Array.isArray(r.runs)||r.runs.length>64)throw new Error("运行列表非法");return r.runs.map(r=>this.checked(r));}
 async get(id:string,signal?:AbortSignal):Promise<AgentRunRecord>{const run=this.checked(await (await this.transport(path(id),"GET",undefined,signal)).json() as AgentRunRecord);if(run.run_id!==id)throw new Error("运行ID不一致");return run;}
 async context(id:string,signal?:AbortSignal):Promise<AgentRunContext>{const c=await (await this.transport(path(id)+"/context","GET",undefined,signal)).json() as AgentRunContext;this.checked(c.run);if(c.run.run_id!==id||!Number.isSafeInteger(c.observation?.tick)||!Array.isArray(c.tools)||c.external_provider_configured!==false)throw new Error("运行上下文非法");return c;}
 async control(id:string,body:RunControl,signal?:AbortSignal):Promise<AgentRunRecord>{return this.checked(await (await this.transport(path(id)+"/control","POST",body,signal)).json() as AgentRunRecord);}
 /** 重试必须原operation_id与原候选；Store会返回当前状态但绝不再派发已登记候选。 */
 async step(id:string,body:RunProposal,signal?:AbortSignal):Promise<AgentRunRecord>{return this.checked(await (await this.transport(path(id)+"/step","POST",body,signal)).json() as AgentRunRecord);}
}
/** provider输入显式白名单投影；不含owner/控制fence/controller/操作收据表或凭据。 */
export interface AgentDecisionContext {
 run:Pick<AgentRunRecord,"world_id"|"package_id"|"version"|"artifact_cid"|"tools"|"budget"|"turns_used"|"calls_used"|"memory"|"last_result">;
 observation:AgentRunContext["observation"];tools:readonly unknown[];prior_events:readonly {seq:number;kind:string}[];
}
export function decisionContext(c:AgentRunContext):AgentDecisionContext {
 const r=c.run;let last=r.last_result;
 if(new TextEncoder().encode(JSON.stringify(last)).length>8192){const outcome=last as {kind?:unknown;response?:{tool?:unknown;tick?:unknown;error?:{code?:unknown;name?:unknown}}};const response=outcome.response;
  last={kind:outcome.kind??"recorded_outcome",truncated:true,source:"persistent-run-events",...(response?{response:{tool:response.tool,tick:response.tick,error:response.error?{code:response.error.code,name:response.error.name,truncated:true}:null}}:{})};
 }
 return structuredClone({run:{world_id:r.world_id,package_id:r.package_id,version:r.version,artifact_cid:r.artifact_cid,tools:r.tools,budget:r.budget,turns_used:r.turns_used,calls_used:r.calls_used,memory:r.memory,last_result:last},observation:c.observation,tools:c.tools,prior_events:r.events.slice(-16).map(({seq,kind})=>({seq,kind}))});
}
/** 注入函数是可信宿主代码，不能把任意远端provider伪称沙箱。私有包仍只由服务端QuickJS执行。 */
export interface DecisionProvider {readonly id:string;readonly boundary:"same-origin-package"|"explicit-local-test";decide(context:AgentDecisionContext,signal:AbortSignal):Promise<ProviderDecision>}
export class PrivatePackageProvider implements DecisionProvider {
 readonly id="private-quickjs-tools/1";readonly boundary="same-origin-package" as const;
 constructor(private hub:PrivateAgentHubClient,private meta:PrivateAgentVersion){}
 async decide(c:AgentDecisionContext,signal:AbortSignal):Promise<ProviderDecision>{const r=c.run;if(r.artifact_cid!==this.meta.artifact_cid||r.package_id!==this.meta.package_id||r.version!==this.meta.version)throw new Error("运行包与provider绑定不一致");const d=await this.hub.evaluate(this.meta,r.world_id,r.memory,r.last_result,signal);return {message:"受限私有包候选（非LLM）",memory:d.output.memory,call:d.output.call};}
}
/** 明确测试provider：不生成假工具回执，仅从持久turn计数选择预先声明的候选。 */
export class ScriptedTestProvider implements DecisionProvider {
 readonly id="scripted-local-test/1";readonly boundary="explicit-local-test" as const;
 constructor(private decisions:readonly ProviderDecision[]){}
 async decide(c:AgentDecisionContext,signal:AbortSignal):Promise<ProviderDecision>{signal.throwIfAborted();return structuredClone(this.decisions[c.run.turns_used]??{message:"测试候选结束，不声称目标成功",memory:c.run.memory,call:null});}
}
/** 受控循环在客户端/可信进程，绝不在权威Tick；不持久化凭据或provider函数。 */
export class ControlledAgentAdapter {
 private busy=false;private aborter:AbortController|null=null;private localEpoch=0;
 readonly controllerId=crypto.randomUUID();
 constructor(readonly client:AgentRunClient,readonly runId:string,private provider:DecisionProvider,private onEvent:(event:AgentRunEvent)=>void=()=>{}){if(!["same-origin-package","explicit-local-test"].includes(provider.boundary))throw new Error("未明确授权的provider边界；默认不接第三方模型");}
 stopLocal():void{this.localEpoch++;this.aborter?.abort();}
 private publish(run:AgentRunRecord,after:number):void{for(const event of run.events)if(event.seq>=after)this.onEvent(structuredClone(event));}
 async step():Promise<AgentRunRecord>{
  if(this.busy)throw new Error("一个循环步尚未结束");this.busy=true;const epoch=this.localEpoch;const aborter=new AbortController();this.aborter=aborter;const timeout=setTimeout(()=>aborter.abort(),15000);
  try{
   let run=await this.client.get(this.runId,aborter.signal);let after=run.events.length;
   if(run.status==="ready")run=await this.client.control(run.run_id,{operation_id:crypto.randomUUID(),expected_revision:run.revision,action:"claim",controller_id:this.controllerId},aborter.signal);
   if(run.status!=="running"||run.lease?.controller_id!==this.controllerId)throw new Error("先显式恢复/认领本人运行；其它控制器或未知调用不能接管");
   run=await this.client.control(run.run_id,{operation_id:crypto.randomUUID(),expected_revision:run.revision,action:"heartbeat",controller_id:this.controllerId},aborter.signal);
   const context=await this.client.context(run.run_id,aborter.signal);this.publish(context.run,after);after=context.run.events.length;
   if(!context.execution_authorization_valid)throw new Error("宿主授权已改变，先显式恢复并重新绑定当前fence");
   const decision=await Promise.race([this.provider.decide(decisionContext(context),aborter.signal),new Promise<never>((_,reject)=>{aborter.signal.addEventListener("abort",()=>reject(new Error("provider候选已取消/超时，未派发")),{once:true});})]);aborter.signal.throwIfAborted();if(epoch!==this.localEpoch)throw new Error("候选已取消，未派发");
   const proposal:RunProposal={...decision,operation_id:crypto.randomUUID(),controller_id:this.controllerId,fence:run.lease!.fence,provider:this.provider.id};
   // 发送后取消或网络失败属于未知，重新发现/对账；不自动重试该调用。
   const result=await this.client.step(run.run_id,proposal,aborter.signal);this.publish(result,after);return result;
  }finally{clearTimeout(timeout);this.aborter=null;this.busy=false;}
 }
 /** 明确调用后才循环；真实Tick不够30时只读取状态等待，不伪造推进或消耗provider轮次。 */
 async run():Promise<AgentRunRecord>{
  const epoch=this.localEpoch;let previousTick=-30;
  for(;;){if(epoch!==this.localEpoch)throw new Error("本地循环已停止；服务端已派发工具不可撤回");
   const context=await this.client.context(this.runId);if(epoch!==this.localEpoch)throw new Error("本地循环已停止");if(context.run.status!=="ready"&&context.run.status!=="running")return context.run;if(Date.now()/1000>=context.run.deadline_unix_s||(context.run.lease&&Date.now()/1000>=context.run.lease.expires_at_unix_s))throw new Error("墙钟预算/控制租约已过期，停止本地循环并显式对账恢复");if(context.observation.tick<previousTick+30){await new Promise(r=>setTimeout(r,250));continue;}
   const result=await this.step();previousTick=context.observation.tick;if(result.status!=="running")return result;
  }
 }
}
