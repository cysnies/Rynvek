// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
// JSDOM传输/生命周期夹具，不是实际受限浏览器、GPU或仿真验收。
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {afterEach,expect,it,vi} from "vitest";
import {FakeAudioContext, FakeGainNode} from "@rynvek/audio/testing";
const world="10000000-0000-4000-8000-000000000001",match="10000000-0000-4000-8000-000000000002";
const transcript=JSON.parse(readFileSync(resolve(process.cwd(),"../../tests/arena/opening.transcript.json"),"utf8"));
const welcome=transcript[0].recv[0].message,observation=transcript[5].recv.find((e:{to:string})=>e.to==="human:p0").message;
const run={world_id:world,match_id:match,principal_id:"123",faction:0,mode:"PvE_Survival",match_state:"Running",tick:1,recovered:false};
class Socket extends EventTarget {
 static mode="PvE_Survival";static all:Socket[]=[];sent:Record<string,unknown>[]=[];closed=false;
 onopen:((event:Event)=>void)|null=null;onerror:((event:Event)=>void)|null=null;onclose:((event:Event)=>void)|null=null;
 constructor(readonly url:string){super();Socket.all.push(this);queueMicrotask(()=>this.onopen?.(new Event("open")));}
 receive(message:unknown){this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify(message)}));}
 send(raw:string){const m=JSON.parse(raw);this.sent.push(m);
  if(m.type==="hello"){this.receive({...welcome,mode:Socket.mode,faction:m.faction});const obs=structuredClone(observation);obs.observation.faction=m.faction;for(const u of [...obs.observation.units,...obs.observation.buildings])u.faction=m.faction;this.receive(obs);}
  if(m.type==="acquire_lease")this.receive({type:"lease",seq:m.seq,lease:{holder:{id:"123",kind:"Human"},faction:0,level:"Sovereign",fencing_token:11,issued_tick:1,expires_tick:900}});
  if(m.type==="command"){const c=m.command as Record<string,unknown>;const ack=c.type==="queue_unit"?{type:"queued",vat:c.vat,count:c.count}:c.type==="cancel_queue"?{type:"cancelled",vat:c.vat}:c.type==="research_tech"?{type:"research_started",site:c.site,tech:c.tech}:{type:"cancelled",vat:0};this.receive({type:"ack",seq:m.seq,tick:1,ack});}
 }
 close(){this.closed=true;this.onclose?.(new Event("close"));}
}
function field(id:string){return document.getElementById(id) as HTMLInputElement;}
async function mount(fetcher:typeof fetch){
 vi.resetModules();Socket.mode="PvE_Survival";Socket.all=[];window.history.replaceState({},"",`/arena?world=${world}`);document.body.innerHTML='<div id="app"></div>';
 vi.spyOn(HTMLCanvasElement.prototype,"getContext").mockReturnValue({setTransform:vi.fn(),fillRect:vi.fn(),fillText:vi.fn()} as unknown as CanvasRenderingContext2D);
 vi.stubGlobal("requestAnimationFrame",vi.fn(()=>1));vi.stubGlobal("cancelAnimationFrame",vi.fn());vi.stubGlobal("fetch",fetcher);vi.stubGlobal("WebSocket",Socket);
 await import("../src/browser.js");field("api-origin").value="http://srv";field("capability").value="test-memory-secret";
}
afterEach(()=>{window.dispatchEvent(new Event("pagehide"));vi.restoreAllMocks();vi.unstubAllGlobals();});
it("Studio已有世界真实挂载原控制器，命令经WS/租约；不走practice或持久化令牌",async()=>{
 const store=vi.spyOn(Storage.prototype,"setItem"),fetcher=vi.fn(async(_url:unknown,_init?:RequestInit)=>Response.json(run));await mount(fetcher);
 (field("connect-world") as HTMLButtonElement).click();await vi.waitFor(()=>expect(document.getElementById("connection")?.textContent).toContain("已获控制租约"));
 expect(fetcher.mock.calls[0]?.[0]).toBe(`http://srv/api/v1/worlds/${world}/arena`);expect(fetcher).toHaveBeenCalledTimes(1);
 const socket=Socket.all[0]!;expect(socket.url).toContain(world);expect(socket.url).not.toContain("secret");expect(socket.sent[0]).toMatchObject({type:"hello",capability_token:"test-memory-secret"});
 expect(field("capability").value).toBe("");expect((field("move") as HTMLButtonElement).disabled).toBe(false);
 expect(document.getElementById("command-json")).toBeNull();
 expect(document.getElementById("build")?.textContent).toContain("精炼厂");
 expect(document.getElementById("build")?.textContent).toContain("坩埚");
 const live=structuredClone(observation.observation);
 live.buildings.push({id:42,faction:0,kind:"bld.spawning_vat",origin:[8,8],hp_milli:900000,max_hp_milli:900000,complete:true,powered:true,build_progress:100,queue:["unit.forager"],research:null,beacon:null});
 socket.receive({type:"observation",observation:live,events:[]});
 await vi.waitFor(()=>expect(document.querySelector('[data-action="select-building"][data-building-id="42"]')).not.toBeNull());
 (document.querySelector('[data-action="select-building"][data-building-id="42"]') as HTMLButtonElement).click();
 await vi.waitFor(()=>expect((document.querySelector('[data-action="cancel-queue"]') as HTMLButtonElement|null)?.disabled).toBe(false));
 (document.querySelector('[data-action="cancel-queue"]') as HTMLButtonElement).click();
 await vi.waitFor(()=>expect(socket.sent.some(m=>m.type==="command")).toBe(true));expect(socket.sent.at(-1)).toMatchObject({type:"command",fencing_token:11,command:{type:"cancel_queue",vat:42}});
 (field("disconnect") as HTMLButtonElement).click();expect(socket.closed).toBe(true);expect(document.getElementById("biomass")?.textContent).toBe("—");
 expect([...document.querySelectorAll("#command-panel [data-action]")].filter((b)=> (b as HTMLButtonElement).dataset.action!=="select-building").every((b)=>(b as HTMLButtonElement).disabled)).toBe(true);expect(store).not.toHaveBeenCalled();
});
it("身份断开后的迟到run回执不能重新连入或恢复私有状态",async()=>{
 let resolve!:(r:Response)=>void;const fetcher=vi.fn((_url:unknown,_init?:RequestInit)=>new Promise<Response>(r=>{resolve=r}));await mount(fetcher);
 (field("connect-world") as HTMLButtonElement).click();await vi.waitFor(()=>expect(fetcher).toHaveBeenCalledTimes(1));(field("disconnect") as HTMLButtonElement).click();
 resolve(Response.json(run));await new Promise(r=>setTimeout(r,0));expect(Socket.all).toHaveLength(0);expect(document.getElementById("connection")?.textContent).toBe("身份已断开");expect(field("capability").value).toBe("");expect(document.getElementById("biomass")?.textContent).toBe("—");
});
it("终局已有世界打开验签前缀播放器，只读控件不向运行Actor发任何命令",async()=>{
 const frame={...run,cursor_id:"10000000-0000-4000-8000-000000000003",initial_tick:1,final_tick:10,tick:1,revision:"1",state_root:"ab".repeat(32),initial_revision:"1",initial_state_root:"ab".repeat(32),target_tick:1,ready:true,signature_verified:true,complete_verified:false,verified_records:0,total_records:9,map:welcome.map,observation:observation.observation,events:[],events_truncated:0,from_record:0};
 const fetcher=vi.fn(async(url:unknown,init?:RequestInit)=>String(url).endsWith("/arena")?Response.json({...run,match_state:"Completed"}):init?.method==="DELETE"?Response.json({closed:true}):Response.json(frame));await mount(fetcher);
 (field("connect-world") as HTMLButtonElement).click();await vi.waitFor(()=>expect((field("watch-replay") as HTMLButtonElement).disabled).toBe(false));(field("watch-replay") as HTMLButtonElement).click();
 await vi.waitFor(()=>expect(document.getElementById("connection")?.textContent).toContain("只读签名重演"));expect(Socket.all).toHaveLength(0);
 expect((field("move") as HTMLButtonElement).disabled).toBe(true);expect(document.getElementById("command-json")).toBeNull();expect([...document.querySelectorAll("#command-panel [data-action]")].filter((b)=> (b as HTMLButtonElement).dataset.action!=="select-building").every((b)=>(b as HTMLButtonElement).disabled)).toBe(true);expect(document.getElementById("replay-status")?.textContent).toContain("未验证后缀");
 (field("close-replay") as HTMLButtonElement).click();await vi.waitFor(()=>expect(fetcher.mock.calls.some(([,init])=>init?.method==="DELETE")).toBe(true));expect(document.getElementById("biomass")?.textContent).toBe("—");
});

