// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {useEffect,useRef,useState} from "react";
import {GENERAL_RUN_CONSENT,type GeneralRun,type GeneralRunOperation,type GeneralRunSpec} from "@rynvek/agent-sdk";
import type {GeneralWorldSession} from "./general-remote.js";
function key():string{const b=crypto.getRandomValues(new Uint8Array(16));let t=BigInt(Date.now());for(let i=5;i>=0;i--){b[i]=Number(t&255n);t>>=8n;}b[6]=(b[6]!&15)|0x70;b[8]=(b[8]!&63)|0x80;const s=Array.from(b,x=>x.toString(16).padStart(2,"0")).join("");return `${s.slice(0,8)}-${s.slice(8,12)}-${s.slice(12,16)}-${s.slice(16,20)}-${s.slice(20)}`;}
function integer(s:string):number{if(!/^(0|[1-9][0-9]*)$/.test(s)||!Number.isSafeInteger(Number(s)))throw new Error("须规范安全整数");return Number(s);}
/** 仅明确HTTP意图/只读查询，不设推进或自动重试计时器。 */
export function GeneralRunPanel({session,busy,perform}:{session:GeneralWorldSession;busy:boolean;perform:(work:()=>Promise<string>)=>Promise<void>}){
 const [ticks,setTicks]=useState("16"),[wall,setWall]=useState("120"),[interval,setInterval]=useState("500"),[energy,setEnergy]=useState("1000000"),[consent,setConsent]=useState(false);
 const [submission,setSubmission]=useState(""),[runId,setRunId]=useState(""),[operation,setOperation]=useState(""),[run,setRun]=useState<GeneralRun>();
 const [lastStart,setLastStart]=useState<GeneralRunSpec>(),[pending,setPending]=useState<{id:string;op:GeneralRunOperation}>();
 const epoch=useRef(0),mounted=useRef(true);useEffect(()=>{mounted.current=true;const off=session.onPrivateInvalidated(()=>{epoch.current++;setRun(undefined);setLastStart(undefined);});const clear=()=>{session.invalidatePrivate();};const hidden=()=>{if(document.visibilityState!=="visible")clear();};window.addEventListener("offline",clear);document.addEventListener("visibilitychange",hidden);return()=>{mounted.current=false;epoch.current++;off();window.removeEventListener("offline",clear);document.removeEventListener("visibilitychange",hidden);};},[session]);
 async function apply(work:()=>Promise<GeneralRun>,clears=false):Promise<string>{const e=epoch.current;const v=await work();if(!mounted.current||!session.durableReceiptCurrent(v)||!clears&&e!==epoch.current)throw new Error("私有运行记录已失效");setRun(v);setRunId(v.id);setSubmission(v.spec.submission_id);if(pending?.id===v.id){const receipt=v.operations[pending.op.operation_id];if(receipt&&receipt.action===pending.op.action&&receipt.generation===pending.op.generation){setPending(undefined);setOperation("");}}return `服务器运行状态 ${v.status}；已确认${v.done}/${v.spec.ticks}，只计规则paid_q ${v.energy_used_q}`;}
 async function change(action:GeneralRunOperation["action"]):Promise<string>{if(!run||run.id!==runId||!operation||pending)throw new Error("须读取运行并明确新操作键；未确认操作先对账");const op={action,generation:run.generation,operation_id:operation};setPending({id:run.id,op});const v=await session.durableControl(run.id,op);if(!mounted.current||!session.durableReceiptCurrent(v))throw new Error("控制回执已失效");setRun(v);setPending(undefined);setOperation("");return `服务器已确认${action}，状态${v.status}；已提交Tick不回滚`;}
 return <section aria-label="服务器耐久运行"><h2>服务器耐久规则运行</h2><p>真正由服务器调度，关闭页面仍可继续。Memory仅进程内；PG还需私有文件CAS。预算只累计规则paid_q，不包含自然代谢等世界总能耗。</p>
 <label>服务器Tick上限<input aria-label="耐久Tick上限" value={ticks} onChange={e=>setTicks(e.target.value)}/></label>
 <label>总墙钟秒数（含暂停/故障等待）<input aria-label="耐久墙钟秒数" value={wall} onChange={e=>setWall(e.target.value)}/></label>
 <label>间隔毫秒<input aria-label="耐久间隔毫秒" value={interval} onChange={e=>setInterval(e.target.value)}/></label>
 <label>规则paid_q上限<input aria-label="耐久规则费上限" value={energy} onChange={e=>setEnergy(e.target.value)}/></label>
 <label>原提交UUIDv7<input aria-label="耐久提交键" value={submission} onChange={e=>{setSubmission(e.target.value);setRun(undefined);}}/></label>
 <button disabled={busy} onClick={()=>{setSubmission(key());setLastStart(undefined);setRun(undefined);setConsent(false);}}>明确准备新提交键</button>
 <label><input type="checkbox" checked={consent} onChange={e=>setConsent(e.target.checked)}/>明确同意上述规则预算与服务器后台执行</label>
 <button disabled={busy||!consent||!submission||session.controlFence===undefined||!session.observation} onClick={()=>void perform(async()=>{const o=session.observation;if(!o)throw new Error("须精确观察");const spec:GeneralRunSpec={submission_id:submission,expected_revision:o.commit.revision,expected_root:o.commit.state_root,ticks:integer(ticks),wall_seconds:integer(wall),interval_ms:integer(interval),max_energy_q:energy,consent:GENERAL_RUN_CONSENT};setLastStart(spec);return apply(()=>session.startDurable(spec),true);})}>明确启动服务器耐久运行</button>
 <button disabled={busy||!submission} onClick={()=>void perform(()=>apply(()=>session.durableStatus(submission,true)))}>按原提交键只读对账</button>
 <label>已有运行UUIDv7<input aria-label="耐久运行ID" value={runId} onChange={e=>{setRunId(e.target.value);setRun(undefined);}}/></label>
 <button disabled={busy||!runId} onClick={()=>void perform(()=>apply(()=>session.durableStatus(runId)))}>只读刷新运行状态</button>
 {run&&<p role="status">运行 {run.id} · {run.status} · {run.done}/{run.spec.ticks} · 规则paid_q {run.energy_used_q}/{run.spec.max_energy_q} · {run.storage_backend==="memory"?"Memory仅本进程，非跨进程耐久":"PG元数据/私有CAS链"} · 控制代次 {run.generation} · 截止Unix秒 {run.deadline_s}；刷新不延长预算。{run.error}</p>}
 <label>控制操作UUIDv7<input aria-label="耐久控制键" value={operation} onChange={e=>setOperation(e.target.value)}/></label><button disabled={busy} onClick={()=>{setOperation(key());setPending(undefined);}}>明确准备新控制键</button>
 {(["pause","resume","cancel"] as const).map(a=><button key={a} disabled={busy||!run||run.id!==runId||!operation||!!pending} onClick={()=>void perform(()=>change(a))}>{a==="pause"?"服务器暂停":a==="resume"?"明确恢复原预算":"服务器取消意图"}</button>)}
 {lastStart&&<details><summary>本次明确启动规格（失回执请保留原键对账）</summary><pre>{JSON.stringify(lastStart,null,2)}</pre></details>}
 {pending&&<p>控制尚未确认，先只读刷新并核原收据；准备新控制键只放弃本地待确认记录，不撤销旧请求。原操作：<code>{JSON.stringify(pending)}</code></p>}
 <p>断开/隐藏只清本地私有显示，不暂停或取消服务器。请求未确认请保留原提交键只读对账，不自动重发。恢复不延原墙钟/费用，不跳Epoch；暂停保留世界，维护换装前先取消意图。</p></section>;
}
