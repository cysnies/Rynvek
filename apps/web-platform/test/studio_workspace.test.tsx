// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
// 仅传输夹具；生产Studio不生成世界/权威回执。
import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor,act} from "@testing-library/react";
import {StudioApp} from "../src/StudioApp.js";
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
const basis={world_id:"w",revision:7,tick:4,state_root:"ab".repeat(32),rule_fault:null,ecology_contract:null,parameter_state:{next_serial:0,entities:{},fault:null},collaboration:{principal_id:"1",role:"owner",grant_id:null},baseline:{width:2,height:2,terrain:[0,0,0,0],params:{},genomes:{},rules:{},templates:[]}};
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
it("一般参数模板真实编辑入口提交原Epoch补丁且只显示服务读取的动态态，断开清理",async()=>{
 const sent:any[]=[];let current=basis;vi.stubGlobal("fetch",vi.fn(async(_url:unknown,init?:RequestInit)=>{
 if(init?.method==="POST"){const patch=JSON.parse(String(init.body));sent.push(patch);return Response.json({status:"applied",world_id:"w",world_revision:11,effective_tick:8,state_root:"cd".repeat(32)});}
 return Response.json(current);
 }));await connect();fireEvent.click(screen.getByText("RuleGraph动作规则"));expect(screen.getByLabelText("一般参数实体registry")).toBeTruthy();fireEvent.click(screen.getByText("创建或编辑registry项"));await screen.findByText(/revision 11/);expect(sent[0].operations[0].SetWorldParam.key_path).toBe("rules.templates.model.counter");expect(sent[0].safety_class).toBe("EpochBoundarySafe");expect(screen.getByLabelText("权威参数实体状态").textContent).not.toContain('"state.total"');
 current={...basis,revision:11,tick:9,state_root:"cd".repeat(32),parameter_state:{next_serial:1,entities:{"counter.one":{serial:1,values:{"state.total":1337}}},fault:null}} as typeof basis;
 fireEvent.click(screen.getByText("读取权威参数实体状态"));await waitFor(()=>expect(screen.getByLabelText("权威参数实体状态").textContent).toContain("1337"));fireEvent.change(screen.getByLabelText("registry JSON"),{target:{value:'{"private-user-value":3}'}});fireEvent.click(screen.getByText("断开身份"));expect((screen.getByLabelText("registry JSON") as HTMLTextAreaElement).value).not.toContain("private-user-value");expect(screen.getByLabelText("权威参数实体状态").textContent).not.toContain("1337");
});
it('所有者明确源码授权与原版本撤销走真实管理API，不自动邀请',async()=>{
 const calls:any[]=[];let grants:any[]=[];
 vi.stubGlobal('fetch',vi.fn(async(url:unknown,init?:RequestInit)=>{calls.push([String(url),init]);if(String(url).endsWith('/editors')){if(init?.method==='POST'){const b=JSON.parse(String(init.body));expect(b.consent).toBe('studio-source-edit/1');grants=[{principal_id:b.principal_id,grant_id:b.active?'g1':'g2',active:b.active,role:'studio-editor',consent:b.consent}];return Response.json(grants[0])}return Response.json({grants})}return Response.json(basis)}));
 await connect();expect(calls).toHaveLength(1);fireEvent.change(screen.getByLabelText('Studio编辑者主体ID'),{target:{value:'18446744073709551600'}});expect((screen.getByText('授予或重授Studio编辑') as HTMLButtonElement).disabled).toBe(true);fireEvent.click(screen.getByLabelText('确认Studio源码编辑授权'));fireEvent.click(screen.getByText('授予或重授Studio编辑'));await screen.findByText(/18446744073709551600 · 有效/);fireEvent.click(screen.getByText('撤销编辑 18446744073709551600'));await screen.findByText(/18446744073709551600 · 已撤销/);const body=JSON.parse(String(calls.filter(c=>c[1]?.method==='POST')[1][1].body));expect(body.expected_grant_id).toBe('g1');
});
it('编辑者同步撤权后清私有视图/身份，不显示管理权或保留假可写状态',async()=>{
 vi.stubGlobal('fetch',vi.fn(async(url:unknown)=>String(url).includes('/changes?')?Response.json({message:'准入已撤销'},{status:404}):Response.json({...basis,collaboration:{principal_id:'2',role:'studio-editor',grant_id:'g1'}})));
 await connect();expect(screen.queryByText('刷新真实创作准入')).toBeNull();fireEvent.click(screen.getByText('同步原权威日志'));await screen.findByRole('alert');expect(screen.getByText('尚无权威基线')).toBeTruthy();expect(screen.queryByText('同步原权威日志')).toBeNull();expect((screen.getByLabelText('Studio令牌') as HTMLInputElement).value).toBe('');
});