const observerGrant={principal_id:"789",faction:0,grant_id:"10000000-0000-4000-8000-000000000004",role:"observer"};
const observerFrame={...observerGrant,principal_kind:"Human",world_id:world,match_id:match,messages:{welcome:{...welcome,tick:observation.observation.tick,tools:[]},observation}};
it("只读观战使用原Renderer，拒绝控制且撤销后清空画面，不创建WS或practice",async()=>{
 let reads=0;const storage=vi.spyOn(Storage.prototype,"setItem");
 const fetcher=vi.fn(async(_url:unknown,init?:RequestInit)=>{expect(init?.method).toBe("GET");return ++reads===1?Response.json(observerFrame):Response.json({message:"准入不存在"},{status:404})});await mount(fetcher);
 field("observe-world").click();await vi.waitFor(()=>expect(field("connection").textContent).toContain("定向只读观战"));
 expect(field("biomass").textContent).not.toBe("—");expect(field("move").disabled).toBe(true);expect(field("claim-control").disabled).toBe(true);expect(Socket.all).toHaveLength(0);
 expect(fetcher.mock.calls[0]?.[0]).toBe(`http://srv/api/v1/worlds/${world}/observation`);expect(field("capability").value).toBe("");
 await vi.waitFor(()=>expect(field("connection").textContent).toContain("观战已停止"));expect(field("biomass").textContent).toBe("—");expect(storage).not.toHaveBeenCalled();
 expect(fetcher.mock.calls.every(([url])=>!String(url).includes("secret"))).toBe(true);
});
it("离开先停拉取，按原准入版本撤销，主动取消的在途读取不冒充失败",async()=>{
 let reads=0;
 const fetcher=vi.fn(async(_url:unknown,init?:RequestInit)=>{
  if(init?.method==="DELETE")return Response.json({status:"left"});
  if(++reads===1)return Response.json(observerFrame);
  return new Promise<Response>((_resolve,reject)=>init?.signal?.addEventListener("abort",()=>reject(new DOMException("取消","AbortError")),{once:true}));
 });await mount(fetcher);field("observe-world").click();await vi.waitFor(()=>expect(reads).toBe(2));field("leave-observer").click();
 await vi.waitFor(()=>expect(field("observer-status").textContent).toContain("服务器已确认离开"));expect(field("biomass").textContent).toBe("—");
 const leave=fetcher.mock.calls.find(([,init])=>init?.method==="DELETE")!;expect(JSON.parse(String(leave[1]?.body))).toEqual({grant_id:observerGrant.grant_id});expect(Socket.all).toHaveLength(0);
});
it("断开身份后的迟到观战响应不得恢复画面或安排轮询",async()=>{
 let resolve!:(r:Response)=>void;const fetcher=vi.fn(()=>new Promise<Response>(r=>{resolve=r}));await mount(fetcher);
 field("observe-world").click();await vi.waitFor(()=>expect(fetcher).toHaveBeenCalledTimes(1));field("disconnect").click();resolve(Response.json(observerFrame));
 await new Promise(r=>setTimeout(r,0));expect(field("biomass").textContent).toBe("—");expect(field("connection").textContent).toBe("身份已断开");expect(Socket.all).toHaveLength(0);expect(fetcher).toHaveBeenCalledTimes(1);
});
it("所有者从真实席位授予当前阵营观战，清单回执后展示并版本化撤销",async()=>{
 let active=false;const fetcher=vi.fn(async(url:unknown,init?:RequestInit)=>{
  if(String(url).endsWith("/arena"))return Response.json(run);
  if(init?.method==="POST"){active=true;return Response.json(observerGrant);}
  if(init?.method==="DELETE"){active=false;return Response.json({status:"revoked"});}
  return Response.json({world_id:world,grants:active?[observerGrant]:[]});
 });await mount(fetcher);field("connect-world").click();await vi.waitFor(()=>expect(field("connection").textContent).toContain("已获控制租约"));
 field("observer-principal").value="789";field("grant-observer").click();await vi.waitFor(()=>expect(field("observer-grants").textContent).toContain("主体 789"));
 const posted=fetcher.mock.calls.find(([url,init])=>String(url).endsWith("/observers")&&init?.method==="POST")!;expect(JSON.parse(String(posted[1]?.body))).toEqual({principal_id:"789",faction:0});
 (field("observer-grants").querySelector("button") as HTMLButtonElement).click();await vi.waitFor(()=>expect(field("observer-grants").textContent).toContain("当前没有"));
 const revoked=fetcher.mock.calls.find(([,init])=>init?.method==="DELETE")!;expect(JSON.parse(String(revoked[1]?.body))).toEqual({grant_id:observerGrant.grant_id});
});

