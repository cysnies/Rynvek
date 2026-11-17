// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/** 真实平台 /arena 的显式双显示入口（临时 Canvas2D / 真实 WebGPU Worker）；仿真只在服务器的 ArenaRuntime/Actor。
 * 可显式选择真实 Dedicated Worker + Rust/wgpu/WebGPU；两条路径都不是浏览器性能验收。
 */
import { createArenaApp, type ArenaApp } from "@rynvek/arena";
import { WebSocketTransport, buildingSpec, rawToFloat, type BuildingKind, type ArenaServerMessage } from "@rynvek/client";
import { buildCommandPanelView, mountCommandPanel } from "./arena-command-panel.js";
import { fogAt, project, unproject, viewScale, type Viewport } from "./arena-view.js";
import { mountArenaRenderer } from "./arena-renderer.js";
import { ArenaWorldClient, ReplayObservationTransport, ObserverTransport, type ObserverFrame, type ObserverGrant, type WorldRun, type PlaybackFrame, type QueueContext, type CustomRoomView } from "./arena-remote.js";
import { mountPrivateAgentPanel } from "./private-agent-panel.js";
import "./browser.css";

const gpuRequested = new URLSearchParams(location.search).get("renderer") === "webgpu";

const mount=document.querySelector<HTMLDivElement>("#app")!;
mount.innerHTML=`<header><h1>RYNVEK <span style="color:var(--rv-accent)">ARENA</span></h1><span id="connection" class="chip">等待开始</span><span id="arena-mode" class="mode muted">已有世界 PvE / 正式人类排位</span><span id="renderer" class="chip">${gpuRequested?"WebGPU Worker 待连接":"临时 Canvas2D"}</span><a class="chip" href="/studio">Studio 创作</a><a class="chip" href="/lab">Lab 实验台</a><button id="start" class="primary">开始本地练习</button></header>
<section class="panel arena-connect"><h2>运行 Studio 已有世界 / 原 Actor</h2><div class="buttons"><label>服务 origin <input id="api-origin" aria-label="Arena服务地址" autocomplete="off"></label><label>世界ID <input id="world-id" aria-label="Arena世界ID" autocomplete="off"></label><label>WorldEdit / MatchPlay 凭据 <input id="capability" aria-label="Arena令牌" type="password" autocomplete="off"></label><button id="connect-world" class="primary">运行/重连此世界</button><button id="disconnect">断开身份</button></div><p class="muted">只运行此ID的真实单人PvE世界，不另建演示。凭据只在内存与握手帧，绝不放URL/存储；请核对服务地址。刷新后重新输入同一身份重连。关闭页面不是暂停，终局世界不重置。</p></section>
<section class="panel arena-connect"><h2>私有自创双人PvP / 非排位</h2><details><summary>显式创建新的Private双人世界（已有世界不要重复创建）</summary><label>标题 <input id="custom-title" value="私有自创对局" aria-label="自创世界标题"></label><label>地图seed <input id="custom-seed" type="number" value="42" aria-label="自创地图seed"></label><button id="custom-create">创建私有双人世界</button></details><div class="buttons"><button id="custom-load">验证身份 / 读取邀请与席位</button><label>受邀主体ID <input id="custom-guest" aria-label="受邀主体ID" autocomplete="off"></label><button id="custom-invite" disabled>定向邀请该正式主体</button><button id="custom-accept" disabled>确认接受我的控制席位</button><button id="custom-connect" disabled>冻结完整席位 / 连接重连</button><button id="custom-close" disabled>撤销或离开并关闭自创局</button></div><p id="custom-status" role="status">使用上方服务origin、世界ID和自己的正式凭据；世界ID不是授权秘密。不自动创建玩家或发送邀请消息。</p><p class="muted">所有者为阵营0，指定玩家为阵营1。先由受邀者确认，再由所有者显式冻结，双方原WS连接后开局；各自只见原fog投影。无源码/私有CID共享、不计排位。撤销/主动离开会回收两席连接与租约并中止尚未结束的整局，不热换玩家、不伪造胜负；关闭后需显式fork/新建，不抹旧世界历史。断开/离页只断连接，不释放席位或暂停；同一身份可恢复原match。只读观战请用下方独立准入。</p></section>
<section class="panel arena-connect"><h2>私有世界 · 定向阵营观战</h2><div class="buttons"><button id="observe-world">只读观战此世界 / 重新确认</button><button id="leave-observer" disabled>离开并撤销我的观战准入</button></div><p id="observer-status" role="status">观察者需正式WorldConnect身份，以及所有者针对该主体的准入；不是公开房间，也没有控制租约。</p><details><summary>所有者管理观战准入（先运行/重连自己的世界）</summary><label>观察者主体ID <input id="observer-principal" aria-label="观察者主体ID" autocomplete="off"></label><button id="grant-observer" disabled>授予当前所控阵营观战</button><button id="refresh-observers" disabled>刷新观战准入</button><div id="observer-grants"></div></details><p class="muted">只共享所有者当前所控阵营的原fog投影，不看未授权阵营，不授予源码、完整归档或CID下载。准入清单不是在线人数。观察者不计入准备/入座，不启动或恢复Actor。服务每次读取重新鉴权，UI以250ms间隔拉取原Actor事实；撤销/离开后不再读取。断开/离页仅停止显示，不等于撤销准入；重新连接须重新输入凭据。当前不提供事件历史或全知观战。</p></section>
<section class="panel arena-connect"><h2>真实人类排位 / HUMAN RANKED</h2><div class="buttons"><button id="rank-auth">验证排位身份 / 找回席位</button><button id="rank-join" disabled>进入人类排位队列</button><button id="rank-refresh" disabled>刷新队列/席位</button><button id="rank-cancel" disabled>退出排队</button><button id="rank-enter" disabled>连接已匹配席位 / 重连</button></div><p id="rank-status" role="status">未验证；使用上方服务 origin 与正式凭据，不创建临时玩家。</p><p class="muted">显式入队才匹配真实已注册对手；无人则等待，不生成机器人。双方连接后原宿主自动开局，无伪准备按钮。排位按原服务规则公开对局元数据/签名归档，不把自创世界送入排位。断开身份/离页只停止本地追踪，不保证离队或暂停：请先按「退出排队」并等回执；已匹配须重连后完成对局/投降。队列与完整配对席位持久保存；失效宿主可从已提交CAS/WAL恢复。健康远端宿主仅显示origin，请核对可信地址后手动切换并重新认证，不自动跨origin转发凭据。内存Store只跨同Store实例保留，不等于进程重启持久化或PG多机灾备。</p></section>
<main class="layout"><section class="world"><div class="worldbar"><span>WORLD / <span id="clock">TICK —</span></span><span id="pointer">只读阵营视图</span></div><canvas id="world" tabindex="0" aria-label="Rynvek 战争迷雾地图，左键选择、右键命令"></canvas><div class="metrics"><span class="metric">生物质<b id="biomass">—</b></span><span class="metric">通量<b id="flux">—</b></span><span class="metric">神经胶<b id="neurogel">—</b></span><span class="metric">人口<b id="supply">—</b></span><span class="metric">波次<b id="wave">—</b></span></div></section>
<aside class="sidebar"><section class="panel"><h2>指挥 / CONTROL</h2><div class="buttons"><button id="select" disabled>全选己方单位</button><button id="home" disabled>主脑居中</button><button id="release-control" disabled>交还控制租约</button><button id="claim-control" disabled>重新申请控制</button><button id="harvest" disabled>采集 → 点目标</button><button id="move" disabled>移动 → 点目标</button><button disabled title="当前权威协议不提供暂停">暂停未开放</button></div><p id="selection" class="muted">尚未选择</p><p id="intent">开始后，左键点选或拖框；右键移动。黄绿色为营养沼泽，工蜂右键自动采集。</p><p id="outcome" class="outcome"></p></section>
<section class="panel"><h2>建造 / BUILD</h2><div id="build" class="build"></div><p class="muted">${gpuRequested?"GPU 黄色轮廓仅表示落点意图，不表示可建；权限、占地、资源和科技由服务器最终验证。":"选择建筑后点地图；黄色框仅为落点预览，权限、占地、资源和科技由服务器最终验证。"}Esc / 右键取消。</p></section>
<section class="panel" id="command-panel" aria-label="生产科技信标操作面板"></section>
<section class="panel"><h2>签名归档逐记录重演 / REPLAY</h2><p>真实已提交创作锚点 + 实际运行WAL + TickRecord重演后的阵营观测。服务端验签；只读，无控制租约。完整终根未到达前不声称全片验证。</p><div class="buttons"><button id="watch-replay" disabled>打开真实回放</button><button id="play-replay" disabled>播放</button><button id="pause-replay" disabled>暂停/停止定位</button><button id="close-replay" disabled>关闭回放</button></div><label>目标Tick<input id="replay-tick" aria-label="回放目标Tick" type="number" min="0" value="0" disabled></label><button id="seek-replay" disabled>定位（有界重演）</button><p id="replay-status" role="status">未装载归档</p><p class="muted">倒退从真实初态重新演算，每请求最多120条记录，可停止；不是插值/伪帧。首帧是实际已提交Studio状态，不伪造编辑过程帧；完整保留运行期及恢复前的真实WAL。</p></section>
<section class="panel"><h2>权威回执 / EVENTS</h2><div id="log" class="log" role="status" aria-live="polite">尚未连接。没有本地随机单位或预演资源。</div><div class="buttons" style="margin-top:12px"><button id="surrender" disabled>结束练习（投降）</button><button id="replay" disabled>下载回放</button></div></section>
<section class="panel"><h2>操作说明</h2><p>① 开始 → 全选己方单位<br>② 右键附近空地，观察真实位置变化<br>③ 点「采集」再点黄绿地块，观察生物质账本<br>④ 选择「萃取器」放在黄绿地块，或信标放在主脑附近空地；观察资源扣除与施工进度<br>⑤ 在主脑上重复建造会收到实际错误码。</p><p class="muted">滚轮缩放，方向键/WASD 平移，Home 回主脑。黑色未探索、暗色已探索；敌人只来自服务器迷雾裁剪观察。</p></section></aside></main>
<footer>${gpuRequested?"当前显示：真实 Rust/wgpu + Dedicated Engine Worker / OffscreenCanvas；GPU不可用会显式报错，不静默切换。地图、己方选择与落点/拖框轮廓走GPU；预览不是合法性预测，命令以服务器回执为准。":"当前显示：临时 Canvas2D / 工业像素占位，不是 WebGPU。"} <a id="switch-renderer" href="/arena">${gpuRequested?"手动重载为临时 Canvas2D":"显式启用 WebGPU Worker"}</a>；生成WASM与软件GPU通过不等于浏览器验收。<br>已有世界通过正式身份/席位连接；仅「开始本地练习」使用显式回环开发身份。令牌不持久化。服务器持续推进，关闭页面不等于暂停。神经生态完整闭环、正式美术和浏览器性能未验收。<a href="/api/v1/meta/source-offer" target="_blank" rel="noopener">源码要约</a></footer>`;
const el=(id:string):HTMLElement=>document.getElementById(id)!;
const button=(id:string):HTMLButtonElement=>el(id) as HTMLButtonElement;
let canvas=el("world") as HTMLCanvasElement;
const ctx=gpuRequested?null:canvas.getContext("2d");
let gpuRenderer:ReturnType<typeof mountArenaRenderer>|null=null;
let arena:ArenaApp|null=null;
let socket:WebSocket|null=null;
type Session=WorldRun & {client:ArenaWorldClient;practice:boolean};
let session:Session|null=null;
let customClient:ArenaWorldClient|null=null,customRoom:CustomRoomView|null=null,customBusy=false,customPending:AbortController|null=null,customEpoch=0;
let customTimer:ReturnType<typeof setTimeout>|null=null,customRead:AbortController|null=null;
let generation=0,request:AbortController|null=null,disposed=false,animationFrame=0;
interface Playing {cursor:string;frame:PlaybackFrame;source:ReplayObservationTransport;session:Session;own:number;busy:boolean;playing:boolean;stop:boolean;timer:ReturnType<typeof setTimeout>|null;request:AbortController|null}
let playback:Playing|null=null;
interface Watching {client:ArenaWorldClient;frame:ObserverFrame;source:ObserverTransport;timer:ReturnType<typeof setTimeout>|null;pending:AbortController|null;own:number;leaving:boolean}
let watching:Watching|null=null,grantBusy=false,grantRequest:AbortController|null=null;
let observerGrants:ObserverGrant[]=[];
let rankClient:ArenaWorldClient|null=null,rankContext:QueueContext|null=null;
let rankEpoch=0,rankBusy=false,rankRequest:AbortController|null=null,rankTimer:ReturnType<typeof setTimeout>|null=null;

