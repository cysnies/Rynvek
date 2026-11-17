// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// HTTP只是传输；所有权、边界、编译和修订由原权威Actor再次验证。
import {checkParameterState,type ParameterState} from "./parameter-registry.js";
import {AuthoringDocument,type AuthoringBaseline} from "./authoring.js";
import {HybridLogicalClock} from "./hlc.js";
import {UndoManager,type LocalEdit} from "./undo.js";
import {type WorldPatchOp,type WorldPatch} from "./patch.js";
export interface AuthoringView {world_id:string;revision:number;tick:number;state_root:string;rule_fault:[string,number]|null;parameter_state:ParameterState;baseline:Omit<AuthoringBaseline,"terrain"> & {terrain:number[]}}
export interface AuthoringAck {status:"applied";world_id:string;world_revision:number;effective_tick:number;state_root:string;patch_cid:string;report:Record<string,number|boolean>}
export class StudioClient {
  readonly origin:string;
  constructor(origin:string,private readonly token:string,private readonly fetcher:typeof fetch=fetch){
    const u=new URL(origin);if(!["http:","https:"].includes(u.protocol)||u.username||u.password||u.pathname!=="/"||u.search||u.hash)throw new Error("服务地址只允许HTTP(S) origin");
    this.origin=u.origin;
  }
  private async request(path:string,signal:AbortSignal,body?:unknown):Promise<unknown>{
    const r=await this.fetcher(`${this.origin}/api/v1/worlds${path?`/${path}`:""}`,{signal,cache:"no-store",credentials:"omit",redirect:"error",headers:{Authorization:`Bearer ${this.token}`,"Content-Type":"application/json"},...(body===undefined?{}:{method:"POST",body:JSON.stringify(body)})});
    if(!r.ok){const error=await r.json().catch(()=>({}));throw new Error(`HTTP ${r.status} ${String(error.message??"权威请求失败")}`)}return r.json();
  }
  async create(title:string,mapSeed:number,ecology:unknown,signal:AbortSignal):Promise<string>{
    const value=await this.request("",signal,{title,template:"arena_pve",map_seed:mapSeed,visibility:"Private",...(ecology===undefined?{}:{ecology})}) as {world_id:string};
    if(typeof value.world_id!=="string"||!value.world_id)throw new Error("未收到创建世界身份");return value.world_id;
  }
  async view(world:string,signal:AbortSignal):Promise<AuthoringView>{
    const v=await this.request(`${encodeURIComponent(world)}/authoring`,signal) as AuthoringView;
    if(v.world_id!==world||!Number.isSafeInteger(v.revision)||v.revision<0||!Number.isSafeInteger(v.tick)||v.tick<0||!v.baseline||!Array.isArray(v.baseline.terrain)||!v.baseline.terrain.every(n=>Number.isInteger(n)&&n>=0&&n<=4)||!/^[a-f0-9]{64}$/.test(v.state_root))throw new Error("创作基线身份/修订非法");
    checkParameterState(v.parameter_state);return v;
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
  valid=true;private busy=false;tick:number;stateRoot:string;
  constructor(readonly client:StudioClient,readonly view:AuthoringView,authorNodeId:number){
    this.document=new AuthoringDocument({...view.baseline,terrain:Uint8Array.from(view.baseline.terrain)});this.document.acknowledgeRevision(view.revision);
    this.undo=new UndoManager(this.document,new HybridLogicalClock(authorNodeId));this.tick=view.tick;this.stateRoot=view.state_root;
  }
  async edit(ops:readonly WorldPatchOp[],signal:AbortSignal){return this.transact(()=>this.undo.commit(ops),signal)}
  async undoEdit(signal:AbortSignal){return this.transact(()=>this.undo.undo(),signal)}
  async redoEdit(signal:AbortSignal){return this.transact(()=>this.undo.redo(),signal)}
  private async transact(make:()=>LocalEdit|null,signal:AbortSignal):Promise<AuthoringAck|null>{
    if(!this.valid||this.busy)throw new Error("创作会话不可提交，请刷新权威基线");this.busy=true;
    try{
      const edit=make();if(!edit)return null;
      // 本地CRDT/撤销只在回执后展示为权威；拒绝或网络不确定时整会话失效，禁止假成功。
      const ack=await this.client.apply(this.view.world_id,edit.patch,signal);
      this.document.acknowledgeRevision(ack.world_revision);this.tick=ack.effective_tick;this.stateRoot=ack.state_root;return ack;
    }catch(e){this.valid=false;throw e}finally{this.busy=false}
  }
}
