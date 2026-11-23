// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
/** 目的隔离General规则驱动，不提供owner观察/源码/安装接口，不后台续租或重试。 */
import { generalCommandInput, readRuleJson, type RuleRequest, type GeneralFrame } from "./private-rule.js";
function uuid(v: string): string { if (typeof v !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(v))
    throw new Error("UUID非法"); return v; }
function safe(v: number): number { if (!Number.isSafeInteger(v) || v < 0)
    throw new Error("整数超出精确范围"); return v; }
function fields(v: any, names: string[]): void { if (!v || typeof v !== "object" || Array.isArray(v) || Object.keys(v).sort().join() !== names.sort().join())
    throw new Error("驱动回执字段不符"); }
function commit(v: any): GeneralDriverCommit { fields(v, ["tick", "revision", "state_root"]); safe(v.tick); safe(v.revision); if (v.tick !== v.revision || typeof v.state_root !== "string" || !/^[0-9a-f]{64}$/.test(v.state_root))
    throw new Error("提交时序/根不符"); return { ...v }; }
export interface GeneralDriverCommit {
    tick: number;
    revision: number;
    state_root: string;
}
export interface GeneralDriverGrant {
    principal_id: string;
    grant_id: string;
    active: boolean;
    purpose: "general_rule_drive";
    consent: "general-installed-rule-drive/1";
}
function grant(v: any): GeneralDriverGrant { fields(v, ["principal_id", "grant_id", "active", "purpose", "consent"]); uuid(v.grant_id); if (typeof v.principal_id !== "string" || !/^[1-9][0-9]*$/.test(v.principal_id) || BigInt(v.principal_id) > 0xffffffffffffffffn || typeof v.active !== "boolean" || v.purpose !== "general_rule_drive" || v.consent !== "general-installed-rule-drive/1")
    throw new Error("目的准入回执不符"); return { ...v }; }
/** 管理端由原WorldEdit/所有者裁决；只创建明确目的，不发送邀请消息。 */
export async function changeGeneralDriver(request: RuleRequest, world: string, principal: string, expected: string | null, active: boolean): Promise<GeneralDriverGrant> {
    if (typeof principal !== "string" || !/^[1-9][0-9]*$/.test(principal) || BigInt(principal) > 0xffffffffffffffffn)
        throw new Error("主体须精确u64字符串");
    if (expected !== null)
        uuid(expected);
    if (!active && expected === null)
        throw new Error("撤销必须绑定当前版本");
    const v = grant(await readRuleJson(await request(`/api/v1/worlds/${uuid(world)}/general/drivers`, "POST", { principal_id: principal, expected_grant_id: expected, active, consent: "general-installed-rule-drive/1" })));
    if (v.principal_id !== principal || v.active !== active)
        throw new Error("目的准入回执身份不符");
    return v;
}
export async function listGeneralDrivers(request: RuleRequest, world: string): Promise<GeneralDriverGrant[]> {
    const v = await readRuleJson(await request(`/api/v1/worlds/${uuid(world)}/general/drivers`, "GET"));
    fields(v, ["world_id", "grants"]);
    if (v.world_id !== world || !Array.isArray(v.grants) || v.grants.length > 64)
        throw new Error("世界/目的列表超限");
    const out = v.grants.map(grant);
    if (new Set(out.map((g: GeneralDriverGrant) => g.principal_id)).size !== out.length)
        throw new Error("重复主体目的准入");
    return out;
}
export class GeneralDriverClient {
    constructor(readonly principalId: string, readonly world: string, readonly grantId: string, private readonly request: RuleRequest) { uuid(world); uuid(grantId); if (typeof principalId !== "string" || !/^[1-9][0-9]*$/.test(principalId) || BigInt(principalId) > 0xffffffffffffffffn)
        throw new Error("主体须精确u64字符串"); }
    private async call(body: object, signal?: AbortSignal): Promise<any> { signal?.throwIfAborted(); const v = await readRuleJson(await this.request(`/api/v1/worlds/${this.world}/general/drive`, "POST", { grant_id: this.grantId, ...body }, signal)); signal?.throwIfAborted(); return v; }
    /** 只读最小帧保持unknown，显示前交原Engine.validateGeneralFrame(frame, world.replaceAll("-", ""))；不复制第二个表现解析器。 */
    async frame(signal?: AbortSignal): Promise<unknown> { const v = await this.call({ frame: true }, signal); fields(v, ["world_id", "grant_id", "frame"]); if (v.world_id !== this.world || v.grant_id !== this.grantId)
        throw new Error("准入/世界不符"); return v.frame; }
    async connect(signal?: AbortSignal): Promise<{
        hostFence: number;
        frame: unknown;
    }> { const v = await this.call({ command: { action: "connect" } }, signal); fields(v, ["host_fence", "frame"]); if (!safe(v.host_fence))
        throw new Error("宿主fence非法"); return { hostFence: v.host_fence, frame: v.frame }; }
    async acquire(hostFence: number, signal?: AbortSignal): Promise<number> { const v = await this.call({ command: { action: "acquire", host_fence: safe(hostFence), level: "Sovereign" } }, signal); if (v.host_fence !== hostFence || v.lease?.holder?.id !== this.principalId || v.lease?.holder?.kind !== "Human" || v.lease?.faction !== 0 || v.lease?.level !== "Sovereign" || !safe(v.lease?.fencing_token))
        throw new Error("控制回执不符"); return v.lease.fencing_token; }
    async tick(f: GeneralFrame, signal?: AbortSignal): Promise<{
        duplicate: boolean;
        commit: GeneralDriverCommit;
        frame?: unknown;
    }> {
        if (f.world !== this.world)
            throw new Error("世界不符");
        const input = generalCommandInput(this.principalId, { ...f });
        const v = await this.call({ command: { action: "tick", input } }, signal);
        if (typeof v.duplicate !== "boolean")
            throw new Error("提交回执不符");
        fields(v, v.duplicate ? ["duplicate", "commit"] : ["duplicate", "commit", "frame"]);
        const c = commit(v.commit);
        if (c.tick !== f.tick + 1)
            throw new Error("只接受本请求下一Tick回执");
        return { ...v, commit: c };
    }
    async release(hostFence: number, controlFence: number, signal?: AbortSignal): Promise<void> { const v = await this.call({ command: { action: "release", host_fence: safe(hostFence), control_fence: safe(controlFence) } }, signal); fields(v, ["released", "closed", "commit"]); commit(v.commit); if (v.released !== true || v.closed !== false)
        throw new Error("交还回执不符"); }
}