const input=(id:string)=>el(id) as HTMLInputElement;
const query=new URLSearchParams(location.search);
input("world-id").value=query.get("world")??"";
input("api-origin").value=query.get("api_origin")??location.origin;
const alternative=new URL("/arena",location.origin);if(query.get("world"))alternative.searchParams.set("world",query.get("world")!);if(query.get("api_origin"))alternative.searchParams.set("api_origin",query.get("api_origin")!);if(!gpuRequested)alternative.searchParams.set("renderer","webgpu");(el("switch-renderer") as HTMLAnchorElement).href=alternative.toString();
let connected=false,linked=false,initialized=false,starting=false;
let lastNotice:object|null=null;
let armed:"Move"|"Harvest"|null=null;
let pointer:[number,number]=[0,0];
let drag:{x:number;y:number;world:[number,number]}|null=null;
let logLines:string[]=[];
const agentRoot=document.createElement("section");agentRoot.className="panel";el("capability").closest("section")!.after(agentRoot);
const privateAgents=mountPrivateAgentPanel(agentRoot,()=>session&&!session.practice&&!session.custom&&session.mode==="PvE_Survival"&&connected&&!starting&&!playback&&!watching&&arena&&!arena.client.observation?.outcome?{worldId:session.world_id,principalId:session.principal_id,arena:arena.client,hub:session.client.privateAgentHub(session.principal_id)}:null);
const labels:Partial<Record<BuildingKind,string>>={"bld.nexus_core":"主脑","bld.biomass_harvester":"萃取器","bld.signal_beacon":"信标","bld.spawning_vat":"孵化槽","bld.pulse_turret":"防御塔","bld.flux_reactor":"反应堆","bld.synapse_refinery":"精炼厂","bld.mutation_crucible":"坩埚"};
const kinds:BuildingKind[]=["bld.biomass_harvester","bld.signal_beacon","bld.spawning_vat","bld.flux_reactor","bld.pulse_turret","bld.synapse_refinery","bld.mutation_crucible"];
for(const kind of kinds){
  const b=document.createElement("button"),spec=buildingSpec(kind);
  b.textContent=`${labels[kind]} · 生物质 ${spec.cost.biomass/1000} / 胶 ${spec.cost.neurogel/1000}`;
  b.disabled=true;b.dataset["game"]="1";
  b.onclick=()=>{arena?.controller.chooseBuilding(kind);armed=null;el("intent").textContent=`建造 ${labels[kind]}：点击落点（黄色为预览，不代表通过）`;};
  el("build").append(b);
}
function canControl():boolean{return connected&&(!session?.custom||(!customBusy&&session.match_state==="Running"));}
function note(value:string):void{logLines=[...logLines,value].slice(-8);el("log").textContent=logLines.join("\n");}
const commandPanel=mountCommandPanel(el("command-panel"),{
  send:async(cmd)=>{if(!arena||!canControl()||playback)throw new Error("当前不可写入命令");const ack=await arena.controller.send(cmd);if(ack)note(`真实权威回执 ${ack.type}`);refreshPanel();return ack;},
  onSelect:(id)=>{arena?.controller.selectOwnedBuilding(id);display();},
  onStatus:(text)=>note(text),
});
function refreshPanel():void{
  const obs=arena?.client.observation??null;
  commandPanel.update(buildCommandPanelView(obs,arena?.ui.selectedBuilding??null,Boolean(canControl()&&!playback&&obs&&!obs.outcome)));
}

