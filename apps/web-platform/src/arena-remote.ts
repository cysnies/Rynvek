// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/** 当前权威世界的 HTTP/WS 与只读重演适配。不造世界、帧、资源、基因或本地仿真。 */
import type { ArenaClientMessage, ArenaServerMessage, ArenaTransport, FactionObservation, MapInfo, MatchMode, RtsEvent } from "@rynvek/client";

export interface WorldRun {
  world_id:string;match_id:string;principal_id:string;faction:number;mode:MatchMode;
  match_state:string;tick:number;recovered:boolean;
}
export interface PlaybackFrame {
  match_id:string;world_id:string;faction:number;mode:MatchMode;initial_tick:number;final_tick:number;
  tick:number;revision:string;state_root:string;initial_revision:string;initial_state_root:string;target_tick:number;ready:boolean;
  signature_verified:true;complete_verified:boolean;verified_records:number;total_records:number;
  map:MapInfo;observation:FactionObservation;events:readonly RtsEvent[];events_truncated:number;from_record:number;
}
export interface OpenPlayback extends PlaybackFrame {cursor_id:string}
function uuid(value:string):string {
  if(!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value))throw new Error("世界/对局/游标必须是UUID");
  return value;
}
export class ArenaWorldClient {
  readonly origin:string;
  constructor(origin:string,private token:string,private readonly fetcher:typeof fetch=fetch){
    const u=new URL(origin);
    if(!["http:","https:"].includes(u.protocol)||u.username||u.password||u.pathname!=="/"||u.search||u.hash)throw new Error("服务地址只允许HTTP(S) origin");
    if(!token.trim())throw new Error("缺少内存凭据");
    this.origin=u.origin;
  }
  destroy():void{this.token="";}
  private headers():Record<string,string>{if(!this.token)throw new Error("身份已断开");return {Authorization:`Bearer ${this.token}`,"Content-Type":"application/json"};}
  private async request<T>(path:string,method:string,body:unknown,signal?:AbortSignal):Promise<T>{
    const r=await this.fetcher(`${this.origin}/api/v1/${path}`,{method,headers:this.headers(),cache:"no-store",redirect:"error",...(signal?{signal}:{}),...(body===undefined?{}:{body:JSON.stringify(body)})});
    if(!r.ok){let message="";try{const error=await r.json() as {message?:string};message=error.message??"";}catch{}throw new Error(`权威请求拒绝 ${r.status} ${message}`);}
    return await r.json() as T;
  }
  async run(world:string,signal?:AbortSignal):Promise<WorldRun>{
    const requested=uuid(world).toLowerCase();
    const result=await this.request<WorldRun>(`worlds/${requested}/arena`,"POST",undefined,signal);
    if(result.world_id!==requested||!/^\d+$/.test(result.principal_id)||result.faction!==0||result.mode!=="PvE_Survival")throw new Error("真实席位/世界回执不一致，停止连接");
    uuid(result.match_id);return result;
  }
  streamURL(world:string):string{const url=new URL(`/api/v1/worlds/${uuid(world)}/stream?format=arena-json`,this.origin);url.protocol=url.protocol==="https:"?"wss:":"ws:";return url.toString();}
  hello(message:ArenaClientMessage):object{if(!this.token)throw new Error("身份已断开");return message.type==="hello"?{...message,capability_token:this.token}:message;}
  async replay(match:string,signal?:AbortSignal):Promise<Blob>{
    const r=await this.fetcher(`${this.origin}/api/v1/matches/${uuid(match)}/replay`,{headers:this.headers(),cache:"no-store",redirect:"error",...(signal?{signal}:{})});
    if(!r.ok)throw new Error(`签名归档未就绪或请求被拒绝：${r.status}`);return await r.blob();
  }
  async openPlayback(match:string,world:string,faction:number,signal?:AbortSignal):Promise<OpenPlayback>{
    const result=await this.request<OpenPlayback>(`matches/${uuid(match)}/playback`,"POST",{faction},signal);
    this.checkFrame(result,match,world,faction);uuid(result.cursor_id);return result;
  }
  async seek(match:string,cursor:string,world:string,faction:number,tick:number,signal?:AbortSignal):Promise<PlaybackFrame>{
    if(!Number.isSafeInteger(tick)||tick<0)throw new Error("目标Tick必须是安全非负整数");
    const frame=await this.request<PlaybackFrame>(`matches/${uuid(match)}/playback/${uuid(cursor)}`,"POST",{tick,max_steps:120},signal);
    this.checkFrame(frame,match,world,faction);return frame;
  }
  closeOwnedPlayback(match:string):Promise<unknown>{return this.request(`matches/${uuid(match)}/playback`,"DELETE",undefined);}
  closePlayback(match:string,cursor:string):Promise<unknown>{return this.request(`matches/${uuid(match)}/playback/${uuid(cursor)}`,"DELETE",undefined);}
  private checkFrame(frame:PlaybackFrame,match:string,world:string,faction:number):void{
    if(frame.match_id!==match||frame.world_id!==world||frame.faction!==faction||frame.observation?.faction!==faction
      ||frame.observation.tick!==frame.tick||frame.signature_verified!==true||!/^([0-9a-f]{64})$/.test(frame.state_root)
      ||![frame.tick,frame.initial_tick,frame.final_tick,frame.target_tick,frame.verified_records,frame.total_records].every(n=>Number.isSafeInteger(n)&&n>=0)
      ||!["PvE_Survival","PvP_Ranked"].includes(frame.mode)||!/^\d+$/.test(frame.initial_revision)||!/^([0-9a-f]{64})$/.test(frame.initial_state_root)
      ||!Array.isArray(frame.events)||frame.events.length>256||![frame.from_record,frame.events_truncated].every(n=>Number.isSafeInteger(n)&&n>=0)
      ||frame.from_record>frame.verified_records||(frame.ready&&frame.tick!==frame.target_tick)
      ||frame.tick<frame.initial_tick||frame.tick>frame.final_tick||frame.verified_records>frame.total_records
      ||(frame.complete_verified&&(frame.tick!==frame.final_tick||frame.verified_records!==frame.total_records))){
      throw new Error("验签重演回执的世界/阵营/范围不一致，停止显示");
    }
  }
}

/** 本地只读消息适配，唯一数据源是服务端验签并逐记录重演的 frame。
 * welcome 仅装载真实地图到既有客户端 reducer，不代表运行席位/租约；任何写请求立即拒绝。
 */
export class ReplayObservationTransport implements ArenaTransport {
  private readonly handlers=new Set<(message:ArenaServerMessage)=>void>();
  private closed=false;
  constructor(private frame:PlaybackFrame){}
  onMessage(handler:(message:ArenaServerMessage)=>void):()=>void{this.handlers.add(handler);return ()=>this.handlers.delete(handler);}
  send(message:ArenaClientMessage):void{
    if(this.closed)throw new Error("只读回放已关闭");
    if(message.type!=="hello")throw new Error("回放没有运行席位/租约，禁止写命令");
    this.publish(this.frame);
  }
  publish(frame:PlaybackFrame):void{
    if(this.closed)return;this.frame=frame;
    for(const h of [...this.handlers])h({type:"welcome",protocol:"rynvek-arena/1",faction:frame.faction,mode:frame.mode,tick:frame.tick,map:frame.map,tools:[]});
    for(const h of [...this.handlers])h({type:"observation",observation:frame.observation,events:frame.events});
  }
  close():void{this.closed=true;this.handlers.clear();}
}
