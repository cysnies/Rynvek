// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import type {PrivateAudioClient,PrivateAudioVersion,SignedAudioUpload} from "@rynvek/agent-sdk";
import type {ArenaAudioSession} from "./arena-audio.js";
export interface PrivateAudioContext {worldId:string;principalId:string;hub:PrivateAudioClient}
/** Explicit private-only publishing/download/application. No automatic fetch or sound unlock. */
export function mountPrivateAudioPanel(root:HTMLElement,context:()=>PrivateAudioContext|null,audio:ArenaAudioSession){
 root.innerHTML=`<h2>私有程序化音效制品 / SOUND BANK</h2><p>签名RYPK · 精确依赖 · 真实24配方参数与混音；无脚本/远端音源/媒体解码。仅本人私有发现，应用后仍需点击上方手势启用，不补播。</p><div data-audio="identity"></div><div class="buttons"><button data-audio="discover">发现我的音效包</button><select aria-label="私有音效包" data-audio="versions"></select><button data-audio="apply">校验下载并应用</button><button data-audio="download">下载签名原制品</button><button data-audio="revoke">撤销该私有版本</button><button data-audio="builtin">恢复内置配方</button></div><label>SDK生成的已签名音效上传JSON（不含私钥）<input type="file" accept="application/json,.json" data-audio="upload"></label><button data-audio="publish">显式发布到本人私有库</button><pre data-audio="status" role="status">先连接实际席位；不自动发现或播放。</pre>`;
 const el=(s:string)=>root.querySelector<HTMLElement>(`[data-audio="${s}"]`)!;const select=el("versions") as HTMLSelectElement;const file=el("upload") as HTMLInputElement;
 let identity="",epoch=0,busy=false,rows:PrivateAudioVersion[]=[],pending:AbortController|null=null;
 function current(){const c=context();return c&&`${c.worldId}/${c.principalId}`===identity?c:null;}
 function reset(){epoch++;pending?.abort();pending=null;busy=false;rows=[];select.replaceChildren();file.value="";el("status").textContent="身份/世界已清理；不保留私有音效结果。";}
 function update(){const c=context(),key=c?`${c.worldId}/${c.principalId}`:"";if(key!==identity){reset();identity=key;}el("identity").textContent=c?`当前本人席位 ${c.worldId}`:"未连接真实席位";for(const b of root.querySelectorAll<HTMLButtonElement>("button"))b.disabled=!c||busy;select.disabled=!c||busy;file.disabled=!c||busy;}
 function selected(){const m=rows[Number(select.value)];if(!m)throw new Error("先发现并选择一个精确版本");return m;}
 async function run(work:(c:PrivateAudioContext,s:AbortSignal,valid:()=>boolean)=>Promise<void>){update();const c=current();if(!c||busy)return;busy=true;const own=epoch;pending=new AbortController();update();const valid=()=>epoch===own&&current()?.worldId===c.worldId;try{await work(c,pending.signal,valid);}catch{if(valid())el("status").textContent="请求/校验未通过；未自动重试、未启用音频。";}finally{if(valid()){busy=false;pending=null;update();}}}
 function show(m:PrivateAudioVersion){el("status").textContent=JSON.stringify({package_id:m.package_id,version:m.version,artifact_cid:m.artifact_cid,profile:m.profile,dependencies:m.dependencies,verification:m.verification,revoked:m.revoked},null,2);}
 (el("discover") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const found=await c.hub.list(s);if(!valid())return;rows=found;select.replaceChildren(...rows.map((r,i)=>{const o=document.createElement("option");o.value=String(i);o.textContent=`${r.package_id}@${r.version} · ${r.profile.role}${r.revoked?" / 已撤销":""}`;return o;}));if(rows[0])show(rows[0]);else el("status").textContent="本人私有库没有音效包。使用SDK签名上传，不自动发布示例。";});
 select.onchange=()=>{const m=rows[Number(select.value)];if(m)show(m);};
 (el("apply") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const m=selected();const bank=await c.hub.load(m,s);if(!valid())return;audio.setSoundBank(bank.manifest,bank.recipes,`${m.package_id}@${m.version} / ${m.artifact_cid.slice(0,12)}`);show(m);el("status").textContent+="\n已应用真实配方数据；仍关闭声音，需上方手势启用。";});
 (el("download") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const m=selected(),bytes=await c.hub.download(m,s);if(!valid())return;const url=URL.createObjectURL(new Blob([bytes.buffer as ArrayBuffer],{type:"application/octet-stream"}));const a=document.createElement("a");a.href=url;a.download=`${m.package_id}-${m.version}.rpack`;a.click();setTimeout(()=>URL.revokeObjectURL(url),0);show(m);});
 (el("revoke") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const m=selected();await c.hub.revoke(m,s);if(!valid())return;audio.clearSoundBank();rows=rows.map(r=>r===m?{...m,revoked:true}:r);el("status").textContent="版本已撤销；当前音图已切断。已下载字节无法远程收回，不声称DRM。";});
 (el("builtin") as HTMLButtonElement).onclick=()=>{audio.clearSoundBank();el("status").textContent="恢复内置配方，需另行手势启用。";};
 (el("publish") as HTMLButtonElement).onclick=()=>void run(async(c,s,valid)=>{const f=file.files?.[0];if(!f||f.size>128*1024)throw new Error("须SDK已签名JSON，至多128KiB");const upload=JSON.parse(await f.text()) as SignedAudioUpload;if(!valid())return;const m=await c.hub.publish(upload,s);if(!valid())return;show(m);file.value="";});
 update();return {update,reset:()=>{reset();identity="";update();}};
}
