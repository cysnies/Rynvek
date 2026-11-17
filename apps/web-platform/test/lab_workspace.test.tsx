// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import {readFileSync} from "node:fs";
import {makeParameterExperiment} from "../src/ParameterExperiment.js";
import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {LabWorkspace} from "../src/LabWorkspace.js";
import {makeExperiment,suggestedRoles,lineageRows} from "../src/lab-model.js";
afterEach(cleanup);
const cid="a".repeat(64),genome="0".repeat(64),childGenome="1".repeat(64),q="9007199254740993";
// 此处仅是传输/组件快验夹具；生产代码没有示范种群，真实HTTP作业另由Rust定向快验执行。
const root={run:0,faction:0,entity:1,parent:null,slug:"root",unit:"unit.forager",generation:0,origin:"initial",born_tick:"0",genome,parent_genome:null,allocated_q:"0"};
const child={...root,entity:2,parent:1,slug:"child",generation:1,origin:"mitosis",born_tick:"1",genome:childGenome,parent_genome:genome,allocated_q:"4000000"};
const energy={enabled:true,run:0,tick:"10",live_bodies:2,remains:0,state_root:cid,received_q:q,stored_q:q,dissipated_q:"0",exported_q:"0",returned_q:"0",transferred_q:"0",external_received_q:q};
const job={exp_id:"private-job",name:"私有快验作业",status:"completed",generation:0,attempt:1,lease_expires_at_unix_s:null,runs_done:1,runs_total:1,variants:[],tables:{runs:cid,metrics:cid,lineage:cid,energy:cid}};
const rows=(table:string)=>table==="lineage"?[root,child]:table==="energy"?[energy]:table==="runs"?[{run:0,variant:"w3-m0",seed:"57"}]:[];
const page=(table:string)=>{const values=rows(table);return {exp_id:job.exp_id,table,cid,page:{total_rows:values.length,offset:0,next_offset:null,columns:Object.keys(values[0]??{}),rows:values}};};
function connect(){fireEvent.change(screen.getByLabelText("权限令牌"),{target:{value:"fixture-private-token"}});fireEvent.click(screen.getByText("使用此令牌"));}
it("真实React入口通过LabClient提交/读取/分析，并在身份断开时清空私有结果",async()=>{
  const fetcher=vi.fn(async(input:string|URL|Request,init?:RequestInit)=>{
    const url=String(input);expect((init?.headers as Record<string,string>)["Authorization"]).toBe("Bearer fixture-private-token");
    if(init?.method==="POST"){const sent=JSON.parse(String(init.body));expect(sent.variants[0].ecology.roles["unit.forager"].replication_mutation_rate_permille).toBe(50);return Response.json(job,{status:202});}
    const table=new URL(url).searchParams.get("table");return Response.json(table?page(table):job);
  });
  render(<LabWorkspace fetcher={fetcher as typeof fetch} initialBaseUrl="http://test"/>);
  expect(screen.queryByLabelText("真实自然出生谱系图")).toBeNull();
  connect();expect((screen.queryByLabelText("权限令牌"))).toBeNull();
  fireEvent.click(screen.getByText("提交真实实验"));await screen.findByText(/私有快验作业 · completed/);
  fireEvent.click(screen.getByText("加载真实谱系与能量分析"));await screen.findByLabelText("真实自然出生谱系图");
  expect(screen.getAllByText(new RegExp(q)).length).toBeGreaterThan(0);expect(screen.getByText(/个体账平衡/)).toBeTruthy();
  fireEvent.click(screen.getByLabelText("实体2 mitosis"));expect(screen.getByText(/自然亲代 1/)).toBeTruthy();
  fireEvent.click(screen.getByRole("tab",{name:"lineage"}));await waitFor(()=>expect(fetcher.mock.calls.some(c=>String(c[0]).includes("/rows?table=lineage"))).toBe(true));
  fireEvent.click(screen.getByText("断开并清空结果"));
  expect(screen.queryByLabelText("真实自然出生谱系图")).toBeNull();expect(screen.queryByText(new RegExp(q))).toBeNull();expect(screen.queryByText(/私有快验作业/)).toBeNull();
});
it("断开后迟到的旧身份分析响应不能重新挂回页面",async()=>{
  const pending:Array<()=>void>=[];
  const fetcher=vi.fn(async(input:string|URL|Request)=>{const url=new URL(String(input));const table=url.searchParams.get("table");
    if(table)return await new Promise<Response>(resolve=>pending.push(()=>resolve(Response.json(page(table)))));
    return Response.json(job);
  });
  render(<LabWorkspace fetcher={fetcher as typeof fetch} initialBaseUrl="http://test"/>);connect();
  fireEvent.change(screen.getByLabelText("实验ID"),{target:{value:job.exp_id}});fireEvent.click(screen.getByText("读取实验"));await screen.findByText(/私有快验作业 · completed/);
  fireEvent.click(screen.getByText("加载真实谱系与能量分析"));await waitFor(()=>expect(pending.length).toBe(4));
  fireEvent.click(screen.getByText("断开并清空结果"));for(const finish of pending)finish();
  await waitFor(()=>expect(screen.getByLabelText("权限令牌")).toBeTruthy());expect(screen.queryByLabelText("真实自然出生谱系图")).toBeNull();
});
it("W3扫描/0‰消融生成当前权威规格，真实父子校验不靠slug猜测",()=>{
  const spec=makeExperiment({name:"w3",seeds:"57,58",ticks:600,sampleEvery:30,players:1,policy:"ecology",rates:"50,100",ablation:true,roles:suggestedRoles()});
  expect(spec.variants?.map(v=>v.ecology?.roles["unit.forager"]?.replication_mutation_rate_permille)).toEqual([50,100,0]);
  expect(()=>lineageRows([child])).toThrow();expect(lineageRows([root,child])).toHaveLength(2);
});

