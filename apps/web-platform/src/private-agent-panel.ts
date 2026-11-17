// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
/** 私有包工作台：不执行上传源码，不收集签名私钥，不携带凭据到下载URL或离开当前origin。 */
import { PrivateAgentRunner, type AgentArenaPort, type PrivateAgentHubClient, type PrivateAgentVersion, type SignedAgentUpload } from "@rynvek/agent-sdk";
export interface PrivateAgentContext {worldId:string;principalId:string;arena:AgentArenaPort;hub:PrivateAgentHubClient}
export function mountPrivateAgentPanel(root:HTMLElement,context:()=>PrivateAgentContext|null):{update():void;reset():void} {
 root.innerHTML=`<h2>私有 Agent 包 / 真实工具委托</h2><p class="muted">仅本人已连接的 Private 单人 PvE。包在服务器无I/O QuickJS执行，真实fog输入；每次只生成一个候选，再通过本人现有控制租约走 ToolBroker。不是独立注册AI、不是人类排位代打，未取得控制不会自动开局。Trust 3 / 64步扫描不等于社区Trust 2认证。</p>
 <div class="buttons"><button data-action="refresh">发现我的私有包</button><label>SDK生成的签名上传JSON（不含私钥）<input data-file type="file" accept="application/json,.json"></label><button data-action="publish">扫描并私有发布</button></div>
 <div class="buttons"><label>精确版本 <select data-pack aria-label="私有Agent精确版本"></select></label><button data-action="download">下载并验摘要/签名</button><button data-action="revoke">撤销此版本</button><button data-action="step" class="primary">委托执行一步真实工具</button><button data-action="stop">停止委托 / 清空记忆</button></div>
 <p class="muted">发布用 @rynvek/agent-sdk 的 signPrivateAgentPackage 外部签名回调生成文件；输入不应含令牌/私钥。禁止公共Hub发行。下载验签公钥来自认证服务元数据，非外部信任链。停止不能撤回已发送工具；未知回执不自动重试。</p><pre data-status aria-live="polite">先连接本人已有世界；本地练习/观战/回放/排位不可用。</pre>`;
 const status=root.querySelector<HTMLElement>("[data-status]")!;const select=root.querySelector<HTMLSelectElement>("[data-pack]")!;const file=root.querySelector<HTMLInputElement>("[data-file]")!;
 const buttons=Array.from(root.querySelectorAll<HTMLButtonElement>("[data-action]"));
 let current:PrivateAgentContext|null=null,rows:PrivateAgentVersion[]=[],runner:PrivateAgentRunner|null=null,busy=false,epoch=0,pending:AbortController|null=null;
 const selected=()=>rows.find(m=>`${m.package_id}@${m.version}`===select.value);
 function controls():void {const m=selected();for(const b of buttons){const a=b.dataset["action"];b.disabled=a==="stop"?!runner:!current||busy||(a==="publish"?!file.files?.length:a==="refresh"?false:!m||m.revoked||(a==="step"&&m.profile.role!=="agent"));}select.disabled=!current||busy||!rows.length;file.disabled=!current||busy;}
 function stop():void{runner?.stop();runner=null;}
 function reset():void{epoch++;pending?.abort();pending=null;stop();busy=false;current=null;rows=[];select.replaceChildren();file.value="";status.textContent="先连接本人已有世界；本地练习/观战/回放/排位不可用。";controls();}
 function update():void {const next=context();if(!next){if(current||busy||rows.length)reset();return;}if(!current||current.arena!==next.arena||current.hub!==next.hub||current.worldId!==next.worldId||current.principalId!==next.principalId){reset();current=next;status.textContent=`已连接本人 ${next.principalId} 的真实世界；点击发现我的私有包。未加载/未运行程序。`;}controls();}
 function render():void {const old=select.value;select.replaceChildren();for(const m of rows){const o=document.createElement("option");o.value=`${m.package_id}@${m.version}`;o.textContent=`${m.package_id}@${m.version} · ${m.profile.role} · ${m.revoked?"已撤销":"Private / Trust3"}`;select.append(o);}if(rows.some(m=>`${m.package_id}@${m.version}`===old))select.value=old;controls();}
 select.onchange=()=>{stop();const m=selected();status.textContent=m?`${m.package_id}@${m.version}\nBLAKE3 ${m.artifact_cid}\n仅选择，不代表已执行。`:"没有私有版本";controls();};file.onchange=controls;
 for(const b of buttons)b.onclick=()=>{const action=b.dataset["action"]!;if(action==="stop"){epoch++;pending?.abort();pending=null;stop();busy=false;status.textContent="已停止委托并清空本地记忆；已发送的工具不可撤回，未知回执不自动重试。";controls();return;}
   update();const c=current;if(!c||busy)return;const own=epoch;busy=true;const abort=new AbortController();pending=abort;controls();const timer=setTimeout(()=>abort.abort(),10000);
   void (async()=>{try{
     if(action==="refresh"){const list=await c.hub.list(abort.signal);if(own!==epoch)return;stop();rows=list;render();status.textContent=`本人私有版本 ${rows.length} 个（含撤销墓碑）；没有向公共Hub请求或发布。`;}
     else if(action==="publish"){const upload=file.files?.[0];if(!upload||upload.size>512*1024)throw new Error("签名上传文件缺失或超过512KiB");const signed=JSON.parse(await upload.text()) as SignedAgentUpload;if(own!==epoch)return;const m=await c.hub.publish(signed,abort.signal);if(own!==epoch)return;stop();rows=await c.hub.list(abort.signal);if(own!==epoch)return;render();select.value=`${m.package_id}@${m.version}`;file.value="";status.textContent=`已私有发布 ${m.package_id}@${m.version}\n${m.artifact_cid}\n64步无I/O沙箱扫描，通过不等于工具已执行。`;}
     else {const m=selected();if(!m||m.revoked)throw new Error("先选择未撤销精确版本");
       if(action==="download"){const bytes=await c.hub.download(m,abort.signal);if(own!==epoch)return;const blob=new Blob([new Uint8Array(bytes).buffer],{type:"application/octet-stream"});const url=URL.createObjectURL(blob);try{const a=document.createElement("a");a.href=url;a.download=`${m.package_id}-${m.version}.rpack`;a.click();}finally{setTimeout(()=>URL.revokeObjectURL(url),1000);}status.textContent="已从私有接口下载并验证BLAKE3/Ed25519（认证服务提供发布者公钥）；未执行源码。";}
       else if(action==="revoke"){stop();await c.hub.revoke(m,abort.signal);if(own!==epoch)return;rows=await c.hub.list(abort.signal);if(own!==epoch)return;render();status.textContent="已撤销此不可变版本；保留墓碑，依赖它的后续求值也拒绝。未删除原制品/历史。";}
       else if(action==="step"){runner??=new PrivateAgentRunner(c.hub,m,c.worldId,c.arena);const active=runner;const result=await active.step();if(own!==epoch||runner!==active)return;status.textContent=result.receipt?`真实ToolBroker回执 ${result.receipt.tool} · Tick ${result.receipt.tick}\n${result.receipt.error?`拒绝 0x${result.receipt.error.code.toString(16)}；未声称成功`:"已接受此工具"}\n${JSON.stringify(result.receipt.result??null).slice(0,2048)}`:"沙箱完成本步，call=null；未发送任何工具。";}
     }
   }catch(error){if(own===epoch){stop();status.textContent=`未确认成功：${String(error)}。无自动重试；如工具已提交则不能撤回。`;}}
   finally{clearTimeout(timer);if(own===epoch){busy=false;pending=null;controls();}}
   })();
 };
 controls();return {update,reset};
}
