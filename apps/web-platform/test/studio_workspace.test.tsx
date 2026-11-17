// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
// 仅传输夹具；生产Studio不生成世界/权威回执。
import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor,act} from "@testing-library/react";
import {StudioApp} from "../src/StudioApp.js";
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
const basis={world_id:"w",revision:7,tick:4,state_root:"ab".repeat(32),rule_fault:null,baseline:{width:2,height:2,terrain:[0,0,0,0],params:{},genomes:{},rules:{},templates:[]}};
async function connect(){render(<StudioApp/>);fireEvent.change(screen.getByLabelText("Studio服务地址"),{target:{value:"http://srv"}});fireEvent.change(screen.getByLabelText("世界ID"),{target:{value:"w"}});fireEvent.change(screen.getByLabelText("Studio令牌"),{target:{value:"private"}});fireEvent.click(screen.getByText("连接权威世界"));await screen.findByText(/revision 7/)}
it("真实HTTP会话挂载停靠编辑，回执前不展示乐观地形/修订，断开清除私有结果",async()=>{
 let resolve!:(r:Response)=>void;const fetcher=vi.fn(async(_url:unknown,init?:RequestInit)=>init?.method==="POST"?new Promise<Response>(r=>{resolve=r}):Response.json(basis));vi.stubGlobal("fetch",fetcher);
 await connect();expect(screen.getByLabelText("在Arena运行当前世界").getAttribute("href")).toBe("/arena?world=w&api_origin=http%3A%2F%2Fsrv");fireEvent.change(screen.getByLabelText("tile_kind"),{target:{value:"4"}});fireEvent.click(screen.getByText("提交地形"));
 await waitFor(()=>expect(fetcher).toHaveBeenCalledTimes(2));expect(screen.getByText(/revision 7/)).toBeTruthy();expect(document.querySelector('svg[aria-label="权威地形"] rect')).toBeNull();
 await act(async()=>resolve(Response.json({status:"applied",world_id:"w",world_revision:8,effective_tick:5,state_root:"cd".repeat(32),patch_cid:"ef".repeat(32),report:{tiles_modified:1}})));
 await screen.findByText(/revision 8/);expect(document.querySelector('svg[aria-label="权威地形"] rect')).toBeTruthy();
 fireEvent.click(screen.getByText("断开身份"));expect(screen.getByText("尚无权威基线")).toBeTruthy();expect(screen.queryByLabelText("权威提交回执")).toBeNull();expect((screen.getByLabelText("Studio令牌") as HTMLInputElement).value).toBe("");
});
it("409后不遗留可提交的假世界",async()=>{
 vi.stubGlobal("fetch",vi.fn(async(_url:unknown,init?:RequestInit)=>init?.method==="POST"?Response.json({message:"修订冲突"},{status:409}):Response.json(basis)));
 await connect();fireEvent.click(screen.getByText("提交地形"));await screen.findByRole("alert");expect(screen.getByText("尚无权威基线")).toBeTruthy();expect(document.querySelector('svg[aria-label="权威地形"]')).toBeNull();
});
it("真实创建请求使用Private和显式实验生态，随后读取新世界权威基线",async()=>{
 const fetcher=vi.fn(async(_url:unknown,init?:RequestInit)=>init?.method==="POST"?Response.json({world_id:"new-world"},{status:201}):Response.json({...basis,world_id:"new-world"}));vi.stubGlobal("fetch",fetcher);
 render(<StudioApp/>);fireEvent.change(screen.getByLabelText("Studio服务地址"),{target:{value:"http://srv"}});fireEvent.change(screen.getByLabelText("Studio令牌"),{target:{value:"owner"}});fireEvent.click(screen.getByText("创建私有世界并连接"));await screen.findByText(/世界 new-world/);
 expect(fetcher.mock.calls[0]?.[0]).toBe("http://srv/api/v1/worlds");const body=JSON.parse(String(fetcher.mock.calls[0]?.[1]?.body));expect(body.visibility).toBe("Private");expect(body.ecology.roles["unit.forager"].replication_mutation_rate_permille).toBe(50);expect(fetcher.mock.calls[1]?.[0]).toBe("http://srv/api/v1/worlds/new-world/authoring");
});

it("首次创建和删除均有权威撤销重做入口，删除回执前保留已提交模块",async()=>{
 const sent:any[]=[];let pending:((r:Response)=>void)|undefined;let deferDelete=false;
 vi.stubGlobal("fetch",vi.fn(async(url:unknown,init?:RequestInit)=>{
  if(String(url).endsWith("compile-rule"))return Response.json({compiled_rir_blake3:new Array(32).fill(26)});
  if(init?.method!=="POST")return Response.json(basis);
  const p=JSON.parse(String(init.body));sent.push(p);
  if(deferDelete)return new Promise<Response>(r=>{pending=r});
  return Response.json({status:"applied",world_id:"w",world_revision:p.base_authoring_revision+4,effective_tick:sent.length*4,state_root:"cd".repeat(32)});
 }));
 await connect();fireEvent.click(screen.getByText("RuleGraph动作规则"));
 const remove=screen.getByText("删除选中规则") as HTMLButtonElement;expect(remove.disabled).toBe(true);
 fireEvent.click(screen.getByText("编译并提交规则"));await screen.findByText(/revision 11/);expect(screen.getByRole("button",{name:"control"})).toBeTruthy();
 fireEvent.click(screen.getByText("权威撤销"));await screen.findByText(/revision 15/);expect(screen.queryByRole("button",{name:"control"})).toBeNull();
 fireEvent.click(screen.getByText("权威重做"));await screen.findByText(/revision 19/);expect(screen.getByRole("button",{name:"control"})).toBeTruthy();
 deferDelete=true;fireEvent.click(screen.getByText("删除选中规则"));await waitFor(()=>expect(sent.length).toBe(4));expect(screen.getByRole("button",{name:"control"})).toBeTruthy();expect(screen.getByText(/revision 19/)).toBeTruthy();
 await act(async()=>pending!(Response.json({status:"applied",world_id:"w",world_revision:23,effective_tick:16,state_root:"ef".repeat(32)})));
 await screen.findByText(/revision 23/);expect(screen.queryByRole("button",{name:"control"})).toBeNull();
 deferDelete=false;fireEvent.click(screen.getByText("权威撤销"));await screen.findByText(/revision 27/);expect(screen.getByRole("button",{name:"control"})).toBeTruthy();
 expect(sent.map(p=>Object.keys(p.operations[0])[0])).toEqual(["UpdateRuleModule","RemoveRuleModule","UpdateRuleModule","RemoveRuleModule","UpdateRuleModule"]);
 expect(sent[4].operations[0]).toEqual(sent[0].operations[0]);expect(sent.every(p=>p.safety_class==="EpochBoundarySafe")).toBe(true);
});
