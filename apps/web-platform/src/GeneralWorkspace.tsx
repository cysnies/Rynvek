// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useRef, useState } from "react";
import type { RuleInstallRequest, SignedRuleRelease } from "@rynvek/agent-sdk";
import {GeneralViewport} from "./GeneralViewport.js";
import { GeneralWorldSession, type GeneralWorld } from "./general-remote.js";
/** 服务器观察工作台；不启动本地Worker或借Arena练习身份伪装远端执行。 */
export function GeneralWorkspace({ origin = location.origin, fetcher = fetch }: {
    origin?: string;
    fetcher?: typeof fetch;
}) {
    const [token, setToken] = useState(""), [client, setClient] = useState<GeneralWorldSession>(), [worlds, setWorlds] = useState<GeneralWorld[]>([]), [id, setId] = useState(""), [packages, setPackages] = useState<SignedRuleRelease[]>([]), [plan, setPlan] = useState(""), [fee, setFee] = useState(""), [message, setMessage] = useState("尚未验证身份"), [busy, setBusy] = useState(false), [, redraw] = useState(0);
    const live = useRef(0), session = useRef<GeneralWorldSession | undefined>(undefined), lock = useRef(false), opening = useRef<AbortController | undefined>(undefined);
    function disconnect() { live.current++; opening.current?.abort(); opening.current = undefined; session.current?.destroy(); session.current = undefined; setClient(undefined); setToken(""); setWorlds([]); setPackages([]); setPlan(""); setFee(""); setId(""); setMessage("身份已断开；已提交服务端操作不会撤销，请重新发现"); lock.current = false; setBusy(false); }
    useEffect(()=>client?.onPrivateInvalidated(()=>{setPackages([]);setPlan("");setFee("");redraw(v=>v+1);}),[client]);
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
    return <main><h1>General · 私有权威世界工作台</h1><p>单Chunk所有者受限World；不是Arena练习、任意World或已验收GPU画布。每次只明确推进一个真实实付Tick。</p>
 <section><h2>正式身份</h2>{!client ? <><label>既有能力令牌<input aria-label="General能力令牌" type="password" autoComplete="off" value={token} onChange={e => setToken(e.target.value)}/></label><button disabled={busy || !token} onClick={() => void perform(async () => { const epoch = live.current; const a = new AbortController(); opening.current = a; const c = await GeneralWorldSession.open(origin, token, fetcher, a.signal); if (opening.current === a)
        opening.current = undefined; if (epoch !== live.current) {
        c.destroy();
        return "";
    } session.current = c; setClient(c); setToken(""); return `已核正式人类身份 ${c.identity.principal_id}`; })}>验证既有身份</button></> : <p>正式主体 {client.identity.principal_id}；登记公钥 {client.identity.ed25519_pubkey}。凭据仅内存，不进URL/存储/包。</p>}<button onClick={disconnect}>断开并清空</button></section>
 <p role="status">{message}</p>
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
 <p>宿主fence：{client.hostFence ?? "未连接"}；控制fence：{client.controlFence ?? "未获取"}。空闲租约可能到期，无后台续租/自治推进。</p>
 <button disabled={client.controlFence === undefined || !observation || !!observation.rule_recovery} onClick={() => void perform(async () => { await client.advance("tick"); return "仅一个下一Tick已由原Actor提交确认"; })}>明确推进一个Tick</button>
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
 {client&&client.world&&<GeneralViewport key={client.world} session={client} onInvalidated={()=>redraw(v=>v+1)}/>}
    {observation && <section><h2>服务器已提交观察（只读）</h2><p>Tick {observation.commit.tick} · revision {observation.commit.revision} · 根 {observation.commit.state_root}</p><p>{observation.rule_recovery ? `隔离维护至Epoch ${observation.rule_recovery.epoch_tick}；本Tick维护实付 ${observation.rule_recovery.maintenance_paid_q}` : "未处于隔离维护；撤销仍可能令下一Tick拒绝"}</p><pre>{JSON.stringify({ bodies: observation.bodies, rule_meter: observation.rule_meter, life: observation.life, components: observation.components }, null, 2)}</pre></section>}
 <p>只读画布沿原FrameScene/Renderer/Worker消费服务器最小投影，不运行本地第二份World。真实浏览器/硬件GPU验收与协作自治仍待后续。</p>
 </main>;
}
