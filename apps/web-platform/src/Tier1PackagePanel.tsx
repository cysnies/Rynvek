// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {Button} from "@rynvek/ui";
import type {Tier1Install} from "@rynvek/studio";

interface Props {
  draft: string;
  id: string;
  built: {install:Tier1Install;artifact_blake3:string}|null;
  params: Readonly<Record<string,string>>;
  disabled: boolean;
  onDraft: (value:string)=>void;
  onId: (value:string)=>void;
  onBuild: ()=>void;
  onInstall: ()=>void;
  onDelete: ()=>void;
}

// 身份、取消控制与私有草稿由原会话持有；本面板不生成乐观世界或自动安装。
export function Tier1PackagePanel(p:Props) {
  const installed=Object.fromEntries(Object.entries(p.params).filter(([key])=>key.startsWith("rules.tier1.")));
  return <section aria-label="Tier1受控组件规则">
    <h3>TS构建产物 / 原RYPK Tier1</h3>
    <p>作者先用tsc把TS编译成单文件全局tick函数；可附原TS作来源资料，执行入口始终是包内JS，不自动相信两者等价。只对声明模板组件读写，无RTS隐藏信息/宿主I/O。构建只出包，不执行、不安装、不自动开局；显式提交后才在原Tick沙箱消费。</p>
    <fieldset disabled={p.disabled}>
      <label>Tier1规则ID<input aria-label="Tier1规则ID" value={p.id} onChange={e=>p.onId(e.target.value)}/></label>
      <textarea aria-label="Tier1构建JSON" rows={10} value={p.draft} onChange={e=>p.onDraft(e.target.value)}/>
      <Button onClick={p.onBuild}>构建受控RYPK（不执行）</Button>
      <Button disabled={!p.built} onClick={p.onInstall}>显式提交Tier1制品</Button>
      <Button onClick={p.onDelete}>删除Tier1绑定</Button>
    </fieldset>
    {p.built?<p className="rv-mono">待提交制品 {p.built.artifact_blake3}</p>:null}
    <p>与Tier0冲突写拒绝；同Tick读旧态、组件整组提交。500000共享CPU配额，每次Tier1至少扣10000轮询预算；4规则/12KiB制品/64MiB堆，失败无部分组件写。不是一般物理ECS或全World执行权限。</p>
    <pre aria-label="已提交Tier1制品">{JSON.stringify(installed,null,2)}</pre>
  </section>;
}
