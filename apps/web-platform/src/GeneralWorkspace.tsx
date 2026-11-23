// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useRef, useState } from "react";
import type { RuleInstallRequest, SignedRuleRelease } from "@rynvek/agent-sdk";
import {GeneralDriverWorkspace} from "./GeneralDriverWorkspace.js";
import {GeneralDriverPanel} from "./GeneralDriverPanel.js";
import {GeneralViewport} from "./GeneralViewport.js";
import { GeneralRunInterrupted, GeneralWorldSession, type GeneralWorld } from "./general-remote.js";
/** 模式互斥挂载；切换即销毁旧凭据/私有态，不把owner会话交给协作面板。 */
export function GeneralWorkspace({origin=location.origin,fetcher=fetch}:{origin?:string;fetcher?:typeof fetch}){
    const [mode,setMode]=useState<"owner"|"driver">("owner");
    return <><nav aria-label="General工作模式"><button aria-pressed={mode==="owner"} onClick={()=>setMode("owner")}>所有者模式</button><button aria-pressed={mode==="driver"} onClick={()=>setMode("driver")}>规则协作者模式</button></nav>{mode==="owner"?<GeneralOwnerWorkspace origin={origin} fetcher={fetcher}/>:<GeneralDriverWorkspace origin={origin} fetcher={fetcher}/>}</>;
}
/** 服务器观察工作台；不运行本地仿真World，显示沿原只读Worker，不借Arena身份伪装。 */
function GeneralOwnerWorkspace({ origin = location.origin, fetcher = fetch }: {
    origin?: string;
    fetcher?: typeof fetch;
}) {
    const [token, setToken] = useState(""), [client, setClient] = useState<GeneralWorldSession>(), [worlds, setWorlds] = useState<GeneralWorld[]>([]), [id, setId] = useState(""), [packages, setPackages] = useState<SignedRuleRelease[]>([]), [plan, setPlan] = useState(""), [fee, setFee] = useState(""), [message, setMessage] = useState("尚未验证身份"), [busy, setBusy] = useState(false), [, redraw] = useState(0);
    const live = useRef(0), session = useRef<GeneralWorldSession | undefined>(undefined), lock = useRef(false), opening = useRef<AbortController | undefined>(undefined);
    const running=useRef<AbortController | undefined>(undefined);
    const [runCount,setRunCount]=useState("16"), [runProgress,setRunProgress]=useState<number | undefined>();
    function disconnect() { running.current?.abort();running.current=undefined;setRunProgress(undefined);live.current++; opening.current?.abort(); opening.current = undefined; session.current?.destroy(); session.current = undefined; setClient(undefined); setToken(""); setWorlds([]); setPackages([]); setPlan(""); setFee(""); setId(""); setMessage("身份已断开；已提交服务端操作不会撤销，请重新发现"); lock.current = false; setBusy(false); }
    useEffect(()=>client?.onPrivateInvalidated(()=>{setPackages([]);setPlan("");setFee("");redraw(v=>v+1);}),[client]);
    useEffect(()=>{
        const stop=()=>{running.current?.abort();if(running.current)session.current?.invalidatePrivate();};
        const hidden=()=>{if(document.visibilityState!=="visible")stop();};
        window.addEventListener("offline",stop);document.addEventListener("visibilitychange",hidden);
        return ()=>{window.removeEventListener("offline",stop);document.removeEventListener("visibilitychange",hidden);running.current?.abort();};
    },[]);
    useEffect(() => () => { live.current++; opening.current?.abort(); opening.current = undefined; session.current?.destroy(); }, []);
    async function perform(work: () => Promise<string>) { if (lock.current)
        return; lock.current = true; const epoch = live.current; setBusy(true); try {
        const m = await work();
        if (epoch === live.current)
            setMessage(m);
    }
    catch {
        if (epoch === live.current)
            setMessage("操作未确认成功；请刷新权威状态/重新连接，不要盲目重复创建或推进");
    }
    finally {
        if (epoch === live.current) {
            lock.current = false;
            setBusy(false);
            redraw(v => v + 1);
        }
    } }
    function parsed(): RuleInstallRequest { if (plan.length > 65536)
        throw new Error("计划超过64KiB"); return JSON.parse(plan) as RuleInstallRequest; }
    const observation = client?.observation;
    return <main><h1>General · 私有权威世界工作台</h1><p>单Chunk所有者受限World；不是Arena练习、任意World或已验收GPU画布。单步或明确数量的有界运行均逐Tick由原Actor实付提交；不是离线自治作业。</p>
 <section><h2>正式身份</h2>{!client ? <><label>既有能力令牌<input aria-label="General能力令牌" type="password" autoComplete="off" value={token} onChange={e => setToken(e.target.value)}/></label><button disabled={busy || !token} onClick={() => void perform(async () => { const epoch = live.current; const a = new AbortController(); opening.current = a; const c = await GeneralWorldSession.open(origin, token, fetcher, a.signal); if (opening.current === a)
        opening.current = undefined; if (epoch !== live.current) {
        c.destroy();
        return "";
    } session.current = c; setClient(c); setToken(""); return `已核正式人类身份 ${c.identity.principal_id}`; })}>验证既有身份</button></> : <p>正式主体 {client.identity.principal_id}；登记公钥 {client.identity.ed25519_pubkey}。凭据仅内存，不进URL/存储/包。</p>}<button onClick={disconnect}>断开并清空</button></section>
 <p role="status">{message}</p>
 {runProgress!==undefined&&<p>本次已确认 {runProgress} 个Tick。<button onClick={()=>running.current?.abort()}>停止后续Tick</button></p>}
 {client && <fieldset disabled={busy}><legend>私有General操作</legend>
 <button onClick={() => void perform(async () => { const v = await client.listWorlds(); if (session.current === client)
            setWorlds(v); return "已读取首100条世界元数据并过滤本人General；更多世界可用精确ID"; })}>发现本人世界</button>
 <ul>{worlds.map(w => <li key={w.world_id}><button onClick={() => { setId(w.world_id); }}>{w.title} · {w.world_id}</button></li>)}</ul>
 <label>上传自己明确准备的RYWL创世包（≤128KiB）<input aria-label="General创世包" type="file" accept=".rworld,.world" onChange={e => { const f = e.target.files?.[0]; e.target.value = ""; if (f)
            void perform(async () => { if (f.size > 131072)
                throw new Error("包过大"); const result = await client.create(new Uint8Array(await f.arrayBuffer())); if (session.current === client)
                setId(result); return `世界已真实创建 ${result}；请明确读取，不会自动认领宿主`; }); }}/></label>
 <label>世界ID<input aria-label="General世界ID" value={id} onChange={e => setId(e.target.value)}/></label>
 <button onClick={() => void perform(async () => { await client.select(id.trim()); return "已读取本人世界权威状态；未认领宿主"; })}>读取所选世界</button>
 <button disabled={!client.world} onClick={() => void perform(async () => { await client.refresh(); return "已刷新权威状态，本地控制fence失效；须重新连接/取得控制"; })}>刷新并清控制</button>
 <button disabled={!client.world} onClick={() => void perform(async () => { await client.connect(); return "宿主已确认；尚未获取Sovereign控制"; })}>明确连接宿主</button>
 <button disabled={client.hostFence === undefined} onClick={() => void perform(async () => { await client.acquire(); return "已取得原Sovereign控制租约"; })}>取得Sovereign</button>
 <p>宿主fence：{client.hostFence ?? "未连接"}；控制fence：{client.controlFence ?? "未获取"}。空闲租约可能到期，无后台续租；不明确开始不会自行推进。</p>
 <button disabled={client.controlFence === undefined || !observation || !!observation.rule_recovery} onClick={() => void perform(async () => { await client.advance("tick"); return "仅一个下一Tick已由原Actor提交确认"; })}>明确推进一个Tick</button>
 <label>有界运行Tick数量（1..128）<input aria-label="General有界Tick数量" type="number" min="1" max="128" value={runCount} onChange={e=>setRunCount(e.target.value)}/></label>
 <p>仅依次执行当前规则，每Tick独立付费且已提交不可退回；无外部模型、自动安装/维护/重试/换控制。停止或断网时末请求可能已提交，须刷新确认。</p>
 <button disabled={client.controlFence===undefined||!observation||!!observation.rule_recovery} onClick={()=>void perform(async()=>{
     const a=new AbortController();running.current=a;setRunProgress(0);const epoch=live.current;
     try{const n=await client.runTicks(Number(runCount),a.signal,(n)=>{if(epoch===live.current){setRunProgress(n);redraw(v=>v+1);}});return `有界运行已确认${n}个原Actor实付Tick；非整批事务`;}catch(e){if(e instanceof GeneralRunInterrupted)return e.message;throw e;}finally{if(running.current===a){running.current=undefined;setRunProgress(undefined);}}
 })}>明确开始有界实付运行</button>
 <h2>私有规则：发现不等于可信安装</h2>
 <button onClick={() => void perform(async () => { const v = await client.discoverRules(); if (session.current === client)
            setPackages(v); return "已发现签名元数据；尚未下载验签或安装，撤销状态由服务端最终提交锁再验"; })}>发现私有规则版本</button>
 <ul>{packages.map(p => <li key={p.manifest.package_id + "@" + p.manifest.version}>{p.manifest.package_id}@{p.manifest.version} · CID {p.manifest.artifact_cid}（元数据，非安装承诺）</li>)}</ul>
 <label>显式完整计划JSON：root精确引用与modules一对一映射<textarea maxLength={65536} aria-label="General安装计划" value={plan} onChange={e => setPlan(e.target.value)}/></label>
 <button disabled={!plan} onClick={() => void perform(async () => `可信闭包已下载验签：${(await client.verify(parsed())).join("、")}；未安装，提交时仍复验撤销`)}>只下载可信验签</button>
 <button disabled={!plan || client.controlFence === undefined || !observation || (observation.commit.tick + 1) % 4 !== 0} onClick={() => void perform(async () => { await client.advance("install", parsed()); return "Epoch安装及本Tick实付已由原Actor确认"; })}>明确Epoch安装并实付Tick</button>
 <p>维护单价：每活控制身体 {observation?.recovery_maintenance_q_per_body ?? "无可用报价"} EnergyQ；不是规则执行gas。旧/新图均不执行；每次真实维护Tick热耗，到Epoch仍须手动安装。</p>
 <label>明确接受上述每身体维护报价<input aria-label="General维护报价" value={fee} onChange={e => setFee(e.target.value)}/></label>
 <button disabled={!plan || !fee || client.controlFence === undefined || !observation || (observation.commit.tick + 1) % 4 === 0} onClick={() => void perform(async () => { await client.advance("recover", parsed(), Number(fee)); return "仅一个实付隔离维护Tick已确认；没有自动激活新图"; })}>明确实付维护一个Tick</button>
 </fieldset>}
 {client&&client.world&&<GeneralDriverPanel key={`drivers:${client.world}`} session={client} busy={busy} perform={perform}/>}
 {client&&client.world&&<GeneralViewport key={`render:${client.world}`} session={client} onInvalidated={()=>redraw(v=>v+1)}/>}
    {observation && <section><h2>服务器已提交观察（只读）</h2><p>Tick {observation.commit.tick} · revision {observation.commit.revision} · 根 {observation.commit.state_root}</p><p>{observation.rule_recovery ? `隔离维护至Epoch ${observation.rule_recovery.epoch_tick}；本Tick维护实付 ${observation.rule_recovery.maintenance_paid_q}` : "未处于隔离维护；撤销仍可能令下一Tick拒绝"}</p><pre>{JSON.stringify({ bodies: observation.bodies, rule_meter: observation.rule_meter, life: observation.life, components: observation.components }, null, 2)}</pre></section>}
 <p>只读画布沿原FrameScene/Renderer/Worker消费服务器最小投影，不运行本地第二份World。真实浏览器/硬件GPU验收及耐久自治仍欠；当前协作只授权最小帧与受控规则驱动。</p>
 </main>;
}
