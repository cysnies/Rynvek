// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {validateGeneralFrame,type GeneralRenderFrame} from "@rynvek/engine";
import { GeneralDriverClient, changeGeneralDriver, listGeneralDrivers, type GeneralDriverGrant, type RuleRequest, PrivateRuleClient, type GeneralFrame, type RuleInstallRequest, type SignedRuleRelease } from "@rynvek/agent-sdk";
export interface GeneralIdentity {
    principal_id: string;
    kind: "Human";
    ed25519_pubkey: string;
    status: "Active";
}
export interface GeneralObservation {
    world_id: string;
    commit: {
        tick: number;
        revision: number;
        state_root: string;
    };
    bodies: unknown[];
    visibility: "private_owner_only";
    execution: "server-authoritative-single-chunk";
    rule_recovery: null | {
        epoch_tick: number;
        maintenance_paid_q: number;
    };
    recovery_maintenance_q_per_body: number | null;
    rule_meter: unknown;
    life: unknown;
    components: unknown;
    installed_releases: unknown;
}
export interface GeneralWorld {
    world_id: string;
    owner_principal_id: string;
    world_profile: string;
    game_mode: string;
    title: string;
}
function integer(v: number): number { if (!Number.isSafeInteger(v) || v < 0)
    throw new Error("整数超出客户端无损范围"); return v; }
function uuid(s: string): string { if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(s))
    throw new Error("世界UUID非法"); return s; }
