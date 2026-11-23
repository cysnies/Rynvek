// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useState } from "react";
import type { GeneralDriverGrant } from "@rynvek/agent-sdk";
import type { GeneralWorldSession } from "./general-remote.js";
/** 只在已核所有者世界内管理显式目的；版本复制不会替代协作者本人身份。 */
export function GeneralDriverPanel({ session, busy, perform }: {
    session: GeneralWorldSession;
    busy: boolean;
    perform: (f: () => Promise<string>) => Promise<void>;
}) {
    const [pid, setPid] = useState(""), [consent, setConsent] = useState(false), [grants, setGrants] = useState<GeneralDriverGrant[]>([]);
    useEffect(() => session.onPrivateInvalidated(() => { setGrants([]); setPid(""); setConsent(false); }), [session]);
    const world = session.world;
    return <fieldset disabled={busy || !world}><legend>所有者 · 规则协作目的准入</legend><p>只授权最小身体帧及已有规则驱动，可能消耗原世界源能量；不授源码/私包/安装/维护，不是Studio许可。每次变更须精确当前版本。不会发送邀请。</p>
    <button onClick={() => void perform(async () => {
            const list = await session.listDrivers();
            if (session.world === world)
                setGrants(list);
            return "仅此世界规则驱动目的列表已读取";
        })}>刷新规则协作准入</button>
    <label>协作者正式主体ID<input aria-label="规则协作者主体" value={pid} onChange={e => { setPid(e.target.value); setConsent(false); }}/></label>
    <label><input aria-label="明确同意规则驱动目的" type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)}/>明确同意最小身体观察与逐Tick驱动，接受原世界费用，不授所有者视图</label>
    <button disabled={!consent || !pid} onClick={() => void perform(async () => {
            const old = grants.find(g => g.principal_id === pid);
            const g = await session.changeDriver(pid, old?.grant_id ?? null, true);
            if (session.world === world) {
                setGrants(v => [...v.filter(x => x.principal_id !== g.principal_id), g]);
                setConsent(false);
            }
            return "规则驱动目的已显式授权；请本人核对主体/世界/版本，不共享能力令牌";
        })}>明确授予规则驱动</button>
    <ul>{grants.map(g => <li key={g.principal_id}><p>主体 {g.principal_id} · {g.active ? "已授权" : "已撤销"} · 世界 {world} · 当前目的版本 {g.grant_id}</p><button disabled={!g.active} onClick={() => void perform(async () => {
                const next = await session.changeDriver(g.principal_id, g.grant_id, false);
                if (session.world === world)
                    setGrants(v => v.map(x => x.principal_id === next.principal_id ? next : x));
                return "目的已撤销，原提交锁将拒迟到候选；已见信息和已提交Tick不回滚";
            })}>撤销规则驱动 {g.principal_id}</button></li>)}</ul>
    </fieldset>;
}
