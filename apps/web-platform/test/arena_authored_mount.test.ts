// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
// JSDOM传输/生命周期夹具，不是实际受限浏览器、GPU或仿真验收。
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {afterEach,expect,it,vi} from "vitest";
const world="10000000-0000-4000-8000-000000000001",match="10000000-0000-4000-8000-000000000002";
const transcript=JSON.parse(readFileSync(resolve(process.cwd(),"../../tests/arena/opening.transcript.json"),"utf8"));
const welcome=transcript[0].recv[0].message,observation=transcript[5].recv.find((e:{to:string})=>e.to==="human:p0").message;
const run={world_id:world,match_id:match,principal_id:"123",faction:0,mode:"PvE_Survival",match_state:"Running",tick:1,recovered:false};
class Socket extends EventTarget {
 static all:Socket[]=[];sent:Record<string,unknown>[]=[];closed=false;
 onopen:((event:Event)=>void)|null=null;onerror:((event:Event)=>void)|null=null;onclose:((event:Event)=>void)|null=null;
 constructor(readonly url:string){super();Socket.all.push(this);queueMicrotask(()=>this.onopen?.(new Event("open")));}
 receive(message:unknown){this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify(message)}));}
 send(raw:string){const m=JSON.parse(raw);this.sent.push(m);
  if(m.type==="hello"){this.receive({...welcome,mode:"PvE_Survival"});this.receive(observation);}
  if(m.type==="acquire_lease")this.receive({type:"lease",seq:m.seq,lease:{holder:{id:"123",kind:"Human"},faction:0,level:"Sovereign",fencing_token:11,issued_tick:1,expires_tick:900}});
  if(m.type==="command"){const c=m.command as Record<string,unknown>;const ack=c.type==="queue_unit"?{type:"queued",vat:c.vat,count:c.count}:c.type==="cancel_queue"?{type:"cancelled",vat:c.vat}:c.type==="research_tech"?{type:"research_started",site:c.site,tech:c.tech}:{type:"cancelled",vat:0};this.receive({type:"ack",seq:m.seq,tick:1,ack});}
 }
 close(){this.closed=true;this.onclose?.(new Event("close"));}
}
function field(id:string){return document.getElementById(id) as HTMLInputElement;}
async function mount(fetcher:typeof fetch){
 vi.resetModules();Socket.all=[];window.history.replaceState({},"",`/arena?world=${world}`);document.body.innerHTML='<div id="app"></div>';
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