async function responseJson(r: Response): Promise<any> {
    if (!r.ok)
        throw new Error(`权威请求拒绝 ${r.status}；未确认提交，请刷新，不自动重试`);
    const reader = r.body?.getReader();
    if (!reader)
        throw new Error("缺少响应");
    let size = 0, text = "";
    const decoder = new TextDecoder();
    for (;;) {
        const { done, value } = await reader.read();
        if (done)
            break;
        size += value.length;
        if (size > 1048576) {
            await reader.cancel();
            throw new Error("响应超限");
        }
        text += decoder.decode(value, { stream: true });
    }
    return JSON.parse(text + decoder.decode());
}
/** 唯一幂等键，无自动重试；不是普通随机UUID伪装v7。 */
function nextKey(): string { const b = crypto.getRandomValues(new Uint8Array(16)); let t = BigInt(Date.now()); for (let i = 5; i >= 0; i--) {
    b[i] = Number(t & 255n);
    t >>= 8n;
} b[6] = 0x70 | (b[6]! & 15); b[8] = 0x80 | (b[8]! & 63); const h = Array.from(b, x => x.toString(16).padStart(2, "0")).join(""); return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`; }
/** 中止只限制后续请求；已发请求可能已提交，不能把取消说成服务器回滚。 */
export class GeneralRunInterrupted extends Error {
    constructor(readonly confirmedTicks: number, readonly requestUnconfirmed: boolean, cause: unknown) {
        super(`有界运行停止：已确认${confirmedTicks}个Tick${requestUnconfirmed ? "；末请求结果未确认，须刷新" : "；未发送后续Tick"}`, {cause});
    }
}
/** 只有服务器已确认观察才能进入UI；不把本地Worker仿真当远端根。 */
export class GeneralWorldSession {
    private token: string;
    private readonly privateListeners=new Set<()=>void>();
    private viewEpoch=0;
    onPrivateInvalidated(listener:()=>void):()=>void{this.privateListeners.add(listener);return ()=>this.privateListeners.delete(listener);}
    invalidatePrivate():void{this.viewEpoch++;this.observation=undefined;this.hostFence=undefined;this.controlFence=undefined;for(const f of this.privateListeners)f();}
    async renderFrame(signal?:AbortSignal):Promise<GeneralRenderFrame>{const world=this.world,epoch=this.viewEpoch;if(!world)throw new Error("未选择世界");const f=await responseJson(await this.raw(`/api/v1/worlds/${world}/general/frame`,"GET",undefined,signal));if(this.world!==world||epoch!==this.viewEpoch)throw new Error("世界/身份已切换");validateGeneralFrame(f,world.replaceAll("-",""));return f;}
    private readonly abort = new AbortController();
    private busy = false;
    readonly rules: PrivateRuleClient;
    readonly origin: string;
    readonly identity: GeneralIdentity;
    world: string | undefined;
    observation: GeneralObservation | undefined;
    hostFence: number | undefined;
    controlFence: number | undefined;
    private constructor(origin: string, token: string, identity: GeneralIdentity, private readonly fetcher: typeof fetch) {
        this.origin = origin;
        this.token = token;
        this.identity = Object.freeze({ ...identity });
        this.rules = new PrivateRuleClient(identity.principal_id, identity.ed25519_pubkey, (p, m, b, s) => this.raw(p, m, b, s));
    }
    static async open(origin: string, token: string, fetcher: typeof fetch = fetch, signal?: AbortSignal): Promise<GeneralWorldSession> {
        const u = new URL(origin);
        if (u.pathname !== "/" || u.search || u.hash || u.username || u.password || !(u.protocol === "https:" || u.protocol === "http:" && ["127.0.0.1", "[::1]", "localhost"].includes(u.hostname)))
            throw new Error("须可信HTTPS origin或本机开发地址");
        if (!token.trim() || /[\r\n]/.test(token))
            throw new Error("凭据格式非法");
        const r = await fetcher(`${u.origin}/api/v1/auth/session`, { headers: { Authorization: `Bearer ${token}` }, redirect: "error", cache: "no-store", credentials: "omit", signal: AbortSignal.any([AbortSignal.timeout(15000), ...(signal ? [signal] : [])]) });
        const i = await responseJson(r);
        if (!i || i.kind !== "Human" || i.status !== "Active" || typeof i.principal_id !== "string" || !/^[1-9][0-9]*$/.test(i.principal_id) || BigInt(i.principal_id) > 0xffffffffffffffffn || !/^[0-9a-f]{64}$/.test(i.ed25519_pubkey))
            throw new Error("须正式Active人类身份");
        signal?.throwIfAborted();
        return new GeneralWorldSession(u.origin, token, i, fetcher);
    }
    /** 共用正式身份/同源凭据传输，返回不暴露owner规则客户端的目的隔离适配器。 */
    static async openRuleDriver(origin:string, token:string, fetcher:typeof fetch=fetch, signal?:AbortSignal):Promise<GeneralDriverSession> {
        const auth=await GeneralWorldSession.open(origin,token,fetcher,signal);
        return new GeneralDriverSession(auth.identity,(p,m,b,s)=>auth.raw(p,m,b,s),()=>auth.destroy());
    }
    async listDrivers():Promise<GeneralDriverGrant[]>{return this.exclusive(()=>{if(!this.world)throw new Error("未选本人世界");return listGeneralDrivers((p,m,b,s)=>this.raw(p,m,b,s),this.world);});}
    async changeDriver(principal:string,expected:string|null,active:boolean):Promise<GeneralDriverGrant>{return this.exclusive(()=>{if(!this.world)throw new Error("未选本人世界");return changeGeneralDriver((p,m,b,s)=>this.raw(p,m,b,s),this.world,principal,expected,active);});}
    destroy(): void { this.abort.abort(); this.token = ""; this.clear(); }
    private clear(): void { this.world = undefined;this.invalidatePrivate(); this.observation = undefined; this.hostFence = undefined; this.controlFence = undefined; }
    private async raw(path: string, method = "GET", body?: unknown, signal?: AbortSignal): Promise<Response> {
        if (!this.token || !path.startsWith("/api/v1/") || path.includes("..") || path.includes("\\"))
            throw new Error("会话失效或路径非法");
        const bytes = body instanceof Uint8Array;
        const response = await this.fetcher(this.origin + path, { method, redirect: "error", cache: "no-store", credentials: "omit", signal: AbortSignal.any([this.abort.signal, AbortSignal.timeout(15000), ...(signal ? [signal] : [])]), headers: { Authorization: `Bearer ${this.token}`, "Content-Type": bytes ? "application/octet-stream" : "application/json" }, ...(body === undefined ? {} : { body: bytes ? body as unknown as BodyInit : JSON.stringify(body) }) });
        if (!this.token)
            throw new Error("身份已断开");
        return response;
    }
    private async json(path: string, method = "GET", body?: unknown): Promise<any> {
        return responseJson(await this.raw(path, method, body));
    }
    private async exclusive<T>(work: () => Promise<T>): Promise<T> { if (this.busy || !this.token)
        throw new Error("会话繁忙或失效"); this.busy = true; try {
        const result = await work();
        if (!this.token) {
            this.clear();
            throw new Error("会话已断开");
        }
        return result;
    }
    catch(e){this.invalidatePrivate();throw e;}
    finally {
        this.busy = false;
    } }
    private accept(v: GeneralObservation): GeneralObservation {
        if (!this.token || v?.world_id !== this.world || v.visibility !== "private_owner_only" || v.execution !== "server-authoritative-single-chunk" || !Array.isArray(v.bodies) || v.bodies.length > 4096 || !/^[0-9a-f]{64}$/.test(v.commit?.state_root))
            throw new Error("世界观察身份/根/剖面非法");
        integer(v.commit.tick);
        integer(v.commit.revision);
        if (v.commit.tick !== v.commit.revision)
            throw new Error("Tick/revision不一致");
        const numbers = (x: unknown, depth = 0): void => { if (depth > 64)
            throw new Error("观察嵌套超限"); if (typeof x === "number" && Number.isInteger(x) && !Number.isSafeInteger(x))
            throw new Error("观察整数超出JS无损范围，不显示截断状态"); if (x && typeof x === "object")
            for (const y of Object.values(x))
                numbers(y, depth + 1); };
        numbers(v);
        this.observation = v;
        return v;
    }
    async listWorlds(offset = 0): Promise<GeneralWorld[]> { return this.exclusive(async () => { const v = await this.json(`/api/v1/worlds?limit=100&offset=${integer(offset)}`); if (!Array.isArray(v.worlds) || v.worlds.length > 100)
        throw new Error("发现清单非法"); return v.worlds.filter((w: GeneralWorld) => w.owner_principal_id === this.identity.principal_id && w.world_profile === "General_Circle_v1" && w.game_mode === "General_Controlled"); }); }
    async create(bytes: Uint8Array): Promise<string> { return this.exclusive(async () => { if (!bytes.length || bytes.length > 131072)
        throw new Error("RYWL包大小须1..128KiB"); const v = await this.json("/api/v1/worlds/general", "POST", bytes.slice()); return uuid(v.world_id); }); }
    async select(id: string): Promise<GeneralObservation> { return this.exclusive(async () => { this.clear(); const world = uuid(id); const m = await this.json(`/api/v1/worlds/${world}`); if (m.world_id !== world || m.owner_principal_id !== this.identity.principal_id || m.world_profile !== "General_Circle_v1" || m.game_mode !== "General_Controlled")
        throw new Error("不是本人受控General世界"); this.world = world; try {
        return this.accept(await this.rules.observe(world, this.abort.signal));
    }
    catch (e) {
        this.clear();
        throw e;
    } }); }
    async refresh(): Promise<GeneralObservation> { return this.exclusive(async () => { if (!this.world)
        throw new Error("未选世界"); this.hostFence = undefined; this.controlFence = undefined; this.invalidatePrivate(); return this.accept(await this.rules.observe(this.world, this.abort.signal)); }); }
    async connect(): Promise<void> { return this.exclusive(async () => { if (!this.world)
        throw new Error("未选世界"); this.hostFence = undefined; this.controlFence = undefined; const v = await this.rules.connect(this.world, this.abort.signal); this.accept(v.observation); this.hostFence = integer(v.host_fence); }); }
    async acquire(): Promise<void> { return this.exclusive(async () => { if (!this.world || this.hostFence === undefined)
        throw new Error("须先明确连接宿主"); this.controlFence = undefined; const v = await this.rules.acquire(this.world, this.hostFence, this.abort.signal); if (v.host_fence !== this.hostFence || v.lease?.level !== "Sovereign")
        throw new Error("控制租约回执不符"); this.controlFence = integer(v.lease.fencing_token); }); }
    async discoverRules(): Promise<SignedRuleRelease[]> { return this.exclusive(() => this.rules.list(this.abort.signal)); }
    async verify(plan: RuleInstallRequest): Promise<string[]> { return this.exclusive(async () => { const v = await this.rules.download(plan.root, this.abort.signal); return v.map(p => p.release.manifest.package_id); }); }
    private async advanceOne(action: "tick" | "install" | "recover", plan?: RuleInstallRequest, fee?: number, signal?: AbortSignal): Promise<GeneralObservation> {
        if (!this.world || !this.observation || this.hostFence === undefined || this.controlFence === undefined)
            throw new Error("须刷新/连接并明确取得Sovereign租约");
        signal?.throwIfAborted();
        const epoch = this.viewEpoch;
        const f: GeneralFrame = { world: this.world, hostFence: this.hostFence, controlFence: this.controlFence, tick: this.observation.commit.tick, idempotencyKey: nextKey() };
        const pending = signal ? AbortSignal.any([this.abort.signal,signal]) : this.abort.signal;
        const v = action === "tick" ? await this.rules.tick(f, pending) : action === "install" && plan ? await this.rules.install(f, plan, pending) : action === "recover" && plan && fee !== undefined ? await this.rules.recoverNextTick(f, plan, fee, pending) : (() => { throw new Error("须明确新计划/维护报价"); })();
        pending.throwIfAborted();
        if (epoch !== this.viewEpoch || this.world !== f.world || v.commit?.tick !== f.tick + 1 || v.commit?.revision !== f.tick + 1 || v.observation?.commit?.tick !== f.tick + 1 || v.commit?.state_root !== v.observation?.commit?.state_root)
            throw new Error("提交回执/世界生命周期不符");
        return this.accept(v.observation);
    }
    async advance(action: "tick" | "install" | "recover", plan?: RuleInstallRequest, fee?: number): Promise<GeneralObservation> {
        return this.exclusive(() => this.advanceOne(action,plan,fee));
    }
    /** 明确有界的客户端调度，复用同一个Actor单Tick实付；不是独立服务端自治作业。 */
    async runTicks(count: number, signal: AbortSignal, onCommitted: (confirmed: number, observation: GeneralObservation) => void = () => {}): Promise<number> {
        // 128仅是本产品单次操作上限，不是CPU/Gas或生命能量配额。
        if (!Number.isSafeInteger(count) || count < 1 || count > 128) throw new Error("有界运行数量须1..128");
        return this.exclusive(async () => {
            let confirmed=0, pending=false;
            const stop=new AbortController();
            const unsubscribe=this.onPrivateInvalidated(()=>stop.abort());
            const active=AbortSignal.any([signal,stop.signal,this.abort.signal]);
            try {
                if (!this.observation || this.observation.rule_recovery || this.controlFence === undefined || this.hostFence === undefined) throw new Error("须明确控制且无隔离维护");
                const world=this.world,host=this.hostFence,control=this.controlFence,epoch=this.viewEpoch;
                while(confirmed<count){
                    active.throwIfAborted();
                    if(this.world!==world||this.hostFence!==host||this.controlFence!==control||this.viewEpoch!==epoch)throw new Error("有界运行世界/双fence已切换");
                    pending=true;
                    const v=await this.advanceOne("tick",undefined,undefined,active);
                    pending=false;
                    confirmed++;
                    onCommitted(confirmed,v);
                }
                return confirmed;
            } catch(cause) {throw new GeneralRunInterrupted(confirmed,pending,cause);}
            finally {unsubscribe();}
        });
    }

}

/** 单Tick目的协作会话。无owner观察、包访问、安装/维护、后台循环或耐久自治宣称。 */
export class GeneralDriverSession {
    readonly identity: GeneralIdentity;
    world: string | undefined;
    grantId: string | undefined;
    frame: GeneralRenderFrame | undefined;
    hostFence: number | undefined;
    controlFence: number | undefined;
    private sdk: GeneralDriverClient | undefined;
    private epoch = 0;
    private busy = false;
    private alive = true;
    private readonly listeners = new Set<() => void>();
    private pending = new AbortController();
    private freshness: ReturnType<typeof setTimeout> | undefined;
    private confirmedFresh(): void {
        clearTimeout(this.freshness);
        this.freshness = setTimeout(() => { if (this.alive)
            this.invalidatePrivate(); }, 12000);
    }
    constructor(identity: GeneralIdentity, private readonly request: RuleRequest, private readonly close: () => void) { this.identity = Object.freeze({ ...identity }); }
    onPrivateInvalidated(listener: () => void): () => void { this.listeners.add(listener); return () => this.listeners.delete(listener); }
    invalidatePrivate(): void {
        clearTimeout(this.freshness);
        this.freshness = undefined;
        this.epoch++;
        this.pending.abort();
        this.pending = new AbortController();
        this.frame = undefined;
        this.hostFence = undefined;
        this.controlFence = undefined;
        for (const f of this.listeners)
            f();
    }
    destroy(): void {
        if (!this.alive)
            return;
        this.alive = false;
        this.invalidatePrivate();
        this.pending.abort();
        this.sdk = undefined;
        this.world = undefined;
        this.grantId = undefined;
        this.close();
    }
    private accept(value: unknown): GeneralRenderFrame {
        if (!this.world)
            throw new Error("未绑定协作世界");
        validateGeneralFrame(value as GeneralRenderFrame, this.world.replaceAll("-", ""));
        // 不让调用方持有并改写传入JSON后影响当前Tick基线。
        const f = structuredClone(value as GeneralRenderFrame);
        this.frame = f;
        this.confirmedFresh();
        return structuredClone(f);
    }
    private async exclusive<T>(work: (sdk: GeneralDriverClient, signal: AbortSignal) => Promise<T>): Promise<T> {
        if (!this.alive || this.busy || !this.sdk)
            throw new Error("协作会话繁忙/失效或尚未绑定");
        this.busy = true;
        const epoch = this.epoch, signal = this.pending.signal;
        try {
            const result = await work(this.sdk, signal);
            signal.throwIfAborted();
            if (!this.alive || epoch !== this.epoch)
                throw new Error("协作会话已失效");
            return result;
        }
        catch (e) {
            this.invalidatePrivate();
            throw e;
        }
        finally {
            this.busy = false;
        }
    }
    async join(world: string, grant: string): Promise<void> {
        if (this.busy || !this.alive)
            throw new Error("协作会话繁忙或失效");
        this.invalidatePrivate();
        this.world = undefined;
        this.grantId = undefined;
        this.sdk = undefined;
        // SDK先核资源标识；只走目的drive，不试探owner元数据/完整观察。
        const sdk = new GeneralDriverClient(this.identity.principal_id, uuid(world), grant, this.request);
        this.world = world;
        this.grantId = grant;
        this.sdk = sdk;
        await this.refresh();
    }
    async refresh(): Promise<void> {
        if (this.busy)
            throw new Error("协作会话繁忙");
        this.invalidatePrivate();
        return this.exclusive(async (sdk, signal) => { const v = await sdk.frame(signal); signal.throwIfAborted(); this.accept(v); });
    }
    /** 原只读Worker轮询可独立读取，不与明确Tick争夺操作锁；不修改控制或当前推进基线。 */
    async renderFrame(signal?: AbortSignal): Promise<GeneralRenderFrame> {
        const sdk = this.sdk, world = this.world, epoch = this.epoch;
        if (!this.alive || !sdk || !world)
            throw new Error("协作会话失效");
        const combined = AbortSignal.any([this.pending.signal, ...(signal ? [signal] : [])]);
        try {
            const f = await sdk.frame(combined);
            combined.throwIfAborted();
            if (epoch !== this.epoch || !this.alive || this.world !== world)
                throw new Error("私有观察已失效");
            validateGeneralFrame(f as GeneralRenderFrame, world.replaceAll("-", ""));
            this.confirmedFresh();
            return structuredClone(f as GeneralRenderFrame);
        }
        catch (e) {
            if (epoch === this.epoch)
                this.invalidatePrivate();
            throw e;
        }
    }
    async connect(): Promise<void> { return this.exclusive(async (sdk, signal) => { this.hostFence = undefined; this.controlFence = undefined; const c = await sdk.connect(signal); signal.throwIfAborted(); this.accept(c.frame); this.hostFence = integer(c.hostFence); }); }
    async acquire(): Promise<void> {
        return this.exclusive(async (sdk, signal) => {
            if (this.hostFence === undefined)
                throw new Error("须先明确连接");
            this.controlFence = undefined;
            const l = await sdk.acquire(this.hostFence, signal);
            signal.throwIfAborted();
            this.controlFence = integer(l);
        });
    }
    async advance(): Promise<void> {
        return this.exclusive(async (sdk, signal) => {
            if (!this.world || !this.frame || this.hostFence === undefined || this.controlFence === undefined)
                throw new Error("须读取、连接并明确取得控制");
            const tick = this.frame.tick, r = await sdk.tick({ world: this.world, hostFence: this.hostFence, controlFence: this.controlFence, tick, idempotencyKey: nextKey() }, signal);
            signal.throwIfAborted();
            if (r.duplicate || r.frame === undefined)
                throw new Error("本次新请求未返回对应权威帧，不拿当前帧冒充旧回执");
            validateGeneralFrame(r.frame as GeneralRenderFrame, this.world.replaceAll("-", ""));
            const f = r.frame as GeneralRenderFrame;
            if (f.tick !== r.commit.tick || f.revision !== r.commit.revision || f.stateRoot !== r.commit.state_root)
                throw new Error("提交与帧不一致");
            this.accept(f);
        });
    }
    async release(): Promise<void> {
        return this.exclusive(async (sdk, signal) => {
            if (this.hostFence === undefined || this.controlFence === undefined)
                throw new Error("尚无控制");
            await sdk.release(this.hostFence, this.controlFence, signal);
            signal.throwIfAborted();
            this.hostFence = undefined;
            this.controlFence = undefined;
        });
    }
}