it("作业发现、取消/重试消费真实状态；提交不确定时原键原规格对账",async()=>{
 const sent:unknown[]=[];let current={...job,status:"running"};let attempts=0;
 const fetcher=vi.fn(async(input:string|URL|Request,init?:RequestInit)=>{
  const u=new URL(String(input));if(init?.method==="POST"&&u.pathname.endsWith("/control")){const op=JSON.parse(String(init.body));sent.push(op);current={...current,status:op.action==="cancel"?"cancelled":"queued",generation:op.action==="retry"?1:0};return Response.json(current);}
  if(init?.method==="POST"){sent.push(JSON.parse(String(init.body)));if(attempts++===0)throw new Error("响应丢失");return Response.json(current,{status:202});}
  return Response.json(u.searchParams.has("limit")?{experiments:[current],offset:0,limit:25}:current);
 });
 render(<LabWorkspace fetcher={fetcher as typeof fetch} initialBaseUrl="http://test"/>);connect();
 fireEvent.click(screen.getByText("提交真实实验"));await screen.findByText("响应丢失");
 fireEvent.change(screen.getByLabelText("实验Ticks"),{target:{value:"0"}});fireEvent.click(screen.getByText("原键对账重试提交"));await screen.findByText(/私有快验作业 · running/);expect(sent[0]).toEqual(sent[1]);
 fireEvent.click(screen.getByText("取消此作业"));await screen.findByText(/私有快验作业 · cancelled/);
 fireEvent.click(screen.getByText("显式重试此作业"));await screen.findByText(/私有快验作业 · queued/);
 fireEvent.click(screen.getByText("发现我的持久作业"));await screen.findByRole("button",{name:/私有快验作业 · queued/});
 fireEvent.click(screen.getByText("断开并清空结果"));expect(screen.queryByRole("button",{name:/私有快验作业 · queued/})).toBeNull();
});