function controls():void{
  privateAgents.update();
  const active=canControl() && !arena?.client.observation?.outcome;
  button("observe-world").disabled=starting||connected||!!playback||!!watching||!!rankClient||!!customClient;
  button("leave-observer").disabled=!watching||starting;
  button("grant-observer").disabled=grantBusy||starting||!session||session.practice||!!playback||(!!session.custom&&customRoom?.role!=="owner");
  button("refresh-observers").disabled=grantBusy||starting||!session||session.practice||!!playback||(!!session.custom&&customRoom?.role!=="owner");
  for(const b of el("observer-grants").querySelectorAll<HTMLButtonElement>("button"))b.disabled=grantBusy||!session;
  for(const id of ["select","home","harvest","move","surrender"]) button(id).disabled=!active;
  for(const b of document.querySelectorAll<HTMLButtonElement>("[data-game]"))b.disabled=!active;
  button("release-control").disabled=!active||!arena?.client.lease;
  button("claim-control").disabled=connected||!linked||starting||!!playback||!!arena?.client.observation?.outcome;
  const ended=!!arena?.client.observation?.outcome||session?.match_state==="Completed";
  button("replay").disabled=!ended||!session||(!!session.custom&&customRoom?.role!=="owner");
  button("watch-replay").disabled=!ended||!session||starting||!!playback||(!!session.custom&&customRoom?.role!=="owner");
  button("start").disabled=!!customClient || !!watching || starting || (connected && !ended) || !!rankClient || rankBusy;
  button("connect-world").disabled=!!customClient || !!watching || starting || (connected && !ended) || !!rankClient || rankBusy;
  rankControls();customControls();
  refreshPanel();
  button("play-replay").disabled=!playback||playback.busy||playback.playing;
  button("pause-replay").disabled=!playback||(!playback.busy&&!playback.playing);
  button("seek-replay").disabled=!playback||playback.busy||playback.playing;
  input("replay-tick").disabled=!playback||playback.busy||playback.playing;
  button("close-replay").disabled=!playback;
}
function view():Viewport{return {...(arena?.ui.camera ?? {x:32,y:32,zoom:1}),width:canvas.clientWidth,height:canvas.clientHeight,yDown:gpuRequested,...(gpuRequested?{pixelRatio:devicePixelRatio||1}:{})};}
function worldPoint(e:PointerEvent|MouseEvent):[number,number]{const r=canvas.getBoundingClientRect();return unproject(view(),e.clientX-r.left,e.clientY-r.top);}
function display():void{
  const obs=arena?.client.observation;if(!obs||!arena)return;
  for(const key of ["biomass","flux","neurogel"] as const)el(key).textContent=(obs.ledger.balance[key]/1000).toFixed(1);
  el("supply").textContent=obs.supply.join(" / ");el("wave").textContent=String(obs.pve_wave ?? "—");
  el("clock").textContent=`TICK ${obs.tick}`;
  const selected=obs.units.filter(u=>arena!.ui.selection.has(u.id));
  el("selection").textContent=`已选 ${selected.length} 单位${selected.length?` · ${selected.map(u=>`#${u.id}`).join(", ")}`:""}${arena.ui.selectedBuilding!==null?` / 建筑 #${arena.ui.selectedBuilding}`:""}`;
  el("outcome").textContent=obs.outcome?`终局：${JSON.stringify(obs.outcome)}`:"";
  if(obs.outcome&&!playback&&!watching){el("connection").textContent="已终局 / 可打开签名回放";if(session)session.match_state="Completed";}
  if(arena.ui.notices.length){const n=arena.ui.notices.at(-1)!;const text=`拒绝 ${n.code===null?"":`0x${n.code.toString(16)}`} ${n.text}`;if(lastNotice!==n){lastNotice=n;note(text);}}
  controls();
}
async function deadline<T>(pending:Promise<T>,label:string):Promise<T>{
  let timer:ReturnType<typeof setTimeout>|undefined;
  try{return await Promise.race([pending,new Promise<never>((_resolve,reject)=>{timer=setTimeout(()=>reject(new Error(`${label}超时，未确认任何成功`)),8000);})]);}
  finally{clearTimeout(timer);}
}
function clearRuntime():void{
  privateAgents.reset();
  request?.abort();request=null;
  if(customTimer)clearTimeout(customTimer);customTimer=null;customRead?.abort();customRead=null;
  grantRequest?.abort();grantRequest=null;grantBusy=false;observerGrants=[];el("observer-grants").replaceChildren();
  const observer=watching;watching=null;
  if(observer){observer.pending?.abort();if(observer.timer)clearTimeout(observer.timer);observer.source.close();observer.client.destroy();}
  el("observer-status").textContent="未连接观战；断开不代表撤销准入";
  const old=playback;playback=null;
  if(old){old.stop=true;old.playing=false;old.request?.abort();if(old.timer)clearTimeout(old.timer);void old.session.client.closePlayback(old.session.match_id,old.cursor).catch(()=>{});}
  gpuRenderer?.dispose();gpuRenderer=null;arena?.dispose();arena=null;socket?.close();socket=null;
  connected=false;linked=false;initialized=false;armed=null;drag=null;lastNotice=null;
  for(const id of ["biomass","flux","neurogel","supply","wave"])el(id).textContent="—";
  el("clock").textContent="TICK —";el("outcome").textContent="";el("selection").textContent="尚未选择";
  el("replay-status").textContent="未装载归档";
}
function disconnect():void{
  resetCustom();resetRank();generation++;clearRuntime();session?.client.destroy();session=null;
  input("capability").value="";starting=false;logLines=[];el("log").textContent="身份已断开；未暂停服务器，也未把未确认命令算成功。";
  el("connection").textContent="身份已断开";controls();
}
function mountGpu(current:ArenaApp,info:Pick<WorldRun,"world_id"|"faction">):void{
  if(!gpuRequested)return;
  gpuRenderer?.dispose();gpuRenderer=null;
  const next=canvas.cloneNode(false) as HTMLCanvasElement;canvas.replaceWith(next);canvas=next;drag=null;bindCanvasInput();
  try{gpuRenderer=mountArenaRenderer(canvas,current.client,{worldId:info.world_id,faction:info.faction,
    view:view(),dpr:devicePixelRatio||1,
    createWorker:()=>new Worker(new URL("./engine.worker.ts",import.meta.url),{type:"module",name:"rynvek-arena-engine"}),
    onStatus:status=>{el("renderer").textContent=status;},
    onError:error=>note(`显示失败：${error.message}；服务器未暂停，可手动切换Canvas2D。`)});
  }catch(error){el("renderer").textContent="WebGPU挂载失败";note(String(error));}
}
async function connectLive(info:Session,own:number,signal:AbortSignal):Promise<void>{
  const ws=new WebSocket(info.client.streamURL(info.world_id));socket=ws;
  await new Promise<void>((resolve,reject)=>{
    const finish=(error?:Error)=>{clearTimeout(t);signal.removeEventListener("abort",abort);ws.onopen=null;ws.onerror=null;if(error){ws.close();reject(error);}else resolve();};
    const abort=()=>finish(new Error("连接已取消"));const t=setTimeout(()=>finish(new Error("连接超时")),8000);
    ws.onopen=()=>finish();ws.onerror=()=>finish(new Error("连接失败，请检查Rust服务"));
    signal.addEventListener("abort",abort,{once:true});if(signal.aborted)abort();
  });
  if(own!==generation||signal.aborted){ws.close();return;}
  const transport=new WebSocketTransport({send:raw=>ws.send(JSON.stringify(info.client.hello(JSON.parse(raw) as Parameters<ArenaWorldClient["hello"]>[0]))),
    close:()=>ws.close(),addEventListener:(_type,listener)=>ws.addEventListener("message",listener),removeEventListener:(_type,listener)=>ws.removeEventListener("message",listener)});
  const current=createArenaApp(transport,{principalId:info.principal_id,kind:"Human",faction:info.faction});arena=current;current.ui.camera.yDown=gpuRequested;
  ws.addEventListener("message",e=>{if(own!==generation||arena!==current||typeof e.data!=="string")return;const m=JSON.parse(e.data) as ArenaServerMessage;if(m.type==="ack"&&m.ack)note(`已提交Tick ${m.tick} · ${m.ack.type}`);});
  current.client.on(event=>{if(own!==generation||arena!==current)return;if(event.type==="observation"){
    if(!initialized){current.controller.jumpToNexus();current.ui.camera.zoom=1.8;initialized=true;}display();
  }if(event.type==="error")note(event.error.message);if(event.type==="lease_revoked"||(event.type==="lease"&&!event.lease)){connected=false;el("connection").textContent="控制租约已结束 / 可按原世界重连";controls();}});
  ws.onclose=()=>{if(own!==generation||socket!==ws)return;connected=false;linked=false;if(!current.client.observation?.outcome){el("connection").textContent="连接关闭 / 同一世界重连";note("连接已关闭；没有把未确认命令标为成功。原Actor/席位继续按规则运行。");}controls();};
  const welcome=await deadline(current.client.connect(),"身份/席位握手");
  if(welcome.faction!==info.faction||welcome.mode!==info.mode)throw new Error("服务器握手与已认证席位不一致");
  linked=true;
  if(own!==generation||signal.aborted)return;
  await deadline(current.client.acquireLease("Sovereign"),"控制租约");
  if(own!==generation||signal.aborted)return;
  connected=true;el("connection").textContent=info.custom?"私有自创席位已连接 / 原租约确认；等待双方或继续原局":info.mode==="PvP_Ranked"?"排位席位已连接 / 租约确认；等待双方连接或原局继续":"原世界仿真中 / 已获控制租约";
  button("surrender").textContent=info.custom?"自创投降（非排位）":info.mode==="PvP_Ranked"?"投降排位（计入真实结果）":"结束练习（投降）";
  el("arena-mode").textContent=info.custom?`私有自创PvP · 阵营 ${info.faction} · 非排位`:info.mode==="PvP_Ranked"?`真实人类排位 · 阵营 ${info.faction}`:"已有世界单人 PvE · 非排位";
  note(`${info.recovered?"已从持久提交恢复":"已连接"}世界 ${info.world_id}；真实阵营观测/命令回执，不替换模板。`);controls();mountGpu(current,info);
}
async function start(authored=false):Promise<void>{
  const previous=session,ended=!!arena?.client.observation?.outcome||previous?.match_state==="Completed";
  const own=++generation;clearRuntime();session=null;starting=true;logLines=[];controls();
  const pending=new AbortController();request=pending;
  el("connection").textContent=authored?"恢复已有世界/席位…":"连接显式本地练习…";
  let client:ArenaWorldClient|null=null;
  try{
    let info:Session;
    if(authored){
      const world=input("world-id").value.trim().toLowerCase(),origin=input("api-origin").value.trim(),token=input("capability").value.trim();
      client=token?new ArenaWorldClient(origin,token):previous&&!previous.practice&&previous.world_id===world&&previous.client.origin===new URL(origin).origin?previous.client:null;
      if(!client)throw new Error("请输入同一世界的正式凭据；不会转为practice创建另一世界");
      input("capability").value="";if(previous&&previous.client!==client)previous.client.destroy();
      const run=await client.run(world,pending.signal);info={...run,client,practice:false};
    }else if(previous?.practice&&!ended){client=previous.client;info=previous;}
    else{
      const response=await fetch("/api/v1/local/practice",{method:"POST",headers:{"X-Rynvek-Local":"1"},signal:pending.signal,cache:"no-store",redirect:"error"});
      if(!response.ok)throw new Error(`本地练习拒绝 ${response.status}`);
      const body=await response.json() as WorldRun & {token:string};
      client=new ArenaWorldClient(location.origin,body.token);info={...body,client,practice:true,match_state:"Created",recovered:false};
    }
    if(own!==generation||pending.signal.aborted){if(client!==previous?.client)client?.destroy();return;}
    if(previous&&previous.client!==client)previous.client.destroy();session=info;
    if(info.match_state==="Completed"){el("connection").textContent="世界已终局 / 可打开真实回放";note("保持终局和历史，不重建或重置世界；可打开签名重演。");return;}
    await connectLive(info,own,pending.signal);
  }catch(error){if(own===generation){connected=false;gpuRenderer?.dispose();gpuRenderer=null;arena?.dispose();arena=null;socket?.close();socket=null;
    if(client&&session?.client!==client)client.destroy();el("connection").textContent="连接失败 / 未确认成功";
    note(`${String(error)}；已有世界开局可能已提交，请按同一世界重连，不重复创建世界。`);}}
  finally{if(previous&&session?.client!==previous.client)previous.client.destroy();if(own===generation){starting=false;request=null;controls();}}
}
async function watchCustom(info:Session,own:number):Promise<void>{
 if(own!==generation||session!==info||!connected)return;
 if(customBusy){customTimer=setTimeout(()=>void watchCustom(info,own),250);return;}
 const pending=new AbortController();customRead=pending;const timer=setTimeout(()=>pending.abort(),2500);
 try{
  const room=await info.client.customRoom(info.world_id,undefined,pending.signal);if(own!==generation||session!==info)return;
  if(room.status==="closed"||room.status==="ended"||!room.seat||room.version!==customRoom?.version||room.seat.match_id!==info.match_id){
   generation++;clearRuntime();info.match_state=room.seat?.match_state??"Aborted";session=room.status==="ended"&&info.match_state==="Completed"?info:null;customRoom=room;el("connection").textContent="自创席位授权已关闭或对局结束";el("custom-status").textContent=`真实房间状态 ${room.status}；未保留私有画面`;controls();return;
  }
  info.match_state=room.seat.match_state;customRoom=room;
  el("custom-status").textContent=`主体 ${room.principal_id} · ${room.role} · ${info.match_state} · 自己的阵营 ${info.faction}（非排位）`;controls();
  customTimer=setTimeout(()=>void watchCustom(info,own),250);
 }catch(error){if(own===generation&&session===info&&!customBusy){generation++;clearRuntime();session=null;customRoom=null;el("custom-status").textContent=`控制准入未确认，已停止并清空：${String(error)}；显式重连对账`;controls();}}
 finally{clearTimeout(timer);if(customRead===pending)customRead=null;}
}
function customControls():void{
 const locked=customBusy||starting||rankBusy||!!rankClient||!!watching||!!playback;
 button("custom-create").disabled=locked||!!customClient||connected;
 button("custom-load").disabled=locked||connected;
 button("custom-invite").disabled=locked||customRoom?.role!=="owner"||customRoom?.status!=="unconfigured";
 button("custom-accept").disabled=locked||customRoom?.role!=="player"||customRoom?.status!=="invited";
 button("custom-connect").disabled=locked||connected||!customRoom||!["ready","active"].includes(customRoom.status)||(customRoom.status==="ready"&&customRoom.role!=="owner");
 button("custom-close").disabled=locked||!customRoom||["closed","unconfigured"].includes(customRoom.status);
}
function resetCustom():void{customEpoch++;customPending?.abort();customPending=null;if(customClient&&customClient!==session?.client)customClient.destroy();customClient=null;customRoom=null;customBusy=false;el("custom-status").textContent="未验证自创身份；断开不代表关闭房间";}
async function customAction(action:"create"|"load"|"invite"|"accept"|"connect"|"close"):Promise<void>{
 if(customBusy||starting||rankClient||watching||playback)return;
 const epoch=customEpoch;customBusy=true;const pending=new AbortController();customPending=pending;controls();const timer=setTimeout(()=>pending.abort(),8000);
 let client=customClient;
 try{
  if(!client){client=new ArenaWorldClient(input("api-origin").value.trim(),input("capability").value.trim());input("capability").value="";customClient=client;}
  let world=input("world-id").value.trim().toLowerCase();
  if(action==="create") {world=await client.createCustom(input("custom-title").value,Number(input("custom-seed").value),pending.signal);if(epoch!==customEpoch)return;input("world-id").value=world;note(`已创建真实Private双人世界 ${world}，不要重复创建。`);}
  const version=customRoom?.version;
  const payload=action==="invite"?{action:"invite" as const,guest_id:input("custom-guest").value.trim()}:action==="accept"||action==="connect"||action==="close"?{action,version:version??""}:undefined;
  const room=await client.customRoom(world,payload,pending.signal);if(epoch!==customEpoch)return;
  customRoom=room;el("custom-status").textContent=`已验证主体 ${room.principal_id} · ${room.role} · ${room.status}${room.guest_id?` · 所有者 ${room.owner_id} / 受邀玩家 ${room.guest_id}`:""}（真实准入，不代表在线）`;
  if(action==="connect"){
   const seat=room.seat;if(!seat)throw new Error("未获实际已冻结席位");
   if(seat.match_state==="Completed"||seat.match_state==="Aborted"){note("原局已结束，不重置世界；完整私有归档仍仅所有者可取。");return;}
   const own=++generation,previous=session;clearRuntime();if(previous?.client!==client)previous?.client.destroy();
   const info:Session={...seat,client,practice:false};session=info;starting=true;
   await connectLive(info,own,pending.signal);if(epoch!==customEpoch)return;
   customTimer=setTimeout(()=>void watchCustom(info,own),250);
  }
  if(action==="close"){
   generation++;clearRuntime();session=null;
   el("connection").textContent="自创局授权已关闭 / 非排位中止";
   note("服务器已确认关闭，尚未结束的整局中止，双方连接/租约/排队输入已回收；不改写既有历史或结果。");
  }
 }catch(error){if(epoch===customEpoch){
  if(action==="connect"){generation++;clearRuntime();session=null;}
  customRoom=null;el("custom-status").textContent=`自创操作未确认：${String(error)}；先读取房间对账，不自动重试写入或重复创建。`;note(String(error));
 }}finally{clearTimeout(timer);if(epoch===customEpoch){customBusy=false;starting=false;customPending=null;controls();}}
}
for(const action of ["create","load","invite","accept","connect","close"] as const)button(`custom-${action}`).onclick=()=>void customAction(action);
function showObserverGrants():void{
  const container=el("observer-grants");container.replaceChildren();
  if(!observerGrants.length)container.textContent="当前没有已确认有效准入（不是在线人数）";
  for(const grant of observerGrants){const row=document.createElement("p"),revoke=document.createElement("button");
    row.textContent=`主体 ${grant.principal_id} · 阵营 ${grant.faction} · 只读准入 `;revoke.textContent=`撤销 ${grant.principal_id}`;
    revoke.onclick=()=>void manageObservers("revoke",grant);row.append(revoke);container.append(row);}
}
async function manageObservers(action:"list"|"grant"|"revoke",grant?:ObserverGrant):Promise<void>{
  const info=session,own=generation;if(!info||info.practice||grantBusy||starting)return;
  grantBusy=true;const pending=new AbortController();grantRequest=pending;controls();const timer=setTimeout(()=>pending.abort(),4000);
  try{
    if(action==="grant")await info.client.grantObserver(info.world_id,input("observer-principal").value.trim(),info.faction,pending.signal);
    if(action==="revoke"&&grant)await info.client.revokeObserver(info.world_id,grant,pending.signal);
    const grants=await info.client.observerGrants(info.world_id,pending.signal);
    if(own!==generation||session!==info)return;observerGrants=grants;showObserverGrants();
    note(action==="list"?"已读取持久准入清单，不代表在线成员。":"授权操作已获服务器回执，清单已重新读取。");
  }catch(error){if(own===generation){observerGrants=[];el("observer-grants").textContent="状态未确认，请显式刷新；不自动重试写入";note(String(error));}}
  finally{clearTimeout(timer);if(own===generation){grantBusy=false;grantRequest=null;controls();}}
}
function stopObservation(state:Watching,message:string):void{
  if(watching!==state)return;generation++;clearRuntime();starting=false;
  el("connection").textContent="观战已停止 / 不保留私有画面";el("observer-status").textContent=message;note(message);controls();
}
async function pollObservation(state:Watching):Promise<void>{
  if(watching!==state||state.own!==generation||state.leaving)return;
  const pending=new AbortController();state.pending=pending;const timer=setTimeout(()=>pending.abort(),2500);
  try{
    const frame=await state.client.observe(state.frame.world_id,pending.signal);
    if(watching!==state||state.own!==generation||state.leaving)return;
    state.source.publish(frame);state.frame=frame;
    el("observer-status").textContent=`真实只读观战 · 主体 ${frame.principal_id} · 授权阵营 ${frame.faction} · Tick ${frame.messages.welcome.tick}（无控制租约）`;
    state.timer=setTimeout(()=>void pollObservation(state),250);
  }catch(error){if(watching===state&&!state.leaving)stopObservation(state,`观战准入/连接未确认：${String(error)}。已停止拉取并清空；请重新确认，不自动重试。`);}
  finally{clearTimeout(timer);state.pending=null;}
}
async function observeWorld():Promise<void>{
  if(starting||connected||playback||watching||rankClient||customClient)return;
  const own=++generation;const previous=session;clearRuntime();previous?.client.destroy();session=null;starting=true;controls();
  const pending=new AbortController();request=pending;const timer=setTimeout(()=>pending.abort(),3000);let client:ArenaWorldClient|null=null;
  try{
    client=new ArenaWorldClient(input("api-origin").value.trim(),input("capability").value.trim());input("capability").value="";
    const frame=await client.observe(input("world-id").value.trim().toLowerCase(),pending.signal);
    if(own!==generation||pending.signal.aborted){client.destroy();return;}
    const source=new ObserverTransport(frame),state:Watching={client,frame,source,own,timer:null,pending:null,leaving:false};watching=state;
    const current=createArenaApp(source,{principalId:frame.principal_id,kind:frame.principal_kind==="Human"?"Human":frame.principal_kind==="Agent"?"Agent":"Script",faction:frame.faction});arena=current;current.ui.camera.yDown=gpuRequested;
    current.client.on(event=>{if(watching===state&&arena===current&&event.type==="observation")display();});
    await current.client.connect();if(own!==generation)return;current.controller.jumpToNexus();current.ui.camera.zoom=1.8;
    el("connection").textContent="定向只读观战 / 无控制席位";el("arena-mode").textContent=`私有世界观战 · 仅阵营 ${frame.faction}`;
    el("observer-status").textContent=`准入已确认 · 主体 ${frame.principal_id} · 阵营 ${frame.faction}`;
    mountGpu(current,frame);
    display();state.timer=setTimeout(()=>void pollObservation(state),250);
  }catch(error){if(own===generation){clearRuntime();client?.destroy();note(`观战未确认：${String(error)}`);el("connection").textContent="观战被拒绝 / 未显示私有状态";}}
  finally{clearTimeout(timer);if(own===generation){starting=false;request=null;controls();}}
}
button("observe-world").onclick=()=>void observeWorld();
button("refresh-observers").onclick=()=>void manageObservers("list");
button("grant-observer").onclick=()=>void manageObservers("grant");
button("leave-observer").onclick=()=>{const state=watching;if(!state||starting)return;starting=true;state.leaving=true;if(state.timer)clearTimeout(state.timer);state.pending?.abort();controls();
  const pending=new AbortController();request=pending;const timer=setTimeout(()=>pending.abort(),3000);
  void state.client.leaveObserver(state.frame.world_id,state.frame,pending.signal).then(()=>stopObservation(state,"服务器已确认离开，原观战准入已撤销；重新加入须所有者再次授权。"))
    .catch(error=>stopObservation(state,`离开未确认：${String(error)}。本地已清空；请让所有者刷新对账，不将超时当撤销成功。`)).finally(()=>clearTimeout(timer));
};
function rankControls():void{
  const locked=rankBusy||starting||connected||!!playback||!!watching||!!customClient;
  button("rank-auth").disabled=locked||!!rankClient;
  button("rank-join").disabled=locked||!rankContext||rankContext.status!=="idle";
  button("rank-refresh").disabled=locked||!rankClient;
  button("rank-cancel").disabled=locked||rankContext?.status!=="queued";
  button("rank-enter").disabled=locked||!rankContext?.active;
}
function resetRank():void{
  rankEpoch++;rankRequest?.abort();rankRequest=null;if(rankTimer)clearTimeout(rankTimer);rankTimer=null;
  rankClient?.destroy();rankClient=null;rankContext=null;rankBusy=false;
  el("rank-status").textContent="身份已清空；未声称离队成功或暂停对局。用同一正式身份重新验证可找回本节点队列/席位。";
}
function showRank():void{
  const c=rankContext;if(!c)return;
  el("rank-status").textContent=`已认证主体 ${c.principal_id} · human · ${c.status==="queued"?"真实排队中，等待另一注册玩家":c.active?`对局 ${c.active.match_id} / 世界 ${c.active.world_id} / 阵营 ${c.active.faction} / ${c.active.match_state} / 宿主 ${c.active.host.state}${c.active.host.origin?` (${c.active.host.origin})`:""}`:"未排队"} · ruleset ${c.ruleset_hash}`;
}
async function rankAction(action:"auth"|"join"|"refresh"|"cancel"):Promise<void>{
  if(rankBusy||connected||starting||playback||watching||customClient)return;
  if(rankTimer)clearTimeout(rankTimer);rankTimer=null;
  const own=rankEpoch;rankBusy=true;const pending=new AbortController();rankRequest=pending;controls();
  // 请求有界；超时不能证明入队/取消未提交，停止追踪并用只读刷新对账。
  const timer=setTimeout(()=>pending.abort(),8000);
  try{
    if(action==="auth"){
      rankClient=new ArenaWorldClient(input("api-origin").value.trim(),input("capability").value.trim());
      input("capability").value="";
    }
    const client=rankClient;if(!client)throw new Error("先验证正式身份");
    if(action==="join"||action==="cancel"||(action==="refresh"&&rankContext?.status==="queued")){
      const c=rankContext;if(!c)throw new Error("先读取当前规则集和身份");
      const reply=await client.queue(c.ruleset_hash,action==="cancel",c.principal_id,pending.signal,action==="refresh");
      if(own!==rankEpoch)return;
      if(reply.status==="matched")note("配对已提交，退出排队不能撤销对局；请连接真实席位。");
    }
    const context=await client.queueContext(pending.signal);if(own!==rankEpoch)return;
    rankContext=context;showRank();
    if(context.status==="queued")rankTimer=setTimeout(()=>{void rankAction("refresh");},3000);
  }catch(error){if(own===rankEpoch){
    el("rank-status").textContent=`匹配请求未确认：${String(error)}。不自动重试写入；刷新或重新验证同一身份对账。`;
    if(action==="auth"){rankClient?.destroy();rankClient=null;rankContext=null;}
  }}finally{clearTimeout(timer);if(own===rankEpoch){rankBusy=false;rankRequest=null;controls();}}
}
async function enterRanked():Promise<void>{
  if(rankBusy||starting||connected||playback||watching||!rankClient||!rankContext?.active)return;
  const client=rankClient,identity=rankContext.principal_id;rankBusy=true;controls();
  const e=rankEpoch,pending=new AbortController();rankRequest=pending;
  const timer=setTimeout(()=>pending.abort(),8000);
  try{
    // 进入前重新读取服务端席位，绝不相信输入的主体/阵营或陈旧匹配回执。
    const c=await client.queueContext(pending.signal);if(e!==rankEpoch)return;
    rankContext=c;showRank();let seat=c.active;
    if(!seat||c.principal_id!==identity||seat.mode!=="PvP_Ranked"||seat.league!=="human")throw new Error("没有当前人类排位席位；已有自创世界请走原世界入口");
    seat=await client.connectRanked(seat.match_id,identity,pending.signal);if(e!==rankEpoch)return;
    rankContext={...c,active:seat};showRank();
    generation++;clearRuntime();if(session?.client!==client)session?.client.destroy();
    const info:Session={...seat,client,practice:false,recovered:false};session=info;
    starting=true;const own=generation;request=pending;
    await connectLive(info,own,pending.signal);
  }catch(error){if(e===rankEpoch){connected=false;clearRuntime();note(String(error));el("connection").textContent="排位连接未确认；请刷新席位后重连";}}
  finally{clearTimeout(timer);if(e===rankEpoch){rankBusy=false;starting=false;rankRequest=null;request=null;controls();}}
}
for(const [id,action] of [["rank-auth","auth"],["rank-join","join"],["rank-refresh","refresh"],["rank-cancel","cancel"]] as const)button(id).onclick=()=>{void rankAction(action);};
button("rank-enter").onclick=()=>{void enterRanked();};
button("start").onclick=()=>{void start(false);};
button("connect-world").onclick=()=>{void start(true);};
button("disconnect").onclick=disconnect;
function playbackStatus(state:Playing):void{
  const f=state.frame;input("replay-tick").min=String(f.initial_tick);input("replay-tick").max=String(f.final_tick);
  el("replay-status").textContent=`服务器签名已验证 · 实际起点Tick ${f.initial_tick}/revision ${f.initial_revision} · 重演Tick ${f.tick}/${f.final_tick} · 已验记录 ${f.verified_records}/${f.total_records} · ${f.complete_verified?"完整终根/命令/检查点已复算一致":"仅当前已重演前缀验证，未验证后缀"}\nroot ${f.state_root}`;
  el("connection").textContent="只读签名重演 / 无控制租约";controls();
}
async function watchReplay():Promise<void>{
  if(!session)return;const info=session,own=++generation;clearRuntime();starting=true;controls();
  const pending=new AbortController();request=pending;
  const closePending=()=>{void info.client.closeOwnedPlayback(info.match_id).catch(()=>{});};
  pending.signal.addEventListener("abort",closePending,{once:true});
  try{
    const frame=await info.client.openPlayback(info.match_id,info.world_id,info.faction,pending.signal);
    if(own!==generation||pending.signal.aborted){void info.client.closePlayback(info.match_id,frame.cursor_id).catch(()=>{});return;}
    const source=new ReplayObservationTransport(frame);
    const current=createArenaApp(source,{principalId:info.principal_id,kind:"Human",faction:info.faction});arena=current;current.ui.camera.yDown=gpuRequested;
    const state:Playing={cursor:frame.cursor_id,frame,source,session:info,own,busy:false,playing:false,stop:false,timer:null,request:null};playback=state;
    current.client.on(e=>{if(own===generation&&arena===current&&e.type==="observation")display();});
    await current.client.connect();current.controller.jumpToNexus();current.ui.camera.zoom=1.8;
    input("replay-tick").value=String(frame.tick);playbackStatus(state);mountGpu(current,info);
  }catch(error){if(own===generation){note(String(error));el("connection").textContent="回放未就绪/拒绝；没有造帧";}}
  finally{pending.signal.removeEventListener("abort",closePending);if(own===generation){starting=false;request=null;controls();}}
}
async function seekReplay(state:Playing,target:number):Promise<void>{
  if(state!==playback||state.busy)return;state.busy=true;state.stop=false;state.request=new AbortController();controls();
  let previous=-1;
  try{
    while(state===playback&&state.own===generation&&!state.stop){
      const frame=await state.session.client.seek(state.session.match_id,state.cursor,state.session.world_id,state.session.faction,target,state.request.signal);
      if(state!==playback||state.own!==generation||state.stop)return;
      if(!frame.ready&&frame.verified_records<=previous)throw new Error("重演没有实际进展，已停止定位");previous=frame.verified_records;
      const resetGpu=gpuRequested&&(frame.tick<=state.frame.tick||JSON.stringify(frame.map)!==JSON.stringify(state.frame.map));
      state.frame=frame;if(arena)arena.client.events.length=0;state.source.publish(frame);
      if(frame.events.length)note(`本批真实重演事件 ${JSON.stringify(frame.events.at(-1))}${frame.events_truncated?`（另${frame.events_truncated}条显示裁剪，非日志删除）`:""}`);input("replay-tick").value=String(frame.tick);playbackStatus(state);
      if(resetGpu&&arena)mountGpu(arena,state.session);
      if(frame.ready)break;
    }
  }catch(error){if(state===playback&&!state.stop){state.playing=false;note(`回放停止：${String(error)}；仍只显示最后确认的前缀。`);}}
  finally{state.busy=false;state.request=null;if(state===playback)controls();}
}
function pauseReplay():void{if(!playback)return;playback.playing=false;playback.stop=true;playback.request?.abort();if(playback.timer)clearTimeout(playback.timer);controls();}
async function playReplay():Promise<void>{
  const state=playback;if(!state||state.busy)return;state.playing=true;controls();
  if(state.frame.complete_verified)await seekReplay(state,state.frame.initial_tick);
  const advance=async()=>{if(state!==playback||!state.playing)return;await seekReplay(state,Math.min(state.frame.final_tick,state.frame.tick+3));
    if(state!==playback)return;if(state.frame.complete_verified)state.playing=false;
    if(state.playing)state.timer=setTimeout(()=>{void advance();},100);controls();};
  void advance();
}
button("watch-replay").onclick=()=>{void watchReplay();};
button("seek-replay").onclick=()=>{if(playback)void seekReplay(playback,Number(input("replay-tick").value));};
button("play-replay").onclick=()=>{void playReplay();};button("pause-replay").onclick=pauseReplay;
button("close-replay").onclick=()=>{generation++;clearRuntime();el("connection").textContent="回放已关闭 / 可按原世界重连";controls();};
button("release-control").onclick=()=>{const current=arena;if(!connected||!current||playback)return;void deadline(current.client.releaseLease(),"交还控制").then(()=>{if(arena!==current)return;connected=false;note("服务器已确认交还租约；连接只读，可重新申请控制，不代表暂停或退出对局。");controls();}).catch(error=>note(String(error)));};
button("claim-control").onclick=()=>{const current=arena,own=generation;if(!linked||!current||playback||starting)return;starting=true;controls();void deadline(current.client.acquireLease("Sovereign"),"重新申请控制").then(()=>{if(own!==generation||arena!==current)return;connected=true;note("服务器已重新确认控制租约；后续命令使用当前 fencing。");}).catch(error=>{if(own===generation)note(String(error));}).finally(()=>{if(own===generation){starting=false;controls();}});};
button("select").onclick=()=>{if(arena?.client.observation){arena.ui.selection=new Set(arena.client.observation.units.filter(u=>u.faction===arena!.client.identity.faction).map(u=>u.id));display();}};
button("home").onclick=()=>arena?.controller.jumpToNexus();
for(const [id,order] of [["harvest","Harvest"],["move","Move"]] as const)button(id).onclick=()=>{armed=order;if(arena)arena.ui.buildMode=null;el("intent").textContent=`${order==="Move"?"移动":"采集"}：左键点击目标地块`;};
button("surrender").onclick=()=>{void arena?.controller.send({type:"surrender"}).then(()=>display());};
button("replay").onclick=()=>{void(async()=>{try{const info=session;if(!info)return;const own=generation;
  const blob=await info.client.replay(info.match_id);if(own!==generation||session!==info)return;
  const url=URL.createObjectURL(blob),a=document.createElement("a");a.href=url;a.download=`${info.match_id}.rreplay`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  note("已下载真实服务器签名世界归档；可在只读播放器重演，未到终根不声称全片验证。");
}catch(error){note(String(error));}})();};
function bindCanvasInput():void{
canvas.oncontextmenu=e=>{e.preventDefault();if(!canControl())return;const [x,y]=worldPoint(e);armed=null;drag=null;arena?.router.handlePointer({kind:"click",button:"right",x,y});display();};
canvas.onpointerdown=e=>{canvas.focus();if(e.button!==0||(!canControl()&&!playback))return;const r=canvas.getBoundingClientRect();drag={x:e.clientX-r.left,y:e.clientY-r.top,world:worldPoint(e)};canvas.setPointerCapture(e.pointerId);};
canvas.onpointermove=e=>{pointer=worldPoint(e);el("pointer").textContent=`Tile ${Math.floor(pointer[0])}, ${Math.floor(pointer[1])}${arena?.client.observation?` / 视野 ${fogAt(arena.client.observation,...pointer)}`:""}`;};
canvas.onpointerup=e=>{if(!drag||!arena)return;const start=drag;drag=null;const point=worldPoint(e);if(armed){void arena.controller.order(armed,point)?.then(()=>display());armed=null;}else if(Math.hypot(point[0]-start.world[0],point[1]-start.world[1])>.5&&!arena.ui.buildMode)arena.router.handlePointer({kind:"drag",button:"left",x:start.world[0],y:start.world[1],x2:point[0],y2:point[1]});else arena.router.handlePointer({kind:"click",button:"left",x:point[0],y:point[1],shift:e.shiftKey});display();};
canvas.onpointercancel=()=>{drag=null;};
canvas.onlostpointercapture=()=>{drag=null;};
canvas.onwheel=e=>{e.preventDefault();if(arena)arena.ui.camera.zoom=Math.min(4,Math.max(.35,arena.ui.camera.zoom*(e.deltaY>0?.85:1.15)));};
canvas.onkeydown=e=>{if(!arena)return;if(!canControl()&&!(["Home","ArrowUp","ArrowDown","ArrowLeft","ArrowRight","w","a","s","d","W","A","S","D"].includes(e.key)))return;if(e.key==="Home"){arena.controller.jumpToNexus();e.preventDefault();return;}if(e.key==="Escape"){armed=null;drag=null;}if(arena.router.handleKey({key:e.key,ctrlKey:e.ctrlKey,altKey:e.altKey,metaKey:e.metaKey,shiftKey:e.shiftKey}))e.preventDefault();display();};
}
bindCanvasInput();
if(!gpuRequested&&!ctx){el("renderer").textContent="Canvas2D 不可用";note("无法取得 Canvas2D 上下文，没有生成替代世界。");}
window.addEventListener("pagehide",()=>{disposed=true;cancelAnimationFrame(animationFrame);disconnect();});
window.addEventListener("pageshow",e=>{if(e.persisted){disposed=false;controls();
draw();}});
const terrainColors=["#27362b","#3b4547","#536c32","#6e3b28","#171d1b"];
function draw():void{
  if(disposed)return;
  if(gpuRequested){
    gpuRenderer?.sync(view(),devicePixelRatio||1);
    if(arena&&gpuRenderer){
      gpuRenderer.select([...arena.ui.selection],arena.ui.selectedBuilding);
      const mode=arena.ui.buildMode, terrain=arena.client.terrain;
      const origin:[number,number]=[Math.floor(pointer[0]),Math.floor(pointer[1])];
      const size=mode?buildingSpec(mode).footprint:0;
      const active=canControl()&&!arena.client.observation?.outcome;
      // 仅按公开地图边界裁剪意图，不查询隐藏占地或伪造可建判决。
      const placement=active&&mode&&terrain?.inBounds(...origin)&&terrain.inBounds(origin[0]+size-1,origin[1]+size-1)?{kind:mode,origin}:null;
      gpuRenderer.overlay(active&&drag&&!mode&&!armed?[drag.world,pointer]:null,placement);
    }
    animationFrame=requestAnimationFrame(draw);return;
  }
  if(!ctx)return;
  const dpr=devicePixelRatio||1,w=canvas.clientWidth,h=canvas.clientHeight;
  if(canvas.width!==Math.round(w*dpr)||canvas.height!==Math.round(h*dpr)){canvas.width=Math.round(w*dpr);canvas.height=Math.round(h*dpr);}
  ctx.setTransform(dpr,0,0,dpr,0,0);ctx.fillStyle="#070908";ctx.fillRect(0,0,w,h);
  const obs=arena?.client.observation,terrain=arena?.client.terrain;
  if(arena&&obs&&terrain){
    const v=view(),scale=viewScale(v),a=unproject(v,0,h),b=unproject(v,w,0);
    for(let y=Math.max(0,Math.floor(a[1]));y<Math.min(terrain.height,Math.ceil(b[1]));y++)for(let x=Math.max(0,Math.floor(a[0]));x<Math.min(terrain.width,Math.ceil(b[0]));x++){
      const fog=fogAt(obs,x,y);if(fog===0)continue;const [sx,sy]=project(v,x,y+1);
      ctx.globalAlpha=fog===1?1:.32;ctx.fillStyle=terrainColors[terrain.at(x,y)]??"#27362b";ctx.fillRect(sx,sy,scale+.5,scale+.5);
      if(scale>9){ctx.strokeStyle="#ffffff08";ctx.lineWidth=.5;ctx.strokeRect(sx,sy,scale,scale);}
    }
    ctx.globalAlpha=1;
    for(const building of obs.buildings){const size=buildingSpec(building.kind).footprint,[x,y]=project(v,building.origin[0],building.origin[1]+size);ctx.fillStyle=building.faction===obs.faction?"#3b5040":"#71493b";ctx.fillRect(x+1,y+1,size*scale-2,size*scale-2);ctx.strokeStyle=building.faction===obs.faction?"#a3d65c":"#ff9469";ctx.lineWidth=arena.ui.selectedBuilding===building.id?3:1;ctx.strokeRect(x+2,y+2,size*scale-4,size*scale-4);ctx.fillStyle="#d7e9c7";ctx.font=`${Math.max(9,Math.min(14,scale))}px monospace`;ctx.fillText(labels[building.kind]??building.kind.slice(4),x+4,y+15);if(!building.complete){ctx.fillStyle="#ead686";ctx.fillText(`施工 ${building.build_progress}`,x+3,y+size*scale-5);} }
    for(const u of obs.units){const [wx,wy]=rawToFloat(u.position),[x,y]=project(v,wx,wy),r=Math.max(3,scale*.35);ctx.fillStyle=u.faction===obs.faction?"#bee78a":"#ff9269";ctx.beginPath();ctx.moveTo(x,y-r);ctx.lineTo(x+r,y+r);ctx.lineTo(x-r,y+r);ctx.closePath();ctx.fill();if(arena.ui.selection.has(u.id)){ctx.strokeStyle="#e4ff9e";ctx.lineWidth=1.5;ctx.beginPath();ctx.arc(x,y,r+4,0,Math.PI*2);ctx.stroke();}ctx.fillStyle="#253229";ctx.fillRect(x-r,y-r-5,r*2,2);ctx.fillStyle="#a3d65c";ctx.fillRect(x-r,y-r-5,r*2*u.hp_milli/u.max_hp_milli,2);}
    if(arena.ui.buildMode){const size=buildingSpec(arena.ui.buildMode).footprint,[x,y]=project(v,Math.floor(pointer[0]),Math.floor(pointer[1])+size);ctx.strokeStyle="#ead686";ctx.lineWidth=2;ctx.strokeRect(x,y,size*scale,size*scale);}
    if(drag){const [x,y]=project(v,...drag.world),[x2,y2]=project(v,...pointer);ctx.strokeStyle="#c8efa0";ctx.strokeRect(x,y,x2-x,y2-y);}
  }else{ctx.fillStyle="#91a091";ctx.font="16px sans-serif";ctx.textAlign="center";ctx.fillText("连接已有Studio世界，或显式开始本地练习",w/2,h/2);ctx.textAlign="start";}
  animationFrame=requestAnimationFrame(draw);
}
controls();
draw();
