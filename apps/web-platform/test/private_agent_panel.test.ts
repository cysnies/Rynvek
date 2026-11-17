// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// JSDOM/传输夹具，仅验证入口与停止边界；真实SDK/HTTP/WS/Actor另见server定向集成。
// @vitest-environment jsdom
import {afterEach,expect,it,vi} from "vitest";
import {PrivateAgentHubClient,type PrivateAgentVersion} from "@rynvek/agent-sdk";
import {mountPrivateAgentPanel,type PrivateAgentContext} from "../src/private-agent-panel.js";
const meta:PrivateAgentVersion={owner_id:"42",package_id:"owner.agent",version:"1.0.0",artifact_cid:"ab".repeat(32),publisher_key:"12".repeat(32),signature:"34".repeat(64),profile:{schema:"rynvek.agent.v1",runtime:"quickjs-tools/1",role:"agent",entrypoint:"main.js",tools:["query_visible_clusters"]},dependencies:[],verification:{trust_level:3},revoked:false,visibility:"Private"};
const world="a0000000-0000-4000-8000-000000000001";
function setup(fetcher:typeof fetch){document.body.innerHTML='<section id="panel"></section>';const root=document.querySelector<HTMLElement>("section")!;const arena={identity:{principalId:"42",faction:0},welcome:{mode:"PvE_Survival"},lease:{fencing_token:7},callTool:vi.fn(async()=>({tool:"query_visible_clusters",tick:31,result:{clusters:[]}}))};const hub=new PrivateAgentHubClient("http://private.local","42",()=>"test-only-memory",fetcher);return {root,arena,hub,button:(action:string)=>root.querySelector<HTMLButtonElement>(`[data-action="${action}"]`)!};}
afterEach(()=>vi.unstubAllGlobals());
it("工作台真实调用SDK发现与受控工具，不执行上传源码；stop不会声称撤回",async()=>{
 const fetcher=vi.fn(async(url:unknown)=>Response.json(String(url).endsWith("/evaluate")?{package_id:meta.package_id,version:meta.version,artifact_cid:meta.artifact_cid,world_id:world,principal_id:"42",faction:0,tick:30,output:{call:{name:"query_visible_clusters",arguments:{include_enemy:true}},memory:{}},gas_used:1,executed:false}:{visibility:"Private",packages:[meta]}));const s=setup(fetcher);const panel=mountPrivateAgentPanel(s.root,()=>({worldId:world,principalId:"42",arena:s.arena,hub:s.hub}));panel.update();s.button("refresh").click();await vi.waitFor(()=>expect(s.root.querySelectorAll("option")).toHaveLength(1));s.button("step").click();await vi.waitFor(()=>expect(s.root.textContent).toContain("真实ToolBroker回执"));expect(s.arena.callTool).toHaveBeenCalledExactlyOnceWith("query_visible_clusters",{include_enemy:true});s.button("stop").click();expect(s.root.textContent).toContain("不能撤回");panel.reset();expect(s.button("step").disabled).toBe(true);s.hub.destroy();
});
it("身份断开后清空包清单并丢弃迟到发现，不向下一身份恢复私有信息",async()=>{
 let resolve!:(r:Response)=>void;const fetcher=vi.fn(()=>new Promise<Response>(r=>{resolve=r;}));const s=setup(fetcher);let ctx:PrivateAgentContext|null={worldId:world,principalId:"42",arena:s.arena,hub:s.hub};const panel=mountPrivateAgentPanel(s.root,()=>ctx);panel.update();s.button("refresh").click();ctx=null;panel.update();resolve(Response.json({visibility:"Private",packages:[meta]}));await new Promise(r=>setTimeout(r,10));expect(s.root.querySelectorAll("option")).toHaveLength(0);expect(s.root.textContent).not.toContain("owner.agent");expect(s.button("refresh").disabled).toBe(true);expect(s.arena.callTool).not.toHaveBeenCalled();s.hub.destroy();
});
