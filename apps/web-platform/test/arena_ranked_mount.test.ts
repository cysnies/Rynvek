// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
// 本机传输夹具，不是实际浏览器、在线玩家或网络服务验收。
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {afterEach,expect,it,vi} from "vitest";
const transcript=JSON.parse(readFileSync(resolve(process.cwd(),"../../tests/arena/opening.transcript.json"),"utf8"));
const welcome=transcript[0].recv[0].message;
const initial=transcript[5].recv.find((e:{to:string})=>e.to==="human:p0").message.observation;
const world="10000000-0000-4000-8000-000000000001",match="10000000-0000-4000-8000-000000000002",hash="ab".repeat(32);
const seat={status:"matched",principal_id:"902",faction:1,mode:"PvP_Ranked",league:"human",match_id:match,world_id:world,match_state:"Lobby",tick:1};
const ctx=(status="idle",active:unknown=null)=>({principal_id:"902",ruleset_hash:hash,league:"human",status,active});
class Socket extends EventTarget{
 static all:Socket[]=[];static rejectLease=false;sent:Record<string,unknown>[]=[];closed=false;
 onopen:(()=>void)|null=null;onerror:(()=>void)|null=null;onclose:(()=>void)|null=null;
 constructor(readonly url:string){super();Socket.all.push(this);queueMicrotask(()=>this.onopen?.());}
 receive(value:unknown){this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify(value)}));}
 send(raw:string){const m=JSON.parse(raw);this.sent.push(m);
  if(m.type==="hello"){this.receive({...welcome,faction:1,mode:"PvP_Ranked"});const o=structuredClone(initial);o.faction=1;o.units=o.units.map((u:object)=>({...u,faction:1}));o.buildings=o.buildings.map((b:object)=>({...b,faction:1}));this.receive({type:"observation",observation:o,events:[]});}
  if(m.type==="acquire_lease")this.receive(Socket.rejectLease?{type:"lease",seq:m.seq,error:{code:32769,name:"UnauthorizedCapabilityToken",message:"租约拒绝"}}:{type:"lease",seq:m.seq,lease:{holder:{id:"902",kind:"Human"},faction:1,level:"Sovereign",fencing_token:44,issued_tick:1,expires_tick:9999}});
  if(m.type==="release_lease")this.receive({type:"lease",seq:m.seq,lease:null});
  if(m.type==="command")this.receive({type:"ack",seq:m.seq,tick:2,ack:{type:"surrendered"}});
 }
 close(){this.closed=true;this.onclose?.();}
}
const btn=(id:string)=>document.getElementById(id) as HTMLButtonElement;
const input=(id:string)=>document.getElementById(id) as HTMLInputElement;
async function mount(fetcher:typeof fetch){
 vi.resetModules();Socket.all=[];Socket.rejectLease=false;history.replaceState({},"","/arena");document.body.innerHTML='<div id="app"></div>';
 vi.spyOn(HTMLCanvasElement.prototype,"getContext").mockReturnValue({setTransform:vi.fn(),fillRect:vi.fn(),fillText:vi.fn()} as unknown as CanvasRenderingContext2D);
 vi.stubGlobal("requestAnimationFrame",vi.fn(()=>1));vi.stubGlobal("cancelAnimationFrame",vi.fn());vi.stubGlobal("WebSocket",Socket);vi.stubGlobal("fetch",fetcher);
 await import("../src/browser.js");input("api-origin").value="http://srv";input("capability").value="rank-memory-secret";
}
afterEach(()=>{window.dispatchEvent(new Event("pagehide"));vi.restoreAllMocks();vi.unstubAllGlobals();});
it("正式身份读取规则集后显式入队，真队列取消回执，不创建practice或自动对手",async()=>{
 let queued=false;const writes:unknown[]=[];
 const f=vi.fn(async(url:unknown,init?:RequestInit)=>{expect(String(url)).toBe("http://srv/api/v1/matches/queue");expect((init?.headers as Record<string,string>).Authorization).toBe("Bearer rank-memory-secret");
  if(init?.method==="POST"){const body=JSON.parse(String(init.body));writes.push(body);queued=!body.cancel;return Response.json(body.cancel?{status:"cancelled"}:{status:"queued",position:1,rating:1500});}
  return Response.json(ctx(queued?"queued":"idle"));});
 await mount(f);btn("rank-auth").click();await vi.waitFor(()=>expect(btn("rank-join").disabled).toBe(false));
 expect(input("capability").value).toBe("");expect(writes).toEqual([]);expect(Socket.all).toHaveLength(0);
 btn("rank-join").click();await vi.waitFor(()=>expect(btn("rank-cancel").disabled).toBe(false));expect(document.getElementById("rank-status")?.textContent).toContain("等待另一注册玩家");
 btn("rank-cancel").click();await vi.waitFor(()=>expect(btn("rank-join").disabled).toBe(false));
 expect(writes).toEqual([{league:"human",ruleset_hash:hash,cancel:false,poll:false},{league:"human",ruleset_hash:hash,cancel:true,poll:false}]);expect(Socket.all).toHaveLength(0);
});
it("服务端席位决定身份和阵营，经原握手租约命令进入排位，断开不假称离队",async()=>{
 const store=vi.spyOn(Storage.prototype,"setItem");const f=vi.fn(async()=>Response.json(ctx("matched",seat)));await mount(f);
 btn("rank-auth").click();await vi.waitFor(()=>expect(btn("rank-enter").disabled).toBe(false));btn("rank-enter").click();
 await vi.waitFor(()=>expect(btn("surrender").disabled).toBe(false));const socket=Socket.all[0]!;
 expect(socket.url).toContain(world);expect(socket.url).not.toContain("secret");expect(socket.sent[0]).toMatchObject({type:"hello",principal_id:"902",faction:1,capability_token:"rank-memory-secret"});
 btn("release-control").click();await vi.waitFor(()=>expect(btn("surrender").disabled).toBe(true));expect(btn("claim-control").disabled).toBe(false);
 btn("claim-control").click();await vi.waitFor(()=>expect(btn("surrender").disabled).toBe(false));
 btn("surrender").click();await vi.waitFor(()=>expect(socket.sent.some(m=>m.type==="command")).toBe(true));expect(socket.sent.at(-1)).toMatchObject({type:"command",fencing_token:44,command:{type:"surrender"}});
 socket.close();await vi.waitFor(()=>expect(btn("rank-enter").disabled).toBe(false));btn("rank-enter").click();await vi.waitFor(()=>expect(Socket.all).toHaveLength(2));await vi.waitFor(()=>expect(btn("surrender").disabled).toBe(false));
 expect(Socket.all[1]?.sent[0]).toMatchObject({principal_id:"902",faction:1,capability_token:"rank-memory-secret"});
 btn("disconnect").click();expect(Socket.all[1]?.closed).toBe(true);expect(document.getElementById("rank-status")?.textContent).toContain("未声称离队成功");expect(btn("surrender").disabled).toBe(true);expect(store).not.toHaveBeenCalled();
});
it("迟到身份回执不能在断开后恢复匹配身份",async()=>{
 let done!:(r:Response)=>void;const f=vi.fn(()=>new Promise<Response>(r=>{done=r}));await mount(f);btn("rank-auth").click();await vi.waitFor(()=>expect(f).toHaveBeenCalledTimes(1));btn("disconnect").click();done(Response.json(ctx("matched",seat)));await Promise.resolve();await Promise.resolve();expect(btn("rank-enter").disabled).toBe(true);expect(Socket.all).toHaveLength(0);
});
it("未获得控制租约不开放排位命令，错误身份/阵营回执在WS前拒绝",async()=>{
 let active={...seat,faction:8};const f=vi.fn(async()=>Response.json(ctx("matched",active)));await mount(f);btn("rank-auth").click();await vi.waitFor(()=>expect(document.getElementById("rank-status")?.textContent).toContain("不匹配"));expect(Socket.all).toHaveLength(0);
 active={...seat};input("capability").value="rank-memory-secret";btn("rank-auth").click();await vi.waitFor(()=>expect(btn("rank-enter").disabled).toBe(false));Socket.rejectLease=true;btn("rank-enter").click();await vi.waitFor(()=>expect(document.getElementById("connection")?.textContent).toContain("未确认"));expect(btn("surrender").disabled).toBe(true);expect(Socket.all[0]?.sent.some(m=>m.type==="command")).toBe(false);
});
