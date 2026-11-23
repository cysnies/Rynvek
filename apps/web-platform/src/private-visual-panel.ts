// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import type {PrivateVisualClient,PrivateVisualVersion,SignedVisualUpload} from "@rynvek/agent-sdk";
import type {LoadedPrivateVisual} from "@rynvek/agent-sdk";
export interface VisualTarget {apply(loaded:LoadedPrivateVisual,label:string):Promise<void>;reset():void}
export interface PrivateVisualContext {worldId:string;principalId:string;hub:PrivateVisualClient}
/** Explicit private-only publishing/download/application. No automatic fetch or application. */
export function mountPrivateVisualPanel(root:HTMLElement,context:()=>PrivateVisualContext|null,visual:VisualTarget){
 root.innerHTML=`<h2>私有受限视觉主题 / RVIEW</h2><p>签名原RYVW与精确依赖 · 仅世界背景/七阵营不透明色，原FrameScene/renderer真实消费。无shader/脚本/纹理，不改变fog、几何或应用UI。必须显式应用，不是最终工业美术。</p><div data-visual="identity"></div><div class="buttons"><button data-visual="discover">发现我的视觉包</button><select aria-label="私有视觉包" data-visual="versions"></select><button data-visual="apply">校验下载并应用</button><button data-visual="download">下载签名原制品</button><button data-visual="revoke">撤销该私有版本</button><button data-visual="builtin">恢复内置视觉</button></div><label>SDK生成的已签名视觉上传JSON（不含私钥）<input type="file" accept="application/json,.json" data-visual="upload"></label><button data-visual="publish">显式发布到本人私有库</button><pre data-visual="status" role="status">先连接实际席位；不自动发现或应用。</pre>`;
 const el=(s:string)=>root.querySelector<HTMLElement>(`[data-visual="${s}"]`)!;const select=el("versions") as HTMLSelectElement;const file=el("upload") as HTMLInputElement;
 let identity="",epoch=0,busy=false,rows:PrivateVisualVersion[]=[],pending:AbortController|null=null;
 function current(){const c=context();return c&&`${c.worldId}/${c.principalId}`===identity?c:null;}
 function reset(){visual.reset();epoch++;pending?.abort();pending=null;busy=false;rows=[];select.replaceChildren();file.value="";el("status").textContent="身份/世界已清理；不保留私有视觉结果。";}
 function update(){const c=context(),key=c?`${c.worldId}/${c.principalId}`:"";if(key!==identity){reset();identity=key;}el("identity").textContent=c?`当前本人席位 ${c.worldId}`:"未连接真实席位";for(const b of root.querySelectorAll<HTMLButtonElement>("button"))b.disabled=!c||busy;select.disabled=!c||busy;file.disabled=!c||busy;}
 function selected(){const m=rows[Number(select.value)];if(!m)throw new Error("先发现并选择一个精确版本");return m;}
 async function run(work:(c:PrivateVisualContext,s:AbortSignal,valid:()=>boolean)=>Promise<void>){update();const c=current();if(!c||busy)return;busy=true;const own=epoch;pending=new AbortController();update();const valid=()=>epoch===own&&current()?.worldId===c.worldId;try{await work(c,pending.signal,valid);}catch{if(valid())el("status").textContent="请求/校验未通过；未自动重试、未应用新的视觉。";}finally{if(valid()){busy=false;pending=null;update();}}}
 function show(m:PrivateVisualVersion){el("status").textContent=JSON.stringify({package_id:m.package_id,version:m.version,artifact_cid:m.artifact_cid,profile:m.profile,dependencies:m.dependencies,verification:m.verification,revoked:m.revoked},null,2);}
 (el("discover") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const found=await c.hub.list(s);if(!valid())return;rows=found;select.replaceChildren(...rows.map((r,i)=>{const o=document.createElement("option");o.value=String(i);o.textContent=`${r.package_id}@${r.version} · ${r.profile.role}${r.revoked?" / 已撤销":""}`;return o;}));if(rows[0])show(rows[0]);else el("status").textContent="本人私有库没有视觉包。使用SDK签名上传，不自动发布示例。";});
 select.onchange=()=>{const m=rows[Number(select.value)];if(m)show(m);};
 (el("apply") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const m=selected();const bank=await c.hub.load(m,s);if(!valid())return;await visual.apply(bank,`${m.package_id}@${m.version} / ${m.artifact_cid.slice(0,12)}`);if(!valid())return;show(m);el("status").textContent+="\n原renderer已确认视觉参数；后续帧使用。签名许可证/署名："+JSON.stringify(bank.packages.map(p=>({id:p.metadata.package_id,credit:p.metadata.profile.credit})));});
 (el("download") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const m=selected(),bytes=await c.hub.download(m,s);if(!valid())return;const url=URL.createObjectURL(new Blob([bytes.buffer as ArrayBuffer],{type:"application/octet-stream"}));const a=document.createElement("a");a.href=url;a.download=`${m.package_id}-${m.version}.rview`;a.click();setTimeout(()=>URL.revokeObjectURL(url),0);show(m);});
 (el("revoke") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const m=selected();await c.hub.revoke(m,s);if(!valid())return;visual.reset();rows=rows.map(r=>r===m?{...m,revoked:true}:r);el("status").textContent="版本已撤销；当前视觉已恢复内置。已下载字节无法远程收回，不声称DRM。";});
 (el("builtin") as HTMLButtonElement).onclick=()=>{visual.reset();el("status").textContent="恢复内置视觉；不修改仿真。";};
 (el("publish") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const f=file.files?.[0];if(!f||f.size>96*1024)throw new Error("须SDK已签名JSON，至多96KiB");const upload=JSON.parse(await f.text()) as SignedVisualUpload;if(!valid())return;const m=await c.hub.publish(upload,s);if(!valid())return;show(m);file.value="";});
 update();return {update,reset:()=>{reset();identity="";update();}};
}
