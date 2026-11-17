// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/** 当前权威世界的 HTTP/WS 与只读重演适配。不造世界、帧、资源、基因或本地仿真。 */
import type { ArenaClientMessage, ArenaServerMessage, ArenaTransport, FactionObservation, MapInfo, MatchMode, RtsEvent } from "@rynvek/client";

export interface WorldRun {
  world_id:string;match_id:string;principal_id:string;faction:number;mode:MatchMode;
  match_state:string;tick:number;recovered:boolean;custom?:boolean;
}
export interface RankedSeat {
  status:"matched"; principal_id:string; match_id:string; world_id:string;
  faction:number; mode:MatchMode; league:string; match_state:string; tick:number;
  host:{state:"local"|"remote"|"recoverable";origin:string|null;expires_at_s:number|null};
}
export interface QueueContext {
  principal_id:string; league:"human"; ruleset_hash:string;
  status:"idle"|"queued"|"matched"; active:RankedSeat|null;
}
export type QueueReply = RankedSeat | {status:"queued";position:number;rating:number} | {status:"cancelled"|"not_queued"};
export interface CustomRoomView {
  world_id:string;principal_id:string;role:"owner"|"player";status:"unconfigured"|"invited"|"ready"|"active"|"ended"|"closed";
  custom:true;owner_id?:string;guest_id?:string;faction?:number;version?:string;accepted?:boolean;seat?:WorldRun|null;
}
export interface ObserverGrant {principal_id:string;faction:number;grant_id:string;role:"observer"}
export interface ObserverFrame extends ObserverGrant {
  world_id:string;match_id:string;principal_kind:"Human"|"Agent"|"Service";
  messages:{welcome:Extract<ArenaServerMessage,{type:"welcome"}>;observation:Extract<ArenaServerMessage,{type:"observation"}>};
}
function checkGrant(g:ObserverGrant):void{
  uuid(g.grant_id);if(g.role!=="observer"||!Number.isSafeInteger(g.faction)||g.faction<0||g.faction>7||!/^[1-9]\d*$/.test(g.principal_id))throw new Error("观战准入回执非法");
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
  async queueContext(signal?:AbortSignal):Promise<QueueContext>{
    const c=await this.request<QueueContext>("matches/queue","GET",undefined,signal);
    if(!/^[1-9]\d*$/.test(c.principal_id)||c.league!=="human"||!/^[0-9a-f]{64}$/.test(c.ruleset_hash)
      ||!["idle","queued","matched"].includes(c.status)||(c.status==="matched")!==Boolean(c.active))throw new Error("匹配身份/规则集回执不合法");
    if(c.active)this.checkSeat(c.active,c.principal_id);
    return c;
  }
  async queue(ruleset:string,cancel:boolean,principal:string,signal?:AbortSignal,poll=false):Promise<QueueReply>{
    if(!/^[0-9a-f]{64}$/.test(ruleset))throw new Error("必须先读取当前规则集");
    const r=await this.request<QueueReply>("matches/queue","POST",{league:"human",ruleset_hash:ruleset,cancel,poll},signal);
    if(r.status==="matched")this.checkSeat(r,principal);
    else if(r.status==="queued"){if(!Number.isSafeInteger(r.position)||r.position<1||!Number.isFinite(r.rating))throw new Error("队列回执不合法");}
    else if(!["cancelled","not_queued"].includes(r.status))throw new Error("未知匹配状态");
    return r;
  }
  async connectRanked(match:string,principal:string,signal?:AbortSignal):Promise<RankedSeat>{
    const id=uuid(match).toLowerCase();
    const seat=await this.request<RankedSeat>(`matches/${id}/connect`,"POST",undefined,signal);
    this.checkSeat(seat,principal);
    if(seat.match_id!==id||seat.mode!=="PvP_Ranked"||seat.league!=="human")throw new Error("连接回执不是本人当前人类排位");
    if(seat.host.state!=="local")throw new Error(seat.host.state==="remote"?`当前健康宿主位于 ${seat.host.origin}；请核对可信服务地址后手动切换并重新验证身份，未向该地址转发凭据。`:"宿主恢复尚未确认，请刷新后重试");
    return seat;
  }
  private checkSeat(seat:RankedSeat,principal:string):void{
    uuid(seat.match_id);uuid(seat.world_id);
    if(seat.principal_id!==principal||![0,1].includes(seat.faction)||seat.mode!=="PvP_Ranked"
      ||!Number.isSafeInteger(seat.tick)||seat.tick<0)throw new Error("真实身份/席位不匹配");
    if(!seat.host||!["local","remote","recoverable"].includes(seat.host.state)
      ||(seat.host.expires_at_s!==null&&(!Number.isSafeInteger(seat.host.expires_at_s)||seat.host.expires_at_s<0)))throw new Error("宿主发现回执非法");
    if(seat.host.origin!==null){const u=new URL(seat.host.origin);if(!["http:","https:"].includes(u.protocol)||u.username||u.password||u.pathname!=="/"||u.search||u.hash)throw new Error("宿主只允许无凭据的HTTP(S) origin");}
    if(seat.host.state==="remote"&&!seat.host.origin)throw new Error("远端宿主没有公布可信入口，请联系部署管理员");
  }
  async createCustom(title:string,mapSeed:number,signal?:AbortSignal):Promise<string>{
    if(!title.trim()||!Number.isSafeInteger(mapSeed)||mapSeed<0)throw new Error("私有自创标题/seed非法");
    const r=await this.request<{world_id:string}>("worlds","POST",{title,template:"arena_duel",visibility:"Private",map_seed:mapSeed},signal);uuid(r.world_id);return r.world_id;
  }
  async customRoom(world:string,action?:{action:"invite";guest_id:string}|{action:"accept"|"connect"|"close";version:string},signal?:AbortSignal):Promise<CustomRoomView>{
    if(action?.action==="invite"&&!/^[1-9]\d*$/.test(action.guest_id))throw new Error("须输入正式受邀主体ID");
    const r=await this.request<CustomRoomView>(`worlds/${uuid(world)}/multiplayer`,action?"POST":"GET",action,signal);
    if(r.world_id!==world||r.custom!==true||!/^[1-9]\d*$/.test(r.principal_id)||!["owner","player"].includes(r.role)
      ||!["unconfigured","invited","ready","active","ended","closed"].includes(r.status))throw new Error("自创房间身份/状态不合法");
    if(r.status!=="unconfigured"){
      uuid(r.version!);if(![0,1].includes(r.faction!)||!r.owner_id||!r.guest_id||(r.role==="owner"?r.owner_id:r.guest_id)!==r.principal_id)throw new Error("房间角色/席位不一致");
    }
    if(r.seat){const s=r.seat;uuid(s.match_id);if(s.world_id!==world||s.principal_id!==r.principal_id||s.faction!==r.faction||s.mode!=="PvP_Ranked"||s.custom!==true)throw new Error("自创实际控制席位不一致");}
    return r;
  }
  async observerGrants(world:string,signal?:AbortSignal):Promise<ObserverGrant[]>{
    const result=await this.request<{world_id:string;grants:ObserverGrant[]}>(`worlds/${uuid(world)}/observers`,"GET",undefined,signal);
    if(result.world_id!==world||!Array.isArray(result.grants)||result.grants.length>32)throw new Error("观战清单世界/范围不一致");
    result.grants.forEach(checkGrant);return result.grants;
  }
  async grantObserver(world:string,principal:string,faction:number,signal?:AbortSignal):Promise<ObserverGrant>{
    if(!/^[1-9]\d*$/.test(principal))throw new Error("请输入正式主体ID，不是显示名/凭据");
    const g=await this.request<ObserverGrant>(`worlds/${uuid(world)}/observers`,"POST",{principal_id:principal,faction},signal);
    checkGrant(g);if(g.principal_id!==principal||g.faction!==faction)throw new Error("准入目标不一致");return g;
  }
  async revokeObserver(world:string,grant:ObserverGrant,signal?:AbortSignal):Promise<void>{
    checkGrant(grant);const r=await this.request<{status:string}>(`worlds/${uuid(world)}/observers/${grant.principal_id}`,"DELETE",{grant_id:grant.grant_id},signal);
    if(r.status!=="revoked")throw new Error("未确认撤销准入");
  }
  async observe(world:string,signal?:AbortSignal):Promise<ObserverFrame>{
    const r=await this.request<ObserverFrame>(`worlds/${uuid(world)}/observation`,"GET",undefined,signal);checkGrant(r);uuid(r.match_id);
    const w=r.messages?.welcome,o=r.messages?.observation;
    if(r.world_id!==world||!["Human","Agent","Service"].includes(r.principal_kind)||w?.type!=="welcome"||w.protocol!=="rynvek-arena/1"||w.faction!==r.faction
      ||!Array.isArray(w.tools)||w.tools.length!==0||o?.type!=="observation"||o.observation.faction!==r.faction
      ||o.observation.tick!==w.tick||!Number.isSafeInteger(w.tick)||w.tick<0||!Array.isArray(o.events))throw new Error("观战世界/阵营/投影回执非法");
    return r;
  }
  async leaveObserver(world:string,grant:ObserverGrant,signal?:AbortSignal):Promise<void>{
    checkGrant(grant);const r=await this.request<{status:string}>(`worlds/${uuid(world)}/observation`,"DELETE",{grant_id:grant.grant_id},signal);
    if(r.status!=="left")throw new Error("未确认离开准入");
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

/** 唯一数据源为原Actor的真实阵营投影；无WS席位/租约，任何写命令在适配层拒绝。 */
export class ObserverTransport implements ArenaTransport {
  private readonly handlers=new Set<(message:ArenaServerMessage)=>void>();private closed=false;private ready=false;
  constructor(private frame:ObserverFrame){}
  onMessage(handler:(message:ArenaServerMessage)=>void):()=>void{this.handlers.add(handler);return ()=>this.handlers.delete(handler);}
  send(message:ArenaClientMessage):void{
    if(this.closed)throw new Error("观战已关闭");
    if(message.type!=="hello")throw new Error("观察者没有控制席位/租约，禁止写命令");
    this.ready=true;for(const h of [...this.handlers])h(this.frame.messages.welcome);this.publish(this.frame);
  }
  publish(frame:ObserverFrame):void{
    if(this.closed)return;
    if(frame.world_id!==this.frame.world_id||frame.match_id!==this.frame.match_id||frame.grant_id!==this.frame.grant_id
      ||frame.principal_id!==this.frame.principal_id||frame.faction!==this.frame.faction||frame.messages.welcome.tick<this.frame.messages.welcome.tick)throw new Error("观战准入/席位已改变，须重新确认");
    this.frame=frame;if(this.ready)for(const h of [...this.handlers])h(frame.messages.observation);
  }
  close():void{this.closed=true;this.handlers.clear();}
}
