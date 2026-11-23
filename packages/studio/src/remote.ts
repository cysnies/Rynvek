// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {blake3Digest,fromHex,toHex} from "@rynvek/assets";
// HTTP只是传输；所有权、边界、编译和修订由原权威Actor再次验证。
import {checkParameterState,type ParameterState,type Tier1Draft,type Tier1Install} from "./parameter-registry.js";
import {AuthoringDocument,type AuthoringBaseline} from "./authoring.js";
import {HybridLogicalClock} from "./hlc.js";
import {UndoManager,type LocalEdit} from "./undo.js";
import {type WorldPatchOp,type WorldPatch} from "./patch.js";
export interface AuthoringView {world_id:string;revision:number;tick:number;state_root:string;rule_fault:[string,number]|null;ecology_contract:unknown;parameter_state:ParameterState;collaboration:{principal_id:string;role:"owner"|"studio-editor";grant_id:string|null};baseline:Omit<AuthoringBaseline,"terrain"> & {terrain:number[]}}
export interface AuthoringAck {status:"applied";world_id:string;world_revision:number;effective_tick:number;state_root:string;patch_cid:string;report:Record<string,number|boolean>}
export interface StudioEditor {principal_id:string;grant_id:string;active:boolean;role:"studio-editor";consent:"studio-source-edit/1"}
export interface StudioChanges {world_id:string;from_revision:number;to_revision:number;tick:number;state_root:string;patches:{revision:number;patch:WorldPatch}[];has_more:boolean}
export class StudioClient {
  private readonly grants=new Map<string,string>();
  readonly origin:string;
  constructor(origin:string,private readonly token:string,private readonly fetcher:typeof fetch=fetch){
    const u=new URL(origin);if(!["http:","https:"].includes(u.protocol)||u.username||u.password||u.pathname!=="/"||u.search||u.hash)throw new Error("服务地址只允许HTTP(S) origin");
    this.origin=u.origin;
  }
  private async request(path:string,signal:AbortSignal,body?:unknown):Promise<unknown>{
    const r=await this.fetcher(`${this.origin}/api/v1/worlds${path?`/${path}`:""}`,{signal,cache:"no-store",credentials:"omit",redirect:"error",headers:{...(this.grants.has(path.split("/")[0]!)?{"X-Rynvek-Studio-Grant":this.grants.get(path.split("/")[0]!)!}:{}),Authorization:`Bearer ${this.token}`,"Content-Type":"application/json"},...(body===undefined?{}:{method:"POST",body:JSON.stringify(body)})});
    if(!r.ok){const error=await r.json().catch(()=>({}));throw new Error(`HTTP ${r.status} ${String(error.message??"权威请求失败")}`)}return r.json();
  }
  async create(title:string,mapSeed:number,ecology:unknown,signal:AbortSignal):Promise<string>{
    const value=await this.request("",signal,{title,template:"arena_pve",map_seed:mapSeed,visibility:"Private",...(ecology===undefined?{}:{ecology})}) as {world_id:string};
    if(typeof value.world_id!=="string"||!value.world_id)throw new Error("未收到创建世界身份");return value.world_id;
  }
  async view(world:string,signal:AbortSignal):Promise<AuthoringView>{
    const v=await this.request(`${encodeURIComponent(world)}/authoring`,signal) as AuthoringView;
    if(!Object.hasOwn(v,"ecology_contract")||v.world_id!==world||!Number.isSafeInteger(v.revision)||v.revision<0||!Number.isSafeInteger(v.tick)||v.tick<0||!v.baseline||!Array.isArray(v.baseline.terrain)||!v.baseline.terrain.every(n=>Number.isInteger(n)&&n>=0&&n<=4)||!/^[a-f0-9]{64}$/.test(v.state_root))throw new Error("创作基线身份/修订非法");
    checkParameterState(v.parameter_state);
    if(!v.collaboration||!/^\d+$/.test(v.collaboration.principal_id)||!["owner","studio-editor"].includes(v.collaboration.role)||v.collaboration.role==="studio-editor"&&typeof v.collaboration.grant_id!=="string")throw new Error("创作准入事实非法");
    this.grants.delete(encodeURIComponent(world));if(v.collaboration.grant_id)this.grants.set(encodeURIComponent(world),v.collaboration.grant_id);return v;
  }
  async editors(world:string,signal:AbortSignal):Promise<StudioEditor[]>{const r=await this.request(`${encodeURIComponent(world)}/authoring/editors`,signal) as {grants:StudioEditor[]};if(!Array.isArray(r.grants))throw new Error("准入清单非法");return r.grants;}
  async changeEditor(world:string,principal:string,expected:string|null,active:boolean,signal:AbortSignal):Promise<StudioEditor>{return await this.request(`${encodeURIComponent(world)}/authoring/editors`,signal,{principal_id:principal,expected_grant_id:expected,active,consent:"studio-source-edit/1"}) as StudioEditor;}
  async changes(world:string,after:number,signal:AbortSignal):Promise<StudioChanges>{
    if(!Number.isSafeInteger(after)||after<0)throw new Error("同步游标非法");
    const r=await this.request(`${encodeURIComponent(world)}/authoring/changes?after_revision=${after}`,signal) as StudioChanges;
    if(r.world_id!==world||r.from_revision!==after||!Number.isSafeInteger(r.to_revision)||r.to_revision<after||!Number.isSafeInteger(r.tick)||!/^[a-f0-9]{64}$/.test(r.state_root)||!Array.isArray(r.patches)||r.patches.length>64||r.patches.some(p=>!Number.isSafeInteger(p.revision)||p.revision<=after||p.revision>r.to_revision)||typeof r.has_more!=="boolean"||r.has_more&&r.to_revision===after)throw new Error("同步页身份/游标非法");return r;
  }
  async buildTier1(world:string,draft:Tier1Draft,signal:AbortSignal):Promise<{install:Tier1Install;artifact_blake3:string}> {
    const v=await this.request(`${encodeURIComponent(world)}/authoring/build-tier1`,signal,draft) as {install:Tier1Install;artifact_blake3:string;profile:string;executed:boolean};
    if(v.profile!=="rynvek.tier1-components/1"||v.executed!==false||!v.install||v.install.template_id!==draft.template_id||typeof v.install.package_hex!=="string"||v.install.package_hex.length>24576||! /^(?:[0-9a-f]{2})+$/.test(v.install.package_hex)||! /^[0-9a-f]{64}$/.test(v.artifact_blake3))throw new Error("Tier1构建回执非法");
    if(toHex(blake3Digest(fromHex(v.install.package_hex)))!==v.artifact_blake3)throw new Error("Tier1制品摘要不符");
    return v;
  }
  async compile(world:string,source:string,signal:AbortSignal):Promise<number[]>{
    const v=await this.request(`${encodeURIComponent(world)}/authoring/compile-rule`,signal,{rule_graph_json:source}) as {compiled_rir_blake3:number[]};
    if(!Array.isArray(v.compiled_rir_blake3)||v.compiled_rir_blake3.length!==32||!v.compiled_rir_blake3.every(n=>Number.isInteger(n)&&n>=0&&n<=255))throw new Error("非法编译摘要");return v.compiled_rir_blake3;
  }
  async apply(world:string,patch:WorldPatch,signal:AbortSignal):Promise<AuthoringAck>{
    const v=await this.request(`${encodeURIComponent(world)}/patches`,signal,patch) as AuthoringAck;
    if(v.status!=="applied"||v.world_id!==world||!Number.isSafeInteger(v.world_revision)||v.world_revision<=patch.base_authoring_revision||!Number.isSafeInteger(v.effective_tick)||v.effective_tick<0||!/^[a-f0-9]{64}$/.test(v.state_root))throw new Error("未收到有效权威提交回执");return v;
  }
}
export class RemoteStudioSession {
  readonly document:AuthoringDocument;readonly undo:UndoManager;
  valid=true;private busy=false;private cursor:number;tick:number;stateRoot:string;
  get isBusy(){return this.busy}
  constructor(readonly client:StudioClient,readonly view:AuthoringView,authorNodeId:number){
    this.cursor=view.revision;
    this.document=new AuthoringDocument({...view.baseline,terrain:Uint8Array.from(view.baseline.terrain)});this.document.acknowledgeRevision(view.revision);
    this.undo=new UndoManager(this.document,new HybridLogicalClock(authorNodeId));this.tick=view.tick;this.stateRoot=view.state_root;
  }
  /** 只消费原耐久日志，不上传离线队列；中断/撤权后显式重新连接并清撤销栈。 */
  async sync(signal:AbortSignal):Promise<{patches:number;hasMore:boolean}>{
    if(!this.valid||this.busy)throw new Error("创作会话不可同步");this.busy=true;
    let count=0,more=false;
    try{for(let page=0;page<8;page++){
      const r=await this.client.changes(this.view.world_id,this.cursor,signal);
      for(const e of r.patches){if(this.document.allPatches().length>=512&&!this.document.hasPatch(e.patch.patch_id))throw new Error("本地历史已达512补丁，请重新连接清理撤销边界");this.document.integrateCommitted(e.patch,e.revision);count++;}
      if(new TextEncoder().encode(JSON.stringify(this.document.allPatches())).byteLength>8*1024*1024)throw new Error("本地创作历史超过8MiB，请重新连接");
      this.cursor=r.to_revision;this.document.acknowledgeRevision(Math.max(this.document.revision,r.to_revision));
      if(r.to_revision>=this.document.revision){this.tick=r.tick;this.stateRoot=r.state_root;}
      more=r.has_more;if(!more)break;
    }return {patches:count,hasMore:more};}catch(e){this.valid=false;throw e}finally{this.busy=false}
  }
  async edit(ops:readonly WorldPatchOp[],signal:AbortSignal){return this.transact(()=>this.undo.commit(ops),signal)}
  async undoEdit(signal:AbortSignal){return this.transact(()=>this.undo.undo(),signal)}
  async redoEdit(signal:AbortSignal){return this.transact(()=>this.undo.redo(),signal)}
  private async transact(make:()=>LocalEdit|null,signal:AbortSignal):Promise<AuthoringAck|null>{
    if(!this.valid||this.busy)throw new Error("创作会话不可提交，请刷新权威基线");this.busy=true;
    try{
      if(this.document.allPatches().length>=512)throw new Error("本地历史已达512补丁，请重新连接清理撤销边界");
      const edit=make();if(!edit)return null;
      if(new TextEncoder().encode(JSON.stringify(this.document.allPatches())).byteLength>8*1024*1024)throw new Error("本地创作历史超过8MiB，请重新连接");
      // 本地CRDT/撤销只在回执后展示为权威；拒绝或网络不确定时整会话失效，禁止假成功。
      const ack=await this.client.apply(this.view.world_id,edit.patch,signal);
      this.document.integrateCommitted(edit.patch,ack.world_revision);
      this.document.acknowledgeRevision(ack.world_revision);this.tick=ack.effective_tick;this.stateRoot=ack.state_root;return ack;
    }catch(e){this.valid=false;throw e}finally{this.busy=false}
  }
}
