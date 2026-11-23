// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
/** 同原权限Actor的私有Tier0共同安装客户端；不复制RYPK/AST/IR解析器，不执行作者代码。 */
import { agentDigest, type PrivateAgentDependency } from "./private-agent.js";
export type ExactRuleRef = PrivateAgentDependency;
export interface RuleReleaseManifest {
    format: "rynvek-private-rule-release/1";
    owner_id: string;
    package_id: string;
    version: string;
    artifact_cid: string;
    dependencies: ExactRuleRef[];
}
export interface SignedRuleRelease {
    manifest: RuleReleaseManifest;
    publisher_key: string;
    signature: string;
}
export interface RuleInstallRequest {
    root: ExactRuleRef;
    modules: Record<string, string>;
}
export interface GeneralFrame {
    world: string;
    hostFence: number;
    controlFence: number;
    tick: number;
    idempotencyKey: string;
}
export type RuleRequest = (path: string, method?: string, body?: unknown, signal?: AbortSignal) => Promise<Response>;
const utf8 = new TextEncoder(), MAX = 1048576;
const hex = (b: Uint8Array) => Array.from(b, x => x.toString(16).padStart(2, "0")).join("");
function unhex(s: string, n: number): Uint8Array { if (typeof s !== "string" || !new RegExp(`^[0-9a-f]{${2 * n}}$`).test(s))
    throw new Error("摘要/签名格式不符"); return Uint8Array.from(s.match(/../g)!, x => parseInt(x, 16)); }
function own(s: string): void { if (typeof s !== "string" || !(/^[1-9][0-9]*$/).test(s) || BigInt(s) > 0xffffffffffffffffn)
    throw new Error("owner必须为规范u64字符串"); }
function safe(n: number): number { if (!Number.isSafeInteger(n) || n < 0)
    throw new Error("线上整数超出JS精确范围，拒绝截断"); return n; }
function exactKeys(v: object, keys: string[]): void { if (!v || typeof v !== "object" || Array.isArray(v) || Object.keys(v).sort().join(",") !== keys.sort().join(","))
    throw new Error("签名字段集合不符"); }
