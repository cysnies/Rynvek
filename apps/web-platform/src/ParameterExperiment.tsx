// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {useEffect,useMemo,useRef,useState} from "react";
import {Button} from "@rynvek/ui";
import {parameterSweep,validateParameterModel,validateSpec,type ExperimentSpec,type LabClient,type ParameterMetric} from "@rynvek/lab";
export interface ParameterDraft {name:string;seeds:string;ticks:number;every:number;model:string;instance:string;component:string;values:string}
export interface ParameterSpec {value:ExperimentSpec|null;error:string}
const csv=(s:string)=>{if(!s.trim()||s.split(",").some(v=>! /^-?\d+$/.test(v.trim())))throw new Error("扫描/种子必须为逗号分隔整数");return s.split(",").map(v=>{const n=Number(v.trim());if(!Number.isSafeInteger(n))throw new Error("整数超出精确范围");return n;});};
export function makeParameterExperiment(d:ParameterDraft):ExperimentSpec {
 const model:unknown=JSON.parse(d.model);validateParameterModel(model);
 const spec:ExperimentSpec={name:d.name,seeds:csv(d.seeds),ticks:d.ticks,sample_every:d.every,
  variants:parameterSweep(model,d.instance,d.component,csv(d.values))};validateSpec(spec);return spec;
}
export function ParameterExperimentEditor({client,onChange}:{readonly client:LabClient|null;readonly onChange:(s:ParameterSpec)=>void}){
 const [d,setD]=useState<ParameterDraft>({name:"参数实体真实扫描",seeds:"57",ticks:30,every:5,model:"",instance:"",component:"param.rate",values:"1,3"});
 const [world,setWorld]=useState(""),[receipt,setReceipt]=useState(""),[error,setError]=useState(""),[busy,setBusy]=useState(false);
 const request=useRef<AbortController|null>(null);
 useEffect(()=>{setD(x=>({...x,model:"",instance:""}));setWorld("");setReceipt("");setError("");setBusy(false);return()=>request.current?.abort();},[client]);
 const result=useMemo<ParameterSpec>(()=>{try{return {value:makeParameterExperiment(d),error:""};}catch(e){return {value:null,error:d.model?(e instanceof Error?e.message:String(e)):"导入有权限的Studio参数模型或粘贴模型源"};}},[d]);
 useEffect(()=>onChange(result),[result,onChange]);
 const capture=async()=>{if(!client||!world.trim())return;request.current?.abort();const control=new AbortController();request.current=control;setBusy(true);setError("");setReceipt("");setD(x=>({...x,model:"",instance:""}));
  try{const r=await client.captureParameterModel(world.trim(),control.signal);if(control.signal.aborted)return;setD(x=>({...x,model:JSON.stringify(r.model,null,2),instance:Object.keys(r.model.instances).sort()[0]??""}));setReceipt(`${r.world_id} @ ${r.revision} / ${r.model_digest}`);}
  catch(e){if(!control.signal.aborted)setError(e instanceof Error?e.message:String(e));}finally{if(!control.signal.aborted)setBusy(false);}};
 return <div><div className="lab-controls"><label>Studio世界ID<input aria-label="参数源世界ID" value={world} onChange={e=>{request.current?.abort();setBusy(false);setWorld(e.target.value);setReceipt("");setD(x=>({...x,model:"",instance:""}));}}/></label><Button disabled={!client||busy||!world.trim()} onClick={()=>void capture()}>导入有权限的Studio参数模型</Button></div>
 <p className="lab-hint">额外需要源世界world.edit。仅复制模板/实例参数/已绑定RuleGraph，从模板初值Tick 0重算，不复制当前动态/隐藏身体/凭据；撤权阻止再次导出，不声称能追回已授权源副本。默认采样排序前16个state输出，可在JSON明确修改。</p>
 {receipt?<p className="rv-mono">已复制源 {receipt}</p>:null}{error?<p role="alert" className="lab-error">{error}</p>:null}
 <label>不可变参数模型JSON<textarea aria-label="参数模型JSON" rows={12} value={d.model} maxLength={262144} onChange={e=>{request.current?.abort();setBusy(false);setReceipt("");setD({...d,model:e.target.value});}}/></label>
 <div className="lab-controls"><label>参数实验名<input value={d.name} onChange={e=>setD({...d,name:e.target.value})}/></label><label>参数种子<input aria-label="参数实验种子" value={d.seeds} onChange={e=>setD({...d,seeds:e.target.value})}/></label><label>参数Ticks<input aria-label="参数实验Ticks" type="number" min={1} value={d.ticks} onChange={e=>setD({...d,ticks:Number(e.target.value)})}/></label><label>参数采样间隔<input aria-label="参数采样间隔" type="number" min={1} value={d.every} onChange={e=>setD({...d,every:Number(e.target.value)})}/></label>
 <label>扫描实例<input aria-label="扫描实例" value={d.instance} onChange={e=>setD({...d,instance:e.target.value})}/></label><label>扫描param组件<input aria-label="扫描param组件" value={d.component} onChange={e=>setD({...d,component:e.target.value})}/></label><label>扫描原始整数<input aria-label="扫描原始整数" value={d.values} onChange={e=>setD({...d,values:e.target.value})}/></label></div>
 <p className="lab-hint">复用Engine Phase07与持久Lab，不执行任意W3/TS。I32整数；Q8_8为原始定点位；Bool只接受0/1。没有自然谱系，生命账enabled=false，RTS指标为null；不造出生/能量。</p></div>;
}
export function ParameterTrajectory({rows}:{readonly rows:readonly ParameterMetric[]}) {
 const [selected,setSelected]=useState("");const id=(r:ParameterMetric)=>JSON.stringify([r.instance_id,r.component]);const keys=[...new Set(rows.map(id))];const active=keys.includes(selected)?selected:keys[0];
 const points=rows.filter(r=>id(r)===active).sort((a,b)=>a.tick<b.tick?-1:a.tick>b.tick?1:0);if(!points.length)return null;
 const lo=Math.min(...points.map(p=>p.raw)),hi=Math.max(...points.map(p=>p.raw)),start=points[0]!.tick,span=points.at(-1)!.tick-start||1n;
 const path=points.map((p,i)=>`${i?"L":"M"}${20+Number((p.tick-start)*560n/span)},${130-(p.raw-lo)*110/(hi-lo||1)}`).join(" ");
 return <section><h3>真实参数轨迹（原CAS metrics）</h3><label>采样组件<select aria-label="轨迹组件" value={active} onChange={e=>setSelected(e.target.value)}>{keys.map(k=><option key={k} value={k}>{k}</option>)}</select></label><svg viewBox="0 0 600 155" role="img" aria-label="真实参数实体轨迹"><path d={path} stroke="var(--rv-color-accent)" fill="none" strokeWidth={2}/><text x={20} y={150} fill="currentColor" fontSize={11}>Tick {start.toString()}–{points.at(-1)!.tick.toString()} / {points[0]!.ty} raw {lo}–{hi} / {points.length}真实采样点</text></svg><p>折线只连接真实采样，不插值补造指标；最后原值 {points.at(-1)!.raw}，Q8_8保持原始位。</p></section>;
}
