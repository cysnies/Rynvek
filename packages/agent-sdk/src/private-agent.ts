// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
import {PrivateVisualClient} from "./private-visual.js";
/** 私有Agent包的协议编解码/签名回调、HTTP消费与显式单步委托。不在SDK/浏览器执行包源码。 */
import { PrivateAudioClient } from "./private-audio.js";
import { AgentRunClient } from "./agent-run.js";
import { blake3 } from "@noble/hashes/blake3.js";
import { AGENT_TOOL_NAMES, type AgentToolCall, type AgentToolName } from "./index.js";
const utf8=new TextEncoder();
export const agentDigest=(bytes:Uint8Array):string=>Array.from(blake3(bytes),b=>b.toString(16).padStart(2,"0")).join("");
const hex=(bytes:Uint8Array):string=>Array.from(bytes,b=>b.toString(16).padStart(2,"0")).join("");
function unhex(value:string,length:number):Uint8Array {if(!new RegExp(`^[0-9a-f]{${length*2}}$`).test(value))throw new Error("非法摘要/签名字节");return Uint8Array.from(value.match(/../g)!,x=>parseInt(x,16));}
const align=(n:number)=>Math.ceil(n/8)*8;
const packageId=(s:string)=>/^[a-z0-9]+(?:-[a-z0-9]+)*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+$/.test(s)&&s.length<=128;
const version=(s:string)=>/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/.test(s)&&s.length<=32;
function path(id:string,ver:string):string{if(!packageId(id)||!version(ver))throw new Error("包ID/精确版本非法");return `/api/v1/private-hub/agents/${id}/${ver}`;}
function owner(id:string):void{if(!/^[1-9]\d*$/.test(id)||BigInt(id)>0xffffffffffffffffn)throw new Error("所有者ID必须为u64十进制字符串");}
function credentialFree(value:string):void {if(/bearer |authorization|capability_token|api_key|apikey|password|client_secret|private key|-----begin/i.test(value))throw new Error("拒绝凭据污染；源码、输入和错误内容不回显");}
export interface PrivateAgentDependency {id:string;version:string;blake3_256:string}
export interface PrivateAgentProfile {schema:"rynvek.agent.v1";runtime:"quickjs-tools/1";role:"agent"|"library";entrypoint:"main.js";tools:AgentToolName[]}
export interface AgentPackageInput {id:string;version:string;name:string;license:"MIT"|"Apache-2.0"|"Apache-2.0 WITH LLVM-exception"|"AGPL-3.0-only"|"CC0-1.0"|"CC-BY-4.0";role:"agent"|"library";tools:AgentToolName[];source:string;dependencies?:PrivateAgentDependency[]}
/** 写出既有RYPK v1三文件profile；不是在旧容器外另造包格式。服务端再次完整校验。 */
export function buildPrivateAgentPackage(input:AgentPackageInput):Uint8Array {
  path(input.id,input.version);if(!input.name||input.name.length>128||utf8.encode(input.source).length>65536)throw new Error("包名称或源码大小非法");
  const profile:PrivateAgentProfile={schema:"rynvek.agent.v1",runtime:"quickjs-tools/1",role:input.role,entrypoint:"main.js",tools:[...input.tools]};
  checkProfile(profile);credentialFree(input.source);credentialFree(input.id+input.version+input.name+JSON.stringify(input.dependencies??[]));
  const deps=input.dependencies??[];if(deps.length>8||new Set(deps.map(d=>d.id)).size!==deps.length)throw new Error("依赖必须唯一且至多8项");
  for(const d of deps){path(d.id,d.version);unhex(d.blake3_256,32);}
  const files=[{path:"agent.json",kind:"data",media_type:"application/json",data:utf8.encode(JSON.stringify(profile))},
    {path:"dependencies.lock",kind:"data",media_type:"application/json",data:utf8.encode(JSON.stringify({lockfile_version:1,packages:deps.map(d=>({id:d.id,version:d.version,blake3_256:d.blake3_256}))}))},
    {path:"main.js",kind:"rule_source",media_type:"text/javascript",data:utf8.encode(input.source)}];
  let cursor=0;const entries=files.map(f=>{const e={path:f.path,kind:f.kind,media_type:f.media_type,offset:cursor,length:f.data.length,blake3_256:agentDigest(f.data)};cursor=align(cursor+f.data.length);return e;});
  const manifest=utf8.encode(JSON.stringify({schema:"rynvek.rpack.v1",id:input.id,version:input.version,name:input.name,license:input.license,engine_compat:"rynvek-agent-tools/1",entries}));
  const payload=new Uint8Array(cursor);for(let i=0;i<files.length;i++)payload.set(files[i]!.data,entries[i]!.offset);
  const out=new Uint8Array(96+align(manifest.length)+payload.length);if(out.length>262144)throw new Error("Agent容器超过256KiB");
  const view=new DataView(out.buffer);out.set(utf8.encode("RYPK"));view.setUint16(4,1,true);view.setUint16(8,64,true);view.setUint16(10,3,true);
  out.set(blake3(utf8.encode(input.id)).slice(0,16),16);view.setBigUint64(32,BigInt(manifest.length),true);view.setBigUint64(40,BigInt(payload.length),true);view.setBigUint64(48,BigInt(out.length),true);
  const digest=blake3.create().update(manifest).update(payload).digest();out.set(digest,64);out.set(manifest,96);out.set(payload,96+align(manifest.length));return out;
}
function checkProfile(p:PrivateAgentProfile):void {
  if(!p||p.schema!=="rynvek.agent.v1"||p.runtime!=="quickjs-tools/1"||p.entrypoint!=="main.js"||!["agent","library"].includes(p.role)||!Array.isArray(p.tools)||p.tools.length>6||new Set(p.tools).size!==p.tools.length||p.tools.some(t=>!(AGENT_TOOL_NAMES as readonly string[]).includes(t))||(p.role==="library"&&p.tools.length!==0))throw new Error("Agent profile非法");
}
/** 私有域与公开Hub签名域分离，绑定owner；回调只接收待签名字节，不收集或保存私钥。 */
export function privateAgentSigningMessage(ownerId:string,id:string,ver:string,artifact:Uint8Array):Uint8Array {
  owner(ownerId);path(id,ver);const prefix=utf8.encode(`rynvek-private-agent/1\0${ownerId}\0${id}\0${ver}\0`);
  const out=new Uint8Array(prefix.length+32);out.set(prefix);out.set(blake3(artifact),prefix.length);return out;
}
export interface SignedAgentUpload {owner_id:string;package_id:string;version:string;artifact_cid:string;artifact_b64:string;signature:string}
export async function signPrivateAgentPackage(ownerId:string,input:AgentPackageInput,sign:(message:Uint8Array)=>Promise<Uint8Array>):Promise<SignedAgentUpload> {
  const bytes=buildPrivateAgentPackage(input);const signature=await sign(privateAgentSigningMessage(ownerId,input.id,input.version,bytes));if(signature.length!==64)throw new Error("Ed25519签名必须64字节");
  let raw="";for(let i=0;i<bytes.length;i+=8192)raw+=String.fromCharCode(...bytes.subarray(i,i+8192));
  return {owner_id:ownerId,package_id:input.id,version:input.version,artifact_cid:agentDigest(bytes),artifact_b64:btoa(raw),signature:hex(signature)};
}
export interface PrivateAgentVersion {owner_id:string;package_id:string;version:string;artifact_cid:string;publisher_key:string;signature:string;profile:PrivateAgentProfile;dependencies:PrivateAgentDependency[];verification:{trust_level:3;[key:string]:unknown};revoked:boolean;visibility:"Private"}
export interface AgentDecision {package_id:string;version:string;artifact_cid:string;world_id:string;principal_id:string;faction:number;tick:number;output:{call:AgentToolCall|null;memory:Record<string,unknown>};gas_used:number;executed:false}
export class PrivateHubHttpError extends Error {constructor(readonly status:number){super(`私有Hub请求被拒绝 ${status}；未确认执行`);}}
export class PrivateAgentHubClient {
  readonly origin:string;private enabled=true;
  constructor(origin:string,readonly ownerId:string,private token:()=>string,private fetcher:typeof fetch=fetch) {
    owner(ownerId);const u=new URL(origin);if(!["http:","https:"].includes(u.protocol)||u.username||u.password||u.pathname!=="/"||u.search||u.hash)throw new Error("私有Hub只允许无凭据HTTP(S) origin");this.origin=u.origin;
  }
  visual():PrivateVisualClient{return new PrivateVisualClient(this.ownerId,(path,method,body,signal)=>this.request(path,method,body,signal));}
  audio():PrivateAudioClient{return new PrivateAudioClient(this.ownerId,(path,method,body,signal)=>this.request(path,method,body,signal));}
  runs():AgentRunClient{return new AgentRunClient(this.ownerId,(path,method,body,signal)=>this.request(path,method,body,signal));}
  destroy():void{this.enabled=false;this.token=()=>"";}
  private async request(path:string,method="GET",body?:unknown,signal?:AbortSignal):Promise<Response> {
    const token=this.enabled?this.token():"";if(!token)throw new Error("私有Hub身份已断开");
    const r=await this.fetcher(this.origin+path,{method,headers:{Authorization:`Bearer ${token}`,"Content-Type":"application/json"},cache:"no-store",redirect:"error",credentials:"omit",...(signal?{signal}:{}),...(body===undefined?{}:{body:JSON.stringify(body)})});
    if(!r.ok)throw new PrivateHubHttpError(r.status);return r;
  }
  private check(m:PrivateAgentVersion):PrivateAgentVersion {
    if(!m||m.owner_id!==this.ownerId||m.visibility!=="Private"||typeof m.revoked!=="boolean"||m.verification?.trust_level!==3)throw new Error("私有包归属/信任回执不一致");
    path(m.package_id,m.version);unhex(m.artifact_cid,32);unhex(m.publisher_key,32);unhex(m.signature,64);checkProfile(m.profile);return m;
  }
  async list(signal?:AbortSignal):Promise<PrivateAgentVersion[]> {
    const r=await (await this.request("/api/v1/private-hub/agents","GET",undefined,signal)).json() as {visibility:string;packages:PrivateAgentVersion[]};
    if(r.visibility!=="Private"||!Array.isArray(r.packages)||r.packages.length>256)throw new Error("私有发现响应非法");return r.packages.map(m=>this.check(m));
  }
  async get(id:string,ver:string,signal?:AbortSignal):Promise<PrivateAgentVersion>{const m=this.check(await (await this.request(path(id,ver),"GET",undefined,signal)).json() as PrivateAgentVersion);if(m.package_id!==id||m.version!==ver)throw new Error("包版本身份不一致");return m;}
  async publish(upload:SignedAgentUpload,signal?:AbortSignal):Promise<PrivateAgentVersion> {
    if(upload.owner_id!==this.ownerId||upload.artifact_b64.length>350000)throw new Error("签名上传归属或大小非法");unhex(upload.signature,64);unhex(upload.artifact_cid,32);
    const m=this.check(await (await this.request(path(upload.package_id,upload.version),"PUT",{artifact_b64:upload.artifact_b64,signature:upload.signature},signal)).json() as PrivateAgentVersion);
    if(m.package_id!==upload.package_id||m.version!==upload.version||m.artifact_cid!==upload.artifact_cid||m.revoked)throw new Error("发布制品回执不一致");return m;
  }
  async download(meta:PrivateAgentVersion,signal?:AbortSignal):Promise<Uint8Array> {
    this.check(meta);if(meta.revoked)throw new Error("不能下载撤销版本");
    const response=await this.request(path(meta.package_id,meta.version)+"/artifact","GET",undefined,signal);
    const bytes=new Uint8Array(await response.arrayBuffer());if(bytes.length>262144||agentDigest(bytes)!==meta.artifact_cid)throw new Error("私有下载BLAKE3校验失败");
    // 公钥锚定于认证服务元数据，不冒充外部CA/独立信任链。
    const key=await crypto.subtle.importKey("raw",unhex(meta.publisher_key,32).buffer as ArrayBuffer,{name:"Ed25519"},false,["verify"]);
    const valid=await crypto.subtle.verify("Ed25519",key,unhex(meta.signature,64).buffer as ArrayBuffer,privateAgentSigningMessage(meta.owner_id,meta.package_id,meta.version,bytes).buffer as ArrayBuffer);
    if(!valid)throw new Error("私有下载Ed25519验签失败");return bytes;
  }
  async revoke(meta:PrivateAgentVersion,signal?:AbortSignal):Promise<void>{this.check(meta);const r=await (await this.request(path(meta.package_id,meta.version),"DELETE",undefined,signal)).json() as {status:string};if(r.status!=="revoked")throw new Error("未确认撤销");}
  async evaluate(meta:PrivateAgentVersion,worldId:string,memory:Record<string,unknown>,lastResult:unknown,signal?:AbortSignal):Promise<AgentDecision> {
    this.check(meta);if(meta.revoked||meta.profile.role!=="agent"||!/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(worldId))throw new Error("只运行未撤销Agent与显式世界UUID");
    const body={world_id:worldId,artifact_cid:meta.artifact_cid,memory,last_result:lastResult};const encoded=JSON.stringify(body);credentialFree(encoded);if(utf8.encode(encoded).length>32768)throw new Error("Agent输入超过32KiB");
    const d=await (await this.request(path(meta.package_id,meta.version)+"/evaluate","POST",body,signal)).json() as AgentDecision;
    if(d.executed!==false||d.principal_id!==this.ownerId||d.world_id!==worldId||d.package_id!==meta.package_id||d.version!==meta.version||d.artifact_cid!==meta.artifact_cid||!Number.isSafeInteger(d.tick)||d.tick<0||!Number.isSafeInteger(d.faction)||d.faction<0||d.faction>7||!d.output||!d.output.memory||typeof d.output.memory!=="object"||Array.isArray(d.output.memory)||utf8.encode(JSON.stringify(d.output.memory)).length>16384)throw new Error("Agent候选回执身份/结构不一致");
    const call=d.output.call;if(call!==null&&(!call||!meta.profile.tools.includes(call.name)||!(AGENT_TOOL_NAMES as readonly string[]).includes(call.name)||!call.arguments||typeof call.arguments!=="object"||Array.isArray(call.arguments)))throw new Error("Agent候选工具越权/参数非法");credentialFree(JSON.stringify(d.output));return d;
  }
}
/** 结构接口，不引入服务端/仿真器或包执行器。ArenaClient可直接满足此接口。 */
export interface AgentToolReceipt {readonly tool:string;readonly tick:number;readonly result?:unknown;readonly error?:{readonly code:number;readonly name:string;readonly message:string}}
export interface AgentArenaPort {
  readonly identity:{readonly principalId:string;readonly faction:number};
  readonly welcome:{readonly mode:string}|null;
  readonly lease:{readonly fencing_token:number}|null;
  callTool(tool:string,args:unknown):Promise<AgentToolReceipt>;
}
export interface AgentStep {decision:AgentDecision;receipt:AgentToolReceipt|null;dispatched:boolean;accepted:boolean}
export class PrivateAgentRunner {
  private memory:Record<string,unknown>={};private lastResult:unknown=null;private busy=false;private stopped=false;private epoch=0;private pending:AbortController|null=null;
  constructor(private hub:PrivateAgentHubClient,private meta:PrivateAgentVersion,private worldId:string,private arena:AgentArenaPort){this.meta=structuredClone(meta);}
  /** 停止只取消尚未发送的候选；已发出的权威工具不能撤回，不自动重试未知回执。 */
  stop():void{this.stopped=true;this.epoch++;this.pending?.abort();this.memory={};this.lastResult=null;}
  private live(fence:number):void {if(this.stopped||this.arena.identity.principalId!==this.hub.ownerId||this.arena.welcome?.mode!=="PvE_Survival"||this.arena.lease?.fencing_token!==fence)throw new Error("委托已停止或本人PvE控制租约已改变");}
  async step():Promise<AgentStep> {
    if(this.busy)throw new Error("同一个Agent步尚未结束，不重复提交");const fence=this.arena.lease?.fencing_token;if(fence===undefined)throw new Error("先显式取得本人世界控制租约");this.live(fence);
    this.busy=true;const epoch=this.epoch;const pending=new AbortController();this.pending=pending;
    let sent=false;let timer:ReturnType<typeof setTimeout>|undefined;
    const timeout=new Promise<never>((_,reject)=>{timer=setTimeout(()=>{this.stop();reject(new Error(sent?"工具已发送但回执未确认；已停止，勿自动重试":"Agent候选超时，已停止"));},8000);});
    try {
      const decision=await Promise.race([this.hub.evaluate(this.meta,this.worldId,this.memory,this.lastResult,pending.signal),timeout]);this.live(fence);
      if(epoch!==this.epoch||decision.faction!==this.arena.identity.faction)throw new Error("过期/非本人阵营候选，未发送工具");
      const call=decision.output.call;let receipt:AgentToolReceipt|null=null;
      if(call){sent=true;receipt=await Promise.race([this.arena.callTool(call.name,call.arguments),timeout]);this.live(fence);if(epoch!==this.epoch||receipt.tool!==call.name)throw new Error("工具回执过期/不一致，不更新本地记忆");}
      this.memory=decision.output.memory;
      this.lastResult=receipt&&utf8.encode(JSON.stringify(receipt)).length>8192?{tool:receipt.tool,tick:receipt.tick,error:receipt.error??null,truncated:true}:receipt;
      return {decision,receipt,dispatched:sent,accepted:receipt!==null&&!receipt.error};
    } catch(error) {if(sent)this.stop();throw error;} finally {clearTimeout(timer);this.pending=null;this.busy=false;}
  }
}