it("参数源导入、真实扫描请求、空谱系的Run发现与CAS轨迹复用现有作业入口",async()=>{
 const model=JSON.parse(readFileSync("../../examples/lab/parameter-counter.json","utf8"));
 const parameterJob={...job,name:"参数持久实验",runs_done:2,runs_total:2};let sent:any;
 const fetcher=vi.fn(async(input:string|URL|Request,init?:RequestInit)=>{
  const u=new URL(String(input));if(u.pathname.endsWith("/authoring/lab-model"))return Response.json({world_id:"private-studio",revision:"3",model_digest:cid,model});
  if(init?.method==="POST"){sent=JSON.parse(String(init.body));return Response.json(parameterJob,{status:202});}
  const t=u.searchParams.get("table");if(t){const values=t==="runs"?[{run:0,variant:"slow",seed:"57",mode:"parameters",end_reason:"tick_budget",model_digest:cid,parameter_fault:null},{run:1,variant:"fast",seed:"57",mode:"parameters",end_reason:"tick_budget",model_digest:cid,parameter_fault:null}]:t==="metrics"?[{run:0,tick:"2",faction:null,score_milli:null,parameter_entity:"counter.one",parameter_component:"state.total",parameter_type:"I32",parameter_value:2},{run:0,tick:"4",faction:null,score_milli:null,parameter_entity:"counter.one",parameter_component:"state.total",parameter_type:"I32",parameter_value:4}]:t==="energy"?[{...energy,enabled:false,live_bodies:0,received_q:"0",stored_q:"0",external_received_q:"0"}]:[];
    return Response.json({exp_id:job.exp_id,table:t,cid,page:{offset:0,total_rows:values.length,next_offset:null,columns:Object.keys(values[0]??{}),rows:values}});}
  return Response.json(parameterJob);
 });
 render(<LabWorkspace fetcher={fetcher as typeof fetch} initialBaseUrl="http://test"/>);connect();
 fireEvent.change(screen.getByLabelText("实验模型"),{target:{value:"parameters"}});
 fireEvent.change(screen.getByLabelText("参数源世界ID"),{target:{value:"private-studio"}});fireEvent.click(screen.getByText("导入有权限的Studio参数模型"));
 await screen.findByText(/已复制源 private-studio/);await waitFor(()=>expect((screen.getByText("提交真实实验") as HTMLButtonElement).disabled).toBe(false));
 fireEvent.click(screen.getByText("提交真实实验"));await screen.findByText(/参数持久实验 · completed/);
 expect(sent.variants.map((v:any)=>v.model.instances["counter.one"].parameters["param.rate"])).toEqual([1,3]);expect(sent.variants.every((v:any)=>v.mode==="parameters"&&v.players===0&&v.policy==="idle")).toBe(true);
 fireEvent.click(screen.getByText("加载真实谱系与能量分析"));await screen.findByLabelText("真实参数实体轨迹");
 expect((screen.getByLabelText("分析Run") as HTMLSelectElement).options).toHaveLength(2);
 expect(screen.queryByLabelText("真实采样存量能量曲线")).toBeNull();expect(screen.queryByLabelText("真实自然出生谱系图")).toBeNull();
 expect(screen.getByText(/没有原生身体与自然出生/)).toBeTruthy();
 fireEvent.click(screen.getByText("断开并清空结果"));expect(screen.queryByLabelText("真实参数实体轨迹")).toBeNull();expect(screen.queryByDisplayValue(/rynvek.lab-parameter-model/)).toBeNull();
});
it("参数草案不把空值、浮点或state写入当作合法扫描",()=>{
 const model=readFileSync("../../examples/lab/parameter-counter.json","utf8");
 const d={name:"scan",seeds:"57",ticks:4,every:2,model,instance:"counter.one",component:"param.rate",values:"1,3"};
 expect(makeParameterExperiment(d).variants).toHaveLength(2);
 for(const values of ["1,","1.5","9007199254740993"])expect(()=>makeParameterExperiment({...d,values})).toThrow();
 expect(()=>makeParameterExperiment({...d,component:"state.total"})).toThrow();
});