const roomVersion="10000000-0000-4000-8000-000000000005";
it("自创所有者显式邀请/冻结，原租约可在大厅等待，Running前禁写且撤销等真实回执",async()=>{
 let stage="unconfigured",phase="Lobby";let close!:((r:Response)=>void);
 const room=()=>({world_id:world,principal_id:"123",owner_id:"123",guest_id:"789",role:"owner",status:stage,custom:true,faction:0,version:roomVersion,accepted:stage!=="invited",seat:stage==="active"?{...run,mode:"PvP_Ranked",custom:true,match_state:phase}:null});
 const fetcher=vi.fn(async(_url:unknown,init?:RequestInit)=>{
  const body=init?.body?JSON.parse(String(init.body)):null;
  if(body?.action==="invite"){expect(body.guest_id).toBe("789");stage="invited";return Response.json(room());}
  if(body?.action==="connect"){expect(body.version).toBe(roomVersion);stage="active";return Response.json(room());}
  if(body?.action==="close")return new Promise<Response>(r=>{close=r});
  return Response.json(room());
 });await mount(fetcher);Socket.mode="PvP_Ranked";
 field("custom-load").click();await vi.waitFor(()=>expect(field("custom-status").textContent).toContain("unconfigured"));
 field("custom-guest").value="789";field("custom-invite").click();await vi.waitFor(()=>expect(field("custom-status").textContent).toContain("invited"));expect(field("custom-accept").disabled).toBe(true);expect(field("custom-connect").disabled).toBe(true);
 stage="ready";field("custom-load").click();await vi.waitFor(()=>expect(field("custom-connect").disabled).toBe(false));field("custom-connect").click();await vi.waitFor(()=>expect(field("connection").textContent).toContain("私有自创席位已连接"));
 expect(field("move").disabled).toBe(true);expect(Socket.all[0]!.sent.some(m=>m.type==="acquire_lease")).toBe(true);
 phase="Running";await vi.waitFor(()=>expect(field("move").disabled).toBe(false));expect(field("surrender").textContent).toContain("非排位");
 field("custom-close").click();await vi.waitFor(()=>expect(close).toBeTypeOf("function"));expect(field("move").disabled).toBe(true);expect(field("connection").textContent).not.toContain("授权已关闭");
 stage="closed";close(Response.json(room()));await vi.waitFor(()=>expect(field("connection").textContent).toContain("授权已关闭"));expect(Socket.all[0]!.closed).toBe(true);expect(field("biomass").textContent).toBe("—");expect(fetcher.mock.calls.every(([url])=>!String(url).includes("practice")&&!String(url).includes("secret"))).toBe(true);
});
it("受邀玩家本人确认后仍等待所有者冻结，连接只授本人阵营且服务撤销后清空",async()=>{
 let stage="invited";const guestRun={...run,principal_id:"789",faction:1,mode:"PvP_Ranked",custom:true};
 const room=()=>({world_id:world,principal_id:"789",owner_id:"123",guest_id:"789",role:"player",status:stage,custom:true,faction:1,version:roomVersion,accepted:stage!=="invited",seat:stage==="active"?guestRun:null});
 const fetcher=vi.fn(async(_url:unknown,init?:RequestInit)=>{const body=init?.body?JSON.parse(String(init.body)):null;if(body?.action==="accept"){stage="ready";}return Response.json(room())});await mount(fetcher);Socket.mode="PvP_Ranked";
 field("custom-load").click();await vi.waitFor(()=>expect(field("custom-accept").disabled).toBe(false));expect(field("custom-invite").disabled).toBe(true);field("custom-accept").click();await vi.waitFor(()=>expect(field("custom-status").textContent).toContain("ready"));expect(field("custom-connect").disabled).toBe(true);
 stage="active";field("custom-load").click();await vi.waitFor(()=>expect(field("custom-connect").disabled).toBe(false));field("custom-connect").click();await vi.waitFor(()=>expect(field("connection").textContent).toContain("私有自创席位已连接"));expect(Socket.all[0]!.sent[0]).toMatchObject({principal_id:"789",faction:1});expect(field("grant-observer").disabled).toBe(true);
 stage="closed";await vi.waitFor(()=>expect(field("biomass").textContent).toBe("—"));expect(Socket.all[0]!.closed).toBe(true);expect(field("move").disabled).toBe(true);
});
it("显式创建私有双人世界保留实际ID；断开后迟到邀请回执不恢复身份",async()=>{
 let resolve!:((r:Response)=>void);const fetcher=vi.fn(async(url:unknown,init?:RequestInit)=>{
  if(String(url).endsWith("/worlds")){expect(JSON.parse(String(init?.body))).toMatchObject({template:"arena_duel",visibility:"Private",map_seed:42});return Response.json({world_id:world});}
  const body=init?.body?JSON.parse(String(init.body)):null;if(body?.action==="invite")return new Promise<Response>(r=>{resolve=r});
  return Response.json({world_id:world,principal_id:"123",role:"owner",status:"unconfigured",custom:true});
 });await mount(fetcher);field("custom-create").click();await vi.waitFor(()=>expect(field("custom-status").textContent).toContain("unconfigured"));expect(field("world-id").value).toBe(world);
 field("custom-guest").value="789";field("custom-invite").click();await vi.waitFor(()=>expect(resolve).toBeTypeOf("function"));field("disconnect").click();resolve(Response.json({world_id:world,principal_id:"123",owner_id:"123",guest_id:"789",role:"owner",status:"invited",version:roomVersion,custom:true,faction:0}));
 await new Promise(r=>setTimeout(r,0));expect(field("connection").textContent).toBe("身份已断开");expect(Socket.all).toHaveLength(0);expect(field("biomass").textContent).toBe("—");
});