function ref(r: ExactRuleRef): void {
    exactKeys(r, ["id", "version", "blake3_256"]);
    if (typeof r.id !== "string" || r.id.length > 128 || !/^([a-z0-9]+(?:-[a-z0-9]+)*\.)+[a-z0-9]+(?:-[a-z0-9]+)*$/.test(r.id) || typeof r.version !== "string" || r.version.length > 32 || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[A-Za-z0-9.-]+)?$/.test(r.version))
        throw new Error("精确包身份非法");
    unhex(r.blake3_256, 32);
}
function identity(m: RuleReleaseManifest): ExactRuleRef { return { id: m.package_id, version: m.version, blake3_256: m.artifact_cid }; }
function manifest(m: RuleReleaseManifest): void {
    exactKeys(m, ["format", "owner_id", "package_id", "version", "artifact_cid", "dependencies"]);
    own(m.owner_id);
    ref(identity(m));
    if (m.format !== "rynvek-private-rule-release/1" || !Array.isArray(m.dependencies) || m.dependencies.length > 8)
        throw new Error("规则发布剖面不符");
    let previous = "";
    for (const d of m.dependencies) {
        ref(d);
        if (d.id <= previous || d.id === m.package_id)
            throw new Error("精确锁必须严格排序、唯一、非自引用");
        previous = d.id;
    }
}
export function privateRuleSigningMessage(m: RuleReleaseManifest, bytes: Uint8Array): Uint8Array {
    manifest(m);
    if (!(bytes instanceof Uint8Array) || bytes.length > MAX || agentDigest(bytes) !== m.artifact_cid)
        throw new Error("制品摘要/大小不符");
    // 固定ASCII键序，与Rust原Hub消息后缀完全相同；不签作者自报TS等价证明。
    const canonical = JSON.stringify({ artifact_cid: m.artifact_cid, dependencies: m.dependencies.map(d => ({ blake3_256: d.blake3_256, id: d.id, version: d.version })), format: m.format, owner_id: m.owner_id, package_id: m.package_id, version: m.version });
    const prefix = utf8.encode(`rynvek-package/1\0${m.package_id}\0${m.version}\0`), suffix = utf8.encode(`\0rynvek-private-rule-release/1\0${canonical}`), out = new Uint8Array(prefix.length + 32 + suffix.length);
    out.set(prefix);
    out.set(unhex(m.artifact_cid, 32), prefix.length);
    out.set(suffix, prefix.length + 32);
    return out;
}
export async function verifyPrivateRule(ownerId: string, trustedKey: string, release: SignedRuleRelease, bytes: Uint8Array): Promise<void> {
    own(ownerId);
    exactKeys(release, ["manifest", "publisher_key", "signature"]);
    manifest(release.manifest);
    unhex(trustedKey, 32);
    if (release.manifest.owner_id !== ownerId || release.publisher_key !== trustedKey)
        throw new Error("发布身份不等于既有可信登记公钥");
    const key = await crypto.subtle.importKey("raw", unhex(trustedKey, 32).buffer as ArrayBuffer, "Ed25519", false, ["verify"]);
    if (!await crypto.subtle.verify("Ed25519", key, unhex(release.signature, 64).buffer as ArrayBuffer, privateRuleSigningMessage(release.manifest, bytes).buffer as ArrayBuffer))
        throw new Error("规则Ed25519验签失败");
}
async function bounded(response: Response, max: number): Promise<Uint8Array> {
    if (!response.ok)
        throw new Error(`私有规则HTTP ${response.status}，未确认执行`);
    const reader = response.body?.getReader();
    if (!reader)
        throw new Error("缺少响应正文");
    let size = 0;
    const chunks: Uint8Array[] = [];
    for (;;) {
        const { done, value } = await reader.read();
        if (done)
            break;
        size += value.length;
        if (size > max) {
            await reader.cancel();
            throw new Error("私有规则响应超限");
        }
        chunks.push(value);
    }
    const out = new Uint8Array(size);
    let i = 0;
    for (const c of chunks) {
        out.set(c, i);
        i += c.length;
    }
    return out;
}
/** 同一私有控制链响应解码，限制尺寸/状态，不新增第二套传输。 */
export async function readRuleJson(response:Response):Promise<any>{return JSON.parse(new TextDecoder("utf-8",{fatal:true}).decode(await bounded(response,524288)));}
/** owner与目的隔离协作共用原CommandEnvelope编码。 */
export function generalCommandInput(principalId:string,f:GeneralFrame):any {
    own(principalId);
    if (!(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/).test(f.idempotencyKey)) throw new Error("须UUIDv7幂等键");
    return {host_fence:safe(f.hostFence),control_fence:safe(f.controlFence),envelope:{idempotency_key:f.idempotencyKey,principal_id:principalId,issued_at_tick:safe(f.tick),target_tick:safe(f.tick+1),payload:[]}};
}
export class PrivateRuleClient {
    constructor(readonly ownerId: string, readonly trustedKey: string, private readonly request: RuleRequest) { own(ownerId); unhex(trustedKey, 32); }
    private path(r: ExactRuleRef): string { ref(r); return `/api/v1/private-hub/rules/${r.id}/${r.version}`; }
    private async json(path: string, method = "GET", body?: unknown, signal?: AbortSignal): Promise<any> { return readRuleJson(await this.request(path, method, body, signal)); }
    private check(v: any, allowRevoked = false): SignedRuleRelease {
        const r = v?.release as SignedRuleRelease;
        manifest(r?.manifest);
        exactKeys(r, ["manifest", "publisher_key", "signature"]);
        unhex(r.publisher_key, 32);
        unhex(r.signature, 64);
        if (v.visibility !== "Private" || typeof v.revoked !== "boolean" || (!allowRevoked && v.revoked) || r.manifest.owner_id !== this.ownerId || r.publisher_key !== this.trustedKey)
            throw new Error("私有规则归属/登记钥/撤销不符");
        return r;
    }
    async list(signal?: AbortSignal): Promise<SignedRuleRelease[]> { const v = await this.json("/api/v1/private-hub/rules", "GET", undefined, signal); if (v.visibility !== "Private" || !Array.isArray(v.packages) || v.packages.length > 256)
        throw new Error("发现响应非法"); return v.packages.map((m: unknown) => this.check(m, true)); }
    async publish(bytes: Uint8Array, m: RuleReleaseManifest, sign: (message: Uint8Array) => Promise<Uint8Array>, signal?: AbortSignal): Promise<SignedRuleRelease> {
        if (m.owner_id !== this.ownerId)
            throw new Error("发布owner不符");
        const signature = await sign(privateRuleSigningMessage(m, bytes));
        if (signature.length !== 64)
            throw new Error("须Ed25519签名");
        let raw = "";
        for (let i = 0; i < bytes.length; i += 8192)
            raw += String.fromCharCode(...bytes.subarray(i, i + 8192));
        const r = this.check(await this.json(this.path(identity(m)), "PUT", { manifest: m, artifact_b64: btoa(raw), signature: hex(signature) }, signal));
        if (JSON.stringify(r.manifest) !== JSON.stringify(m)) {
            // 响应JSON键序不是签名含义；对规范签名消息逐字节比较。
            if (hex(privateRuleSigningMessage(r.manifest, bytes)) !== hex(privateRuleSigningMessage(m, bytes)))
                throw new Error("发布清单回执不符");
        }
        await verifyPrivateRule(this.ownerId, this.trustedKey, r, bytes);
        return r;
    }
    async revoke(pin: ExactRuleRef, signal?: AbortSignal): Promise<void> { const v = await this.json(this.path(pin), "DELETE", undefined, signal); if (v.revoked !== true || v.package_id !== pin.id || v.version !== pin.version)
        throw new Error("撤销回执不符"); }
    async download(root: ExactRuleRef, signal?: AbortSignal): Promise<Array<{
        release: SignedRuleRelease;
        bytes: Uint8Array;
    }>> {
        ref(root);
        const seen = new Map<string, string>(), gray = new Set<string>(), done = new Set<string>(), out: Array<{
            release: SignedRuleRelease;
            bytes: Uint8Array;
        }> = [];
        let size = 0;
        const visit = async (pin: ExactRuleRef, depth: number): Promise<void> => {
            ref(pin);
            const lock = `${pin.version}:${pin.blake3_256}`;
            if (depth > 8 || (seen.has(pin.id) && seen.get(pin.id) !== lock) || gray.has(pin.id))
                throw new Error("精确依赖冲突/环/深度超限");
            if (done.has(pin.id))
                return;
            seen.set(pin.id, lock);
            if (seen.size > 16)
                throw new Error("依赖闭包超过16包");
            gray.add(pin.id);
            const path = this.path(pin), r = this.check(await this.json(path, "GET", undefined, signal));
            if (r.manifest.package_id !== pin.id || r.manifest.version !== pin.version || r.manifest.artifact_cid !== pin.blake3_256)
                throw new Error("下载身份/CID不符");
            const bytes = await bounded(await this.request(`${path}/artifact`, "GET", undefined, signal), MAX - size);
            size += bytes.length;
            await verifyPrivateRule(this.ownerId, this.trustedKey, r, bytes);
            for (const d of r.manifest.dependencies)
                await visit(d, depth + 1);
            gray.delete(pin.id);
            done.add(pin.id);
            out.push({ release: r, bytes });
        };
        await visit(root, 0);
        return out;
    }
    private world(id: string): string { if (typeof id !== "string" || !(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/).test(id))
        throw new Error("世界UUID非法"); return `/api/v1/worlds/${id}/general`; }
    async observe(world: string, signal?: AbortSignal): Promise<any> { return this.json(this.world(world), "GET", undefined, signal); }
    async connect(world: string, signal?: AbortSignal): Promise<any> { return this.json(this.world(world), "POST", { action: "connect" }, signal); }
    async acquire(world: string, hostFence: number, signal?: AbortSignal): Promise<any> { return this.json(this.world(world), "POST", { action: "acquire", host_fence: safe(hostFence), level: "Sovereign" }, signal); }
    private frame(f: GeneralFrame): any { return generalCommandInput(this.ownerId,f); }

