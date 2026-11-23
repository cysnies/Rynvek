// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Panel, Tabs, ThemeProvider } from "@rynvek/ui";
import {ParameterExperimentEditor,ParameterTrajectory,type ParameterSpec} from "./ParameterExperiment.js";
import { LabClient, parameterMetrics, type ParameterMetric, LAB_TABLES, descendants, energyBalance, type ExperimentRecord, type LabRows, type LabTable, type LineageRow, type RoleEnergyContract } from "@rynvek/lab";
import { ROLE_KINDS, analysisTable, energyRows, lineageRows, makeExperiment, suggestedRoles, type EnergySample, type LabDraft, type JsonRow } from "./lab-model.js";
const numberFields:readonly [keyof Omit<RoleEnergyContract,"natural_reproduction">,string][]=[
  ["basal_cost_micro_per_tick","基础micro/Tick"],["reserve_ticks","预留Ticks"],["birth_energy_milli","出生milli"],
  ["movement_cost_micro_per_tile","运动micro/tile"],["synapse_maintenance_micro_per_tick","维护micro/突触/Tick"],["attack_cost_micro_per_shot","发射micro/次"],["repair_cost_micro_per_milli","修复micro/milli HP或盾"],["secretion_cost_micro_per_field_raw","分泌micro/Q8.8 raw"],["continuous_cost_micro_per_neuron_step","连续计算micro/隐藏元/子步"],["rule_cost_micro_per_gas","规则计算micro/Gas"],["material_capacity_milli","结构容量milli"],["material_growth_milli_per_tick","结构摄取milli/Tick"],["material_synthesis_micro_per_milli","结构合成micro/milli"],["birth_synthesis_milli","出生合成milli"],["activity_reserve_milli","动作留存milli"],["replication_mutation_rate_permille","本角色突变‰"]
];
export interface LabWorkspaceProps {readonly fetcher?:typeof fetch;readonly initialBaseUrl?:string}
export function LabWorkspace({fetcher,initialBaseUrl=globalThis.location?.origin??""}:LabWorkspaceProps) {
  const [base,setBase]=useState(initialBaseUrl),[token,setToken]=useState("");
  const [client,setClient]=useState<LabClient|null>(null),[server,setServer]=useState("");
  const [design,setDesign]=useState<"natural"|"parameters">("natural"),[parameterSpec,setParameterSpec]=useState<ParameterSpec>({value:null,error:"导入参数模型"});
  const [draft,setDraft]=useState<LabDraft>(()=>({name:"W3 自然复制对照",seeds:"57",ticks:600,sampleEvery:30,players:1,policy:"ecology",rates:"50",ablation:false,roles:suggestedRoles()}));
  const [job,setJob]=useState<ExperimentRecord|null>(null),[jobId,setJobId]=useState("");
  const [page,setPage]=useState<LabRows|null>(null),[table,setTable]=useState<LabTable>("runs");
  const [analysis,setAnalysis]=useState<{lineage:LineageRow[];energy:EnergySample[];runs:JsonRow[];parameters:ParameterMetric[]}|null>(null);
  const [run,setRun]=useState(0),[sample,setSample]=useState(0),[selected,setSelected]=useState<number|null>(null);
  const [following,setFollowing]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const [jobs,setJobs]=useState<ExperimentRecord[]>([]),[listOffset,setListOffset]=useState(0);
  const pending=useRef<{id:string;spec:Parameters<LabClient["submit"]>[0]}|null>(null);
  const [pendingId,setPendingId]=useState("");
  const pendingControl=useRef<{id:string;op:Parameters<LabClient["control"]>[1]}|null>(null);
  const [pendingAction,setPendingAction]=useState("");
  const forgetPending=()=>{pending.current=null;setPendingId("");pendingControl.current=null;setPendingAction("");};
  const epoch=useRef(0),operation=useRef<AbortController|null>(null);
  useEffect(()=>()=>{epoch.current++;operation.current?.abort();},[]);
  const clear=()=>{setJob(null);setJobId("");setPage(null);setAnalysis(null);setSelected(null);setRun(0);setSample(0);};
  const disconnect=()=>{epoch.current++;operation.current?.abort();setFollowing(false);setClient(null);setDesign("natural");setParameterSpec({value:null,error:"导入参数模型"});setToken("");setServer("");setBusy(false);setError("");setJobs([]);setListOffset(0);forgetPending();clear();};
  const connect=()=>{
    try {
      const url=new URL(base);if(!["http:","https:"].includes(url.protocol)||url.username||url.password||url.pathname!=="/"||url.search||url.hash)throw new Error("服务器必须是无凭据/路径/查询的HTTP(S) origin");
      if(!token.trim())throw new Error("需要自己的lab.submit/lab.read权限令牌");
      epoch.current++;operation.current?.abort();clear();setJobs([]);setListOffset(0);forgetPending();setError("");setFollowing(false);
      setClient(new LabClient({baseUrl:url.origin,token:token.trim(),...(fetcher?{fetch:fetcher}:{})}));setServer(url.origin);setToken("");
    }catch(e){setError(e instanceof Error?e.message:String(e));}
  };
  async function perform(f:(signal:AbortSignal,live:()=>boolean)=>Promise<void>) {
    operation.current?.abort();const controller=new AbortController();operation.current=controller;
    const version=++epoch.current;const live=()=>epoch.current===version&&!controller.signal.aborted;
    setBusy(true);setError("");setFollowing(false);
    try{await f(controller.signal,live);}catch(e){if(live()){setError(e instanceof Error?e.message:String(e));setPage(null);setAnalysis(null);}}
    finally{if(live())setBusy(false);}
  }
  useEffect(()=>{
    if(!client||!job||!following||!["queued","running"].includes(job.status))return;
    let stopped=false;let timer:ReturnType<typeof setTimeout>;const controller=new AbortController();const id=job.exp_id;const version=epoch.current;
    const poll=async()=>{
      try{const next=await client.getExperiment(id,controller.signal);if(stopped||epoch.current!==version)return;setJob(next);
        if(next.status==="queued"||next.status==="running")timer=setTimeout(()=>void poll(),1000);
      }catch(e){if(!stopped){setError(e instanceof Error?e.message:String(e));setFollowing(false);setPage(null);setAnalysis(null);}}
    };
    timer=setTimeout(()=>void poll(),1000);return()=>{stopped=true;clearTimeout(timer);controller.abort();};
  },[client,job?.exp_id,job?.status,following]);
  const spec=useMemo(()=>{if(design==="parameters")return parameterSpec;try{return {value:makeExperiment(draft),error:""};}catch(e){return {value:null,error:e instanceof Error?e.message:String(e)};}},[draft,design,parameterSpec]);
  const submit=()=>void perform(async(signal,live)=>{if(!client||(!pending.current&&!spec.value))return;
    if(!pending.current){pending.current={id:crypto.randomUUID(),spec:spec.value!};setPendingId(pending.current.id);}
    const request=pending.current;const result=await client.submit(request.spec,request.id,signal);
    if(live()){pending.current=null;setPendingId("");clear();setJob(result);setJobId(result.exp_id);setFollowing(true);}});
  const load=()=>void perform(async(signal,live)=>{if(!client||!jobId.trim())return;const result=await client.getExperiment(jobId.trim(),signal);if(live()){setJob(result);setPage(null);setAnalysis(null);setFollowing(true);}});
  const discover=(offset=0)=>void perform(async(signal,live)=>{if(!client)return;const result=await client.listExperiments(offset,25,signal);if(live()){setJobs(result.experiments);setListOffset(result.offset);}});
  const control=(action:"cancel"|"retry")=>void perform(async(signal,live)=>{
    if(!client||!job)return;if(!pendingControl.current){pendingControl.current={id:job.exp_id,op:{operation_id:crypto.randomUUID(),generation:job.generation,action}};setPendingAction(action);}
    const request=pendingControl.current;const result=await client.control(request.id,request.op,signal);
    if(live()){pendingControl.current=null;setPendingAction("");setJob(result);setJobId(result.exp_id);setPage(null);setAnalysis(null);setFollowing(true);}
  });
  const loadPage=(t:LabTable,offset=0)=>{setTable(t);setPage(null);void perform(async(signal,live)=>{if(!client||!job)return;const result=await client.rows(job.exp_id,t,offset,100,signal);if(live()){setTable(t);setPage(result);}});};
  const analyze=()=>void perform(async(signal,live)=>{
    if(!client||!job)return;
    const [l,e,runs,metrics]=await Promise.all([analysisTable(client,job.exp_id,"lineage",signal),analysisTable(client,job.exp_id,"energy",signal),analysisTable(client,job.exp_id,"runs",signal),analysisTable(client,job.exp_id,"metrics",signal)]);
    const next={lineage:lineageRows(l),energy:energyRows(e),runs,parameters:parameterMetrics(metrics)};
    if(live()){setAnalysis(next);setRun(Number(next.runs[0]?.["run"]??0));setSample(0);setSelected(null);}
  });
  const download=(t:LabTable)=>void perform(async(signal,live)=>{
    if(!client||!job)return;const result=await client.tryParquet(job.exp_id,t,signal);if(!live())return;
    if("pending" in result)throw new Error(result.pending);
    const url=URL.createObjectURL(new Blob([Uint8Array.from(result.bytes)],{type:"application/vnd.apache.parquet"}));
    const link=document.createElement("a");link.href=url;link.download=`${job.exp_id.replace(/[^a-zA-Z0-9_-]/g,"_")}.${t}.parquet`;link.click();setTimeout(()=>URL.revokeObjectURL(url),0);
  });
  const energy=(analysis?.energy??[]).filter(e=>e.run===run).sort((a,b)=>Number(BigInt(a.tick)-BigInt(b.tick)));
  const runSummary=analysis?.runs.find(r=>r["run"]===run),parameterRun=runSummary?.["mode"]==="parameters";
  const current=energy[Math.min(sample,Math.max(0,energy.length-1))];
  const lineage=(analysis?.lineage??[]).filter(r=>r.run===run&&(!current||BigInt(r.born_tick)<=BigInt(current.tick)));
  const shown=lineage.slice(0,200);const levels=[...new Set(shown.map(r=>r.generation))].sort((a,b)=>a-b);const slots=new Map(shown.map((r,i)=>[r.entity,{x:30+levels.indexOf(r.generation)*160,y:30+i*42}]));
  const focus=lineage.find(r=>r.entity===selected);
  const balance=current?.enabled?energyBalance(current):null;
  const descendantsCount=focus?descendants(lineage,run,focus.entity).length:0;
  const patchRole=(kind:typeof ROLE_KINDS[number],patch:Partial<RoleEnergyContract>)=>setDraft(d=>({...d,roles:{...d.roles,[kind]:{...d.roles[kind],rule:{...d.roles[kind].rule,...patch}}}}));
  return <ThemeProvider defaultTheme="neutral-dark"><div className="lab-app">
    <header className="lab-header"><div><h1>RYNVEK / LAB</h1><p>W3 / 参数模型 · 持久多种子扫描 · 真实四表</p></div><nav><a href="/arena">Arena</a> / <a href="/lab" aria-current="page">Lab</a></nav></header>
    <Panel title="私有实验连接"><div className="lab-pad lab-controls">
      <label>服务器origin<input aria-label="服务器origin" value={base} disabled={!!client} onChange={e=>setBase(e.target.value)} /></label>
      {!client?<><label>权限令牌<input aria-label="权限令牌" type="password" autoComplete="off" value={token} onChange={e=>setToken(e.target.value)} /></label><Button onClick={connect}>使用此令牌</Button></>:<><span>凭据仅在内存：{server}（权限由服务器核验）</span><Button onClick={disconnect}>断开并清空结果</Button></>}
    </div><p className="lab-hint">需要自己的lab.submit/lab.read；不会自动创建身份、把令牌放进URL/存储或公开研究结果。</p></Panel>
    <div role="alert" className="lab-error">{error}</div>
    <Panel title="持久作业发现 / 恢复"><div className="lab-pad">
      <Button disabled={!client||busy} onClick={()=>discover(0)}>发现我的持久作业</Button>
      <Button disabled={!client||busy||listOffset===0} onClick={()=>discover(Math.max(0,listOffset-25))}>上一页作业</Button>
      <Button disabled={!client||busy||jobs.length<25||listOffset+25>512} onClick={()=>discover(listOffset+25)}>下一页作业</Button>
      <ul>{jobs.map(j=><li key={j.exp_id}><Button disabled={busy} onClick={()=>{epoch.current++;operation.current?.abort();setFollowing(false);clear();setJob(j);setJobId(j.exp_id);setFollowing(true);}}>{j.name} · {j.status} · {j.runs_done}/{j.runs_total} · {j.exp_id}</Button></li>)}</ul>
      <p>断开/刷新不会取消服务端作业。重新连接后点发现，再载入状态；取消撤销租约，计算在Tick边界协作停止。恢复复用已完成run，未完成run从种子重算，不是Tick快照或PG多机灾备。</p>
      {pendingId?<p>提交结果未对账：{pendingId}，重试固定原规格；改表单不会改这个请求。先发现/对账，勿盲目新建。<Button disabled={busy} onClick={()=>{pending.current=null;setPendingId("");}}>显式放弃本地提交键（不取消服务端）</Button></p>:null}
      {job?<div><p>代次 {job.generation} · 认领 {job.attempt} · 租约到期 {job.lease_expires_at_unix_s??"无"} · 检查点 {job.runs_done}/{job.runs_total}</p>
        <Button disabled={busy||!!pendingAction||!["queued","running"].includes(job.status)} onClick={()=>control("cancel")}>取消此作业</Button>
        <Button disabled={busy||!!pendingAction||!["failed","cancelled"].includes(job.status)} onClick={()=>control("retry")}>显式重试此作业</Button>
        {pendingAction?<><p>待对账操作：{pendingControl.current?.id} / generation {pendingControl.current?.op.generation}，原请求只作用于此作业。</p><Button disabled={busy} onClick={()=>control(pendingAction as "cancel"|"retry")}>原键对账上次操作</Button><Button disabled={busy} onClick={()=>{pendingControl.current=null;setPendingAction("");}}>显式放弃本地操作键（不更改服务端）</Button></>:null}
      </div>:null}
    </div></Panel>
    <Panel title="实验设计 / W3与参数模型"><div className="lab-pad">
      <label>实验模型<select aria-label="实验模型" value={design} onChange={e=>{setParameterSpec({value:null,error:"导入参数模型"});setDesign(e.target.value as "natural"|"parameters");}}><option value="natural">自然生命W3</option><option value="parameters">Studio一般参数实体</option></select></label>
      {design==="parameters"?<ParameterExperimentEditor client={client} onChange={setParameterSpec}/>:<>

      <p>动作价格为显式实验设计值，非原规范冻结平衡值；0明确关闭对应费用。发射按次，护盾/治疗按实增milli，Spitter分泌按实际Q8.8 raw，出生合成另付且不变成启动能量。当前无一般生长/物质体量模型，非完整生态。</p>
      <div className="lab-controls"><label>实验名<input value={draft.name} onChange={e=>setDraft({...draft,name:e.target.value})}/></label>
      <label>种子（逗号）<input aria-label="实验种子" value={draft.seeds} onChange={e=>setDraft({...draft,seeds:e.target.value})}/></label>
      <label>Ticks<input aria-label="实验Ticks" type="number" min={1} value={draft.ticks} onChange={e=>setDraft({...draft,ticks:Number(e.target.value)})}/></label>
      <label>采样间隔<input type="number" min={1} value={draft.sampleEvery} onChange={e=>setDraft({...draft,sampleEvery:Number(e.target.value)})}/></label>
      <label>玩家<select value={draft.players} onChange={e=>setDraft({...draft,players:Number(e.target.value)})}><option value={1}>1</option><option value={2}>2</option></select></label>
      <label>控制器<select value={draft.policy} onChange={e=>setDraft({...draft,policy:e.target.value as LabDraft["policy"]})}><option value="ecology">合法采集/建槽，不训练</option><option value="economy">经济/防守脚本</option><option value="idle">不下命令</option></select></label></div>
      <div className="lab-scroll"><table><caption>参数为可编辑实验建议，不是规范冻结平衡；零成本明确关闭该项，未配置角色不称已预算化。</caption><thead><tr><th>配置角色</th>{numberFields.map(([k,label])=><th key={k}>{label}</th>)}<th>自然复制</th></tr></thead><tbody>{ROLE_KINDS.map(kind=><tr key={kind}><th><label><input type="checkbox" checked={draft.roles[kind].enabled} onChange={e=>setDraft(d=>({...d,roles:{...d.roles,[kind]:{...d.roles[kind],enabled:e.target.checked}}}))}/>{kind}</label></th>{numberFields.map(([key,label])=><td key={key}><input aria-label={`${kind} ${label}`} type="number" min={0} value={draft.roles[kind].rule[key]} disabled={!draft.roles[kind].enabled} onChange={e=>patchRole(kind,{[key]:Number(e.target.value)})}/></td>)}<td><input aria-label={`${kind}自然复制`} type="checkbox" checked={draft.roles[kind].rule.natural_reproduction} disabled={!draft.roles[kind].enabled} onChange={e=>patchRole(kind,{natural_reproduction:e.target.checked})}/></td></tr>)}</tbody></table></div>
      <div className="lab-controls"><label>自然复制突变率扫描‰（逗号）<input aria-label="突变率扫描" value={draft.rates} onChange={e=>setDraft({...draft,rates:e.target.value})}/></label><label><input type="checkbox" checked={draft.ablation} onChange={e=>setDraft({...draft,ablation:e.target.checked})}/>增加0‰复制突变消融</label></div>
      <p className="lab-hint">扫描覆盖启用自然复制角色的突变率；不是人工适应度。脚本控制不等于自主觅食，当前出生仍需要原RTS人口/设施。指纹含slug/代数，不是遗传距离。</p>
      </>}<Button variant="primary" disabled={!client||busy||(!pendingId&&!spec.value)} onClick={submit}>{pendingId?"原键对账重试提交":"提交真实实验"}</Button>
      {spec.error?<p className="lab-error">{spec.error}</p>:<details><summary>将提交的当前权威规格（{(spec.value?.variants?.length??1)*(spec.value?.seeds.length??0)} runs）</summary><pre>{JSON.stringify(spec.value,null,2)}</pre></details>}
    </div></Panel>
    <Panel title="私有作业 / 真实进度"><div className="lab-pad"><div className="lab-controls"><label>实验ID<input aria-label="实验ID" value={jobId} onChange={e=>setJobId(e.target.value)}/></label><Button disabled={!client||busy||!jobId.trim()} onClick={load}>读取实验</Button><Button disabled={!following} onClick={()=>setFollowing(false)}>停止本地跟踪</Button></div>
      <p className="lab-hint">停止跟踪/关闭页面不会取消服务器作业；需要ID恢复，作业规格、认领与完成run检查点在持久Store；物理持久性取决于实际后端，不把内存Store称灾备。</p>
      {job?<div aria-live="polite"><strong>{job.name} · {job.status} · {job.runs_done}/{job.runs_total} runs</strong><p className="rv-mono">{job.exp_id}</p><progress max={job.runs_total} value={job.runs_done}/>{job.error?<p className="lab-error">{job.error}</p>:null}
        <div className="lab-controls">{LAB_TABLES.map(t=><Button key={t} disabled={job.status!=="completed"||busy} onClick={()=>download(t)}>下载 {t}.parquet</Button>)}<Button disabled={job.status!=="completed"||busy} onClick={analyze}>加载真实谱系与能量分析</Button></div>
        <details><summary>服务端当前规则与四表CID</summary><pre>{JSON.stringify({variants:job.variants,tables:job.tables},null,2)}</pre></details>
      </div>:<p>尚无实验；不会展示示范曲线或伪造种群。</p>}</div></Panel>
    <Panel title="四表权威数据 / 有界分页"><div className="lab-pad">
      <fieldset disabled={job?.status!=="completed"||busy}><Tabs label="结果表" value={table} onValueChange={t=>loadPage(t as LabTable)} items={LAB_TABLES.map(t=>({id:t,label:t,content:(page?<><p className="rv-mono">CID {page.cid} · {page.page.offset}–{page.page.offset+page.page.rows.length} / {page.page.total_rows}</p><div className="lab-scroll"><table><thead><tr>{page.page.columns.map(c=><th key={c}>{c}</th>)}</tr></thead><tbody>{page.page.rows.map((r,i)=><tr key={i}>{page.page.columns.map(c=><td key={c}>{r[c]===null?"∅":String(r[c])}</td>)}</tr>)}</tbody></table></div><div className="lab-controls"><Button disabled={busy||page.page.offset===0} onClick={()=>loadPage(table,Math.max(0,page.page.offset-100))}>上一页</Button><Button disabled={busy||page.page.next_offset===null} onClick={()=>loadPage(table,page.page.next_offset??0)}>下一页</Button></div></>:<p>点击已完成实验的表读取同一CID结果。64/128位值保留十进制原文。</p>)}))}/></fieldset></div></Panel>
    {analysis?<Panel title="真实结果分析 / 参数轨迹与生命账"><div className="lab-pad"><div className="lab-controls"><label>Run<select aria-label="分析Run" value={run} onChange={e=>{setRun(Number(e.target.value));setSample(0);setSelected(null);}}>{[...new Set(analysis.runs.map(r=>Number(r["run"])))].map(v=><option key={v} value={v}>{v} / {String(analysis.runs.find(r=>r["run"]===v)?.["variant"]??"")} / seed {String(analysis.runs.find(r=>r["run"]===v)?.["seed"]??"")}</option>)}</select></label>{energy.length&&!parameterRun?<label>真实采样Tick {String(current?.tick)}<input aria-label="采样时间线" type="range" min={0} max={energy.length-1} value={Math.min(sample,energy.length-1)} onChange={e=>setSample(Number(e.target.value))}/></label>:null}</div>
      {current?<><p>活体 {current.live_bodies} / 遗留 {current.remains} · {balance?(balance.balanced?"个体账平衡":"个体账不平衡！"):"此样本未开启生态，不作为零能量验证"}</p><p className="rv-mono">{current.state_root}</p>{balance?<p>外部收入 {balance.externalReceived.toString()} q = 已核算 {balance.accounted.toString()} q；亲本内部转账已剔除。动作热耗子集（已含在总热耗中）：发射 {String(current.attack_q)} / 修复 {String(current.repair_q)} / 分泌 {String(current.secretion_q)} / 合成 {String(current.synthesis_q)} / 连续计算 {String(current.continuous_q)} / 规则 {String(current.rule_q)} q。结构材料：外源 {balance.materialExternal.toString()} u / 存储加归还 {balance.materialAccounted.toString()} u（{balance.materialBalanced?"结构账平衡":"结构账不平衡！"}）。</p>:null}{current.enabled?<EnergyChart rows={energy}/>:null}</>:null}
      <ParameterTrajectory key={run} rows={analysis.parameters.filter(r=>r.run===run)}/>
      {parameterRun?<p>参数实体profile：没有原生身体与自然出生，lineage为空，生命账未启用；不作为能量守恒证据。模型摘要 {String(runSummary?.["model_digest"])} / 结束 {String(runSummary?.["end_reason"])} / 规则故障 {String(runSummary?.["parameter_fault"]??"无")}</p>:null}
      <p className="lab-hint">这里只是个体活体/遗留累计账，不是全环境/阵营热力学证明；显式单材料增长、动作规则与连续脑更新已实付；野生供能、元素化学与三维物理等仍未闭合。图形比例只用于展示，数值校验保持BigInt。</p>
      <p>采样时已出生 {lineage.length} 节点；图中最多显示前200个真实节点，完整分析上限5000行/表，超限请导出Parquet。</p>
      {shown.length?<div className="lab-scroll"><svg role="img" aria-label="真实自然出生谱系图" width={Math.max(600,levels.length*160+60)} height={Math.max(100,shown.length*42+30)}>{shown.map(r=>{const slot=slots.get(r.entity)!;const parent=r.parent===null?undefined:slots.get(r.parent);return <g key={r.entity}>{parent?<line x1={parent.x+110} y1={parent.y} x2={slot.x} y2={slot.y} stroke="currentColor" opacity={.4}/>:null}<g role="button" tabIndex={0} aria-label={`实体${r.entity} ${r.origin}`} onClick={()=>setSelected(r.entity)} onKeyDown={e=>{if(e.key==="Enter"||e.key===" "){e.preventDefault();setSelected(r.entity);}}}><rect x={slot.x} y={slot.y-14} width={112} height={29} rx={2} fill={selected===r.entity?"var(--rv-color-accent)":"var(--rv-color-surface-raised)"} stroke="currentColor"/><text x={slot.x+5} y={slot.y+4} fill={selected===r.entity?"var(--rv-color-bg)":"currentColor"} fontSize={12}>#{r.entity} · G{r.generation} · {r.origin}</text><title>{r.slug} / {String(r.genome)}</title></g></g>;})}</svg></div>:null}
      {focus?<div><p>实体 #{focus.entity} · 自然亲代 {focus.parent??"无事实，不猜测"} · 已加载时间范围子孙 {descendantsCount} · 实付 {String(focus.allocated_q)} q</p><pre>{JSON.stringify(focus,(_k,v)=>typeof v==="bigint"?v.toString():v,2)}</pre></div>:null}
    </div></Panel>:null}
  </div></ThemeProvider>;
}
function EnergyChart({rows}:{readonly rows:readonly EnergySample[]}) {
  const max=rows.reduce((m,r)=>{const q=BigInt(r.stored_q);return q>m?q:m;},1n);
  const path=rows.map((r,i)=>`${i?"L":"M"}${20+i*560/Math.max(1,rows.length-1)},${130-Number(BigInt(r.stored_q)*110n/max)}`).join(" ");
  return <svg viewBox="0 0 600 155" role="img" aria-label="真实采样存量能量曲线"><path d={path} stroke="var(--rv-color-accent)" fill="none" strokeWidth={2}/>{rows.map((r,i)=><circle key={i} cx={20+i*560/Math.max(1,rows.length-1)} cy={130-Number(BigInt(r.stored_q)*110n/max)} r={3} fill="currentColor"><title>Tick {String(r.tick)} / stored {String(r.stored_q)} q</title></circle>)}<text x={20} y={150} fill="currentColor" fontSize={11}>真实采样点序列 · stored_q，非插值仿真</text></svg>;
}