it("实际Arena挂载需显式音频手势；原阵营WS事件入混音，静音/断线/换世界无旧声图",async()=>{
 const contexts:FakeAudioContext[]=[];
 class Context extends FakeAudioContext {constructor(_options?:AudioContextOptions){super(48000,"suspended");contexts.push(this);}}
 await mount(vi.fn(async()=>Response.json(run)));vi.stubGlobal("AudioContext",Context);
 field("connect-world").click();await vi.waitFor(()=>expect(field("connection").textContent).toContain("已获控制租约"));
 expect(contexts).toHaveLength(0);expect(field("audio-enable").disabled).toBe(false);
 field("audio-enable").click();await vi.waitFor(()=>expect(field("audio-status").textContent).toContain("音频已启用"));
 const frames=transcript.flatMap((s:{recv:{to:string;message:{type:string;observation?:unknown}}[]})=>s.recv.filter(r=>r.to==="human:p0"&&r.message.type==="observation").map(r=>r.message));
 Socket.all[0]!.receive(frames[2]);expect(contexts[0]!.nodesOfType("oscillator")).toHaveLength(1);
 field("audio-mute").click();expect(contexts[0]!.state).toBe("closed");
 expect(contexts[0]!.nodesOfType<FakeGainNode>("gain").some(g=>g.disconnected)).toBe(true);
 field("audio-enable").click();await vi.waitFor(()=>expect(contexts).toHaveLength(2));
 expect(contexts[1]!.nodesOfType("oscillator")).toHaveLength(0);
 Socket.all[0]!.close();expect(contexts[1]!.state).toBe("closed");expect(field("audio-enable").disabled).toBe(true);
 field("connect-world").click();await vi.waitFor(()=>expect(Socket.all).toHaveLength(2));
 expect(contexts).toHaveLength(2);expect(field("audio-status").textContent).not.toContain("音频已启用");
});