    async tick(f: GeneralFrame, signal?: AbortSignal): Promise<any> { return this.json(this.world(f.world), "POST", { action: "tick", input: this.frame(f) }, signal); }
    async install(f: GeneralFrame, plan: RuleInstallRequest, signal?: AbortSignal): Promise<any> { return this.change(f, plan, false, undefined, signal); }
    /** 显式接受单身体维护EnergyQ，仅推进一个有源维护Tick；不执行撤销图、不自动补Tick或激活新规则。 */
    async recoverNextTick(f: GeneralFrame, plan: RuleInstallRequest, acceptMaintenanceQPerBody: number, signal?: AbortSignal): Promise<any> { return this.change(f, plan, true, acceptMaintenanceQPerBody, signal); }
    private async change(f: GeneralFrame, plan: RuleInstallRequest, recover: boolean, accept?: number, signal?: AbortSignal): Promise<any> {
        plan = { root: { ...plan.root }, modules: { ...plan.modules } };
        f = { ...f };
        const input = this.frame(f);
        if (recover ? (f.tick + 1) % 4 === 0 : (f.tick + 1) % 4 !== 0)
            throw new Error("安装只在Epoch，隔离维护不能越过Epoch");
        if (recover) {
            const v = await this.observe(f.world, signal);
            if (!accept || safe(accept) !== v.recovery_maintenance_q_per_body || v.commit?.tick !== f.tick)
                throw new Error("须明确接受当前隔离维护费和时序");
        }
        const packages = await this.download(plan.root, signal);
        const modules = Object.entries(plan.modules);
        if (packages.length !== modules.length || new Set(modules.map(([, m]) => m)).size !== modules.length || packages.some(p => !Object.hasOwn(plan.modules, p.release.manifest.package_id)) || modules.some(([, m]) => typeof m !== "string" || m.length === 0 || m.length > 128))
            throw new Error("显式映射须一对一覆盖可信闭包");
        return this.json(this.world(f.world), "POST", { action: recover ? "recover" : "install", input: { ...input, [recover ? "recovery" : "install"]: plan } }, signal);
    }
}
