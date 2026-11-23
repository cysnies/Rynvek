// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useRef, useState } from "react";
import { GeneralWorldSession, type GeneralDriverSession } from "./general-remote.js";
import { GeneralViewport } from "./GeneralViewport.js";
/** 与owner工作台互斥挂载，不读取owner元数据/源码/账户，不开启客户端循环。 */
export function GeneralDriverWorkspace({ origin, fetcher }: {
    origin: string;
    fetcher: typeof fetch;
}) {
    const [token, setToken] = useState(""), [world, setWorld] = useState(""), [grant, setGrant] = useState(""), [client, setClient] = useState<GeneralDriverSession>(), [busy, setBusy] = useState(false), [message, setMessage] = useState("尚未核协作身份"), [, redraw] = useState(0);
    const current = useRef<GeneralDriverSession | undefined>(undefined), epoch = useRef(0), lock = useRef(false), opening = useRef<AbortController | undefined>(undefined);
    function clear() { epoch.current++; opening.current?.abort(); opening.current = undefined; current.current?.destroy(); current.current = undefined; setClient(undefined); setToken(""); setWorld(""); setGrant(""); setBusy(false); lock.current = false; setMessage("本地协作身份/帧/fence已清；不是服务端交还或回滚，末请求请重新读取对账"); }
    useEffect(() => () => { epoch.current++; opening.current?.abort(); current.current?.destroy(); }, []);
    useEffect(() => {
        const off = client?.onPrivateInvalidated(() => { redraw(v => v + 1); setMessage("协作观察/会话失效或超过12秒未复验，已清私有帧与双fence，请重新读取"); });
        const stop = () => { client?.invalidatePrivate(); setMessage("离线/隐藏，协作帧和双fence已清；末请求若未确认，请重新对账"); };
        const hidden = () => {
            if (document.hidden)
                stop();
        };
        window.addEventListener("offline", stop);
        document.addEventListener("visibilitychange", hidden);
        return () => { off?.(); window.removeEventListener("offline", stop); document.removeEventListener("visibilitychange", hidden); };
    }, [client]);
    async function perform(work: () => Promise<string>) {
        if (lock.current)
            return;
        lock.current = true;
        setBusy(true);
        const e = epoch.current;
        try {
            const m = await work();
            if (e === epoch.current)
                setMessage(m);
        }
        catch {
            if (e === epoch.current) {
                current.current?.invalidatePrivate();
                setMessage("协作请求未确认；已清私有帧与控制，不自动重试。请刷新准入或联系所有者");
            }
        }
        finally {
            if (e === epoch.current) {
                lock.current = false;
                setBusy(false);
                redraw(v => v + 1);
            }
        }
    }
    const bound = !!client?.world && client.world === world.trim() && client.grantId === grant.trim();
    const frame = bound ? client?.frame : undefined;
    return <main><h1>General · 目的隔离规则协作</h1><p>仅最小身体观察及已有规则逐Tick驱动，不授owner账户/源码/包，不是耐久自治。未启GPU也在12秒未复验后清帧/控制，刷新不会自行推进。所有者须明确提供世界与当前目的版本；不自动发现私有世界或发送邀请。</p>
    <section><h2>协作者正式身份</h2>{!client ? <><label>本人既有能力令牌<input aria-label="协作者能力令牌" type="password" autoComplete="off" value={token} onChange={e => setToken(e.target.value)}/></label><button disabled={busy || !token} onClick={() => void perform(async () => {
                const e = epoch.current, a = new AbortController();
                opening.current = a;
                const c = await GeneralWorldSession.openRuleDriver(origin, token, fetcher, a.signal);
                if (e !== epoch.current) {
                    c.destroy();
                    return "";
                }
                opening.current = undefined;
                current.current = c;
                setClient(c);
                setToken("");
                return `已核协作者 ${c.identity.principal_id}；尚未访问任何世界`;
            })}>验证本人协作身份</button></> : <p>正式协作者 {client.identity.principal_id}；凭据只驻内存，不共享owner令牌。</p>}<button onClick={clear}>断开协作并清空</button></section>
    <p role="status">{message}</p>
    {client && <fieldset disabled={busy}><legend>明确目的与原Actor操作</legend><label>所有者提供的世界ID<input aria-label="协作世界ID" value={world} onChange={e => { client.invalidatePrivate(); setWorld(e.target.value); }}/></label><label>当前准入版本ID<input aria-label="协作准入版本" value={grant} onChange={e => { client.invalidatePrivate(); setGrant(e.target.value); }}/></label>
    <button onClick={() => void perform(async () => { await client.join(world.trim(), grant.trim()); return "仅最小帧已读取；未取得宿主/控制或推进"; })}>绑定目的并只读</button>
    <button disabled={!bound} onClick={() => void perform(async () => { await client.refresh(); return "已复验目的并读取最小帧，双fence已清"; })}>刷新协作并清控制</button>
    <button disabled={!bound} onClick={() => void perform(async () => { await client.connect(); return "原宿主已确认，尚未取得控制"; })}>明确连接协作宿主</button>
    <button disabled={!bound || client.hostFence === undefined} onClick={() => void perform(async () => { await client.acquire(); return "已取得原Sovereign租约；未推进"; })}>明确取得协作控制</button>
    <p>宿主fence {client.hostFence ?? "未连接"}；控制fence {client.controlFence ?? "未取得"}。不抢占仍有效的人类租约；撤销、重授权必须按原版本重新对账。断开/隐藏只清本地，不自动释放服务端租约；请先明确交还，或由所有者撤销目的后接管。</p>
    <button disabled={!frame || client.controlFence === undefined} onClick={() => void perform(async () => { await client.advance(); return "仅一个原规则Tick及对应权威帧已确认；费用沿用世界原配置"; })}>明确驱动一个规则Tick</button>
    <button disabled={!bound || client.controlFence === undefined} onClick={() => void perform(async () => { await client.release(); return "控制已交还，未关闭世界或撤销历史Tick"; })}>明确交还协作控制</button></fieldset>}
    {client && bound && <GeneralViewport key={client.world} session={client} onInvalidated={() => redraw(v => v + 1)}/>}
    {frame && <section><h2>最小已提交帧（非owner观察）</h2><p>协作Tick {frame.tick} · 根 {frame.stateRoot}</p><pre>{JSON.stringify(frame, null, 2)}</pre></section>}
    </main>;
}
