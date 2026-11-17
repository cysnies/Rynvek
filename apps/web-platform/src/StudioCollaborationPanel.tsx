// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {useState} from 'react';
import {Button,Panel} from '@rynvek/ui';
import {type StudioClient,type RemoteStudioSession,type StudioEditor} from '@rynvek/studio';
type Work=(f:(s:AbortSignal,live:()=>boolean)=>Promise<void>)=>Promise<void>;
export function StudioCollaborationPanel({client,session,busy,work,automatic,onAutomatic,onSync}:{client:StudioClient;session:RemoteStudioSession;busy:boolean;work:Work;automatic:boolean;onAutomatic:(v:boolean)=>void;onSync:()=>void}){
 const[grants,setGrants]=useState<StudioEditor[]>([]),[pid,setPid]=useState(''),[consent,setConsent]=useState(false);
 const owner=session.view.collaboration.role==='owner',world=session.view.world_id;
 const refresh=()=>void work(async(s,live)=>{const r=await client.editors(world,s);if(live())setGrants(r)});
 const change=(principal:string,expected:string|null,active:boolean)=>void work(async(s,live)=>{await client.changeEditor(world,principal,expected,active,s);const r=await client.editors(world,s);if(live()){setGrants(r);setPid('');setConsent(false);}});
 return <Panel title="私有Studio协作 / 原日志同步"><p>正式身份 {session.view.collaboration.principal_id} · {owner?'所有者':'已授权创作者'}。清单为准入事实，不是在线人数；不发送站外邀请。</p><label><input aria-label="自动同步Studio" type="checkbox" checked={automatic} disabled={busy} onChange={e=>onAutomatic(e.target.checked)}/>每秒消费已提交日志</label><Button disabled={busy} onClick={onSync}>同步原权威日志</Button><p>不上传离线写队列。并发过期编辑明确冲突，重新连接后再编辑；断网/撤权清私有会话。重新连接读取当前基线且清撤销栈；同会话同步保留本人选择性撤销，不覆盖他人后写。</p>{owner?<><p>以下授权允许读取该世界全部Studio地形、基因、规则源码和参数并编辑；不授予Arena控制、完整归档/快照或其它世界。最多16个有效编辑者。</p><Button disabled={busy} onClick={refresh}>刷新真实创作准入</Button><label>正式主体ID<input aria-label="Studio编辑者主体ID" disabled={busy} value={pid} onChange={e=>setPid(e.target.value)}/></label><label><input aria-label="确认Studio源码编辑授权" type="checkbox" checked={consent} disabled={busy} onChange={e=>setConsent(e.target.checked)}/>明确授权此主体读取源码并编辑此私有世界</label><Button disabled={busy||!consent||!/^\d+$/.test(pid)} onClick={()=>change(pid,grants.find(g=>g.principal_id===pid)?.grant_id??null,true)}>授予或重授Studio编辑</Button><ul aria-label="真实Studio准入">{grants.map(g=><li key={g.principal_id}>{g.principal_id} · {g.active?'有效':'已撤销'} · {g.grant_id}<Button disabled={busy||!g.active} onClick={()=>change(g.principal_id,g.grant_id,false)}>撤销编辑 {g.principal_id}</Button></li>)}</ul></>:null}</Panel>;
}
