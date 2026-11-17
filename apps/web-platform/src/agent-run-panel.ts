// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
/** 原Arena私有包面板的真实持久执行入口；默认同源QuickJS，不配置外部LLM。 */
import {ControlledAgentAdapter,PrivatePackageProvider,PrivateHubHttpError,type AgentRunRecord,type AgentRunClient,type PrivateAgentVersion,type RunControl} from "@rynvek/agent-sdk";
import type {PrivateAgentContext} from "./private-agent-panel.js";
export function mountAgentRunPanel(root:HTMLElement,context:()=>PrivateAgentContext|null,selected:()=>PrivateAgentVersion|undefined):{update():void;reset():void}{
 root.innerHTML=`<h3>独立持久运行 / Pi参考受控循环</h3><p>Pi仅借鉴循环/状态/事件/中断设计，未引入其代码。当前provider是同源私有QuickJS包，不是LLM。无第三方模型、密钥、费用或私有数据外发。工具仍由原ToolBroker裁决；同一世界只保留一个未关闭运行。</p>
 <label>轮次上限<input data-turns type="number" min="1" max="32" value="4"></label><label>工具次数上限<input data-calls type="number" min="1" max="32" value="4"></label><label>墙钟秒预算<input data-seconds type="number" min="10" max="900" value="120"></label>
 <label><input data-consent type="checkbox">我明确授权当前选中的私有包及其声明工具，仅控制此本人私有PvE，以上预算内运行</label><p data-grant></p>
 <div class="buttons"><button data-run="create">创建明确授权的持久运行</button><button data-run="list">发现我的持久运行</button><select data-run-select aria-label="持久Agent运行"></select><button data-run="load">读取真实状态/事件</button></div>
 <div class="buttons"><button data-run="step">执行一轮（同源私有包）</button><button data-run="loop">启动预算内循环（非LLM）</button><button data-run="pause">暂停后续轮次</button><button data-run="cancel">取消服务端运行</button><button data-run="recover">回收过期运行租约</button><button data-run="resume">本人确认当前宿主/控制fence并续跑</button><button data-run="ack_unknown">已对账未知调用且不重发原调用</button><button data-run="finish">关闭此运行</button><button data-run="local-stop">停止本地循环（不撤回派发）</button></div>
 <p>刷新/断开不删除运行。再次连接后发现、读取；过期租约显式回收，unknown先查真实状态/WAL再确认，不自动重试。取消不能撤销已发送工具。恢复保留原事件、记忆和已消耗预算；不是独立注册Agent、无人值守后台模型或exactly-once灾备。</p><pre data-run-status aria-live="polite"></pre>`;
 let current:PrivateAgentContext|null=null,api:AgentRunClient|null=null,rows:AgentRunRecord[]=[],adapter:ControlledAgentAdapter|null=null,busy=false,epoch=0;
 let pendingCreate:Parameters<AgentRunClient["create"]>[0]|null=null;let pendingControl:{id:string;body:RunControl}|null=null;
 const status=root.querySelector<HTMLElement>("[data-run-status]")!,grant=root.querySelector<HTMLElement>("[data-grant]")!,select=root.querySelector<HTMLSelectElement>("[data-run-select]")!,consent=root.querySelector<HTMLInputElement>("[data-consent]")!;
 const buttons=Array.from(root.querySelectorAll<HTMLButtonElement>("[data-run]"));
 const input=(key:string)=>Number(root.querySelector<HTMLInputElement>(`[data-${key}]`)!.value);
 function controls(){const m=selected();for(const button of buttons){const action=button.dataset["run"]!;button.disabled=action==="local-stop"?!adapter:!current||(busy&&!["pause","cancel"].includes(action))||(action==="create"?(!m||m.revoked||!consent.checked||current.arena.lease===null):!["list"].includes(action)&&!select.value);}
  grant.textContent=current&&m?`将授权 ${m.package_id}@${m.version} / ${m.artifact_cid}；工具：${m.profile.tools.join(", ")}。选择不等于授权。`:"先选择未撤销的私有Agent版本。";select.disabled=busy||!current;
 }
 function render(value:AgentRunRecord){const index=rows.findIndex(r=>r.run_id===value.run_id);if(index<0)rows.unshift(value);else rows[index]=value;const chosen=value.run_id;select.replaceChildren();for(const row of rows){const o=document.createElement("option");o.value=row.run_id;o.textContent=`${row.run_id} · ${row.status} · ${row.turns_used}/${row.budget.turns}`;select.append(o);}select.value=chosen;status.textContent=JSON.stringify({run_id:value.run_id,status:value.status,budget:value.budget,deadline_unix_s:value.deadline_unix_s,turns:value.turns_used,calls:value.calls_used,lease:value.lease,last_result:value.last_result,pending:value.pending,events:value.events},null,2);controls();}
 function reset(){epoch++;adapter?.stopLocal();adapter=null;current=null;api=null;rows=[];busy=false;pendingCreate=null;pendingControl=null;select.replaceChildren();consent.checked=false;status.textContent="未创建/未认领运行；仅本人现有私有PvE可用。";controls();}
 function update(){const c=context();if(!c){if(current)reset();return;}if(current?.arena!==c.arena||current?.hub!==c.hub||current?.worldId!==c.worldId){reset();current=c;api=c.hub.runs();}controls();}
 consent.onchange=controls;select.onchange=()=>{adapter?.stopLocal();adapter=null;controls();};
 for(const button of buttons)button.onclick=()=>{const action=button.dataset["run"]!;if(action==="local-stop"){epoch++;adapter?.stopLocal();adapter=null;busy=false;status.textContent="已停止本地循环。已派发工具不可撤回；请读取服务端状态/对账，不自动重发。";controls();return;}
  if(busy&&["pause","cancel"].includes(action)){epoch++;adapter?.stopLocal();adapter=null;busy=false;}
  update();const c=current,client=api;if(!c||!client||busy)return;const own=epoch;busy=true;controls();
  void(async()=>{try{
   if(action==="create"){const m=selected(),fence=c.arena.lease?.fencing_token;if(!m||!consent.checked||fence===undefined)throw new Error("先选择私有Agent、显式授权并取得本人控制租约");pendingCreate??={submission_id:crypto.randomUUID(),world_id:c.worldId,package_id:m.package_id,version:m.version,artifact_cid:m.artifact_cid,control_fence:String(fence),tools:[...m.profile.tools],budget:{turns:input("turns"),tool_calls:input("calls"),wall_seconds:input("seconds")},consent:"private-pve-tools/1"};const value=await client.create(pendingCreate);if(own!==epoch)return;pendingCreate=null;consent.checked=false;render(value);}
   else if(action==="list"){const list=(await client.list()).filter(r=>r.world_id===c.worldId);if(own!==epoch)return;rows=list;select.replaceChildren();if(list[0])render(list[0]);else{status.textContent="此世界没有本人持久运行。";}}
   else if(action==="load"){const value=await client.get(select.value);if(own!==epoch)return;render(value);}
   else if(action==="step"||action==="loop"){const value=await client.get(select.value);const m=selected();if(!m||value.artifact_cid!==m.artifact_cid)throw new Error("先选择此运行精确绑定的包版本");adapter??=new ControlledAgentAdapter(client,value.run_id,new PrivatePackageProvider(c.hub,m));const result=await(action==="loop"?adapter.run():adapter.step());if(own!==epoch)return;render(result);}
   else {adapter?.stopLocal();adapter=null;const value=await client.get(select.value);let binding:{}={};if(action==="resume"){const context=await client.context(value.run_id);const fence=c.arena.lease?.fencing_token;if(fence===undefined)throw new Error("先显式取得本人当前控制租约");binding={host_fence:context.current_host_fence,control_fence:String(fence)};}
    if(pendingControl&&(pendingControl.id!==value.run_id||pendingControl.body.action!==action))throw new Error("先对账上次原键控制操作，再发其它动作");
    pendingControl??={id:value.run_id,body:{operation_id:crypto.randomUUID(),expected_revision:value.revision,action:action as RunControl["action"],...binding}};
    const result=await client.control(pendingControl.id,pendingControl.body);if(own!==epoch)return;pendingControl=null;render(result);
   }
  }catch(error){if(error instanceof PrivateHubHttpError&&[400,404,409].includes(error.status)){pendingControl=null;if(error.status===400)pendingCreate=null;}if(own===epoch)status.textContent=`未确认完成：${String(error)}。创建/控制重试保留原键；已派发候选不自动重发，请读取真实状态对账。`;}
  finally{if(own===epoch){busy=false;controls();}}})();
 };controls();return{update,reset};
}
