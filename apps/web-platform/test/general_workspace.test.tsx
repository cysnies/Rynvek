// @vitest-environment jsdom
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { afterEach, expect, it, vi } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import { GeneralWorkspace } from "../src/GeneralWorkspace.js";
import { GeneralRunInterrupted, GeneralWorldSession } from "../src/general-remote.js";
const world = "019aabbc-dddd-7eee-8fff-000000000001", key = "01".repeat(32);
const identity = { principal_id: "18446744073709551615", kind: "Human", status: "Active", ed25519_pubkey: key };
const observation = { world_id: world, visibility: "private_owner_only", execution: "server-authoritative-single-chunk", commit: { tick: 0, revision: 0, state_root: "00".repeat(32) }, bodies: [], rule_recovery: null, recovery_maintenance_q_per_body: 4 };
function response(v: unknown) { return new Response(JSON.stringify(v)); }
function transport() { return vi.fn<typeof fetch>(async (input, init) => { const url = String(input); if (url.endsWith("auth/session"))
    return response(identity); if (url.endsWith(`/worlds/${world}`))
    return response({ world_id: world, owner_principal_id: identity.principal_id, world_profile: "General_Circle_v1", game_mode: "General_Controlled" }); if (url.endsWith("/general") && init?.method !== "POST")
    return response(observation); const b = JSON.parse(String(init?.body)); if (b.action === "connect")
    return response({ host_fence: 2, observation }); if (b.action === "acquire")
    return response({ host_fence: 2, lease: { level: "Sovereign", fencing_token: 3 } }); return response({}); }); }
afterEach(() => cleanup());
it("正式身份从服务端读回且大u64不截断；不发送令牌到重定向/不安全origin", async () => { const f = transport(); const c = await GeneralWorldSession.open("http://127.0.0.1:3000", "test-capability", f); expect(c.identity.principal_id).toBe(identity.principal_id); expect(f.mock.calls[0]?.[1]).toMatchObject({ redirect: "error", cache: "no-store", credentials: "omit" }); await expect(GeneralWorldSession.open("http://external.invalid", "x", f)).rejects.toThrow(); c.destroy(); await expect(c.listWorlds()).rejects.toThrow(); });
it("只读取不偷取租约；明确连接/取得控制后才发送一个Tick；失败清双fence和观察", async () => { const f = transport(), c = await GeneralWorldSession.open("http://127.0.0.1:3000", "test", f); await c.select(world); expect(c.hostFence).toBeUndefined(); await expect(c.advance("tick")).rejects.toThrow(); await c.connect(); await c.acquire(); expect(c.controlFence).toBe(3); await expect(c.advance("tick")).rejects.toThrow(); expect(c.observation).toBeUndefined(); expect(c.hostFence).toBeUndefined(); const body = JSON.parse(String(f.mock.calls.at(-1)?.[1]?.body)); expect(body.input.envelope).toMatchObject({ principal_id: identity.principal_id, target_tick: 1, payload: [] }); expect(body.input.envelope.idempotency_key).toMatch(/-7[0-9a-f]{3}-[89ab]/); c.destroy(); });
it("串行保护与登出后晚回执不能重新填入状态", async () => { const f = transport(), c = await GeneralWorldSession.open("http://127.0.0.1:3000", "test", f); let finish!: (r: Response) => void; f.mockImplementationOnce(() => new Promise(r => { finish = r; })); const reading = c.select(world); await expect(c.listWorlds()).rejects.toThrow(); c.destroy(); finish(response({ world_id: world })); await expect(reading).rejects.toThrow(); expect(c.observation).toBeUndefined(); });
it("错世界/不安全Tick/非人类身份拒绝，不把任意JSON当成功", async () => { const f = transport(); f.mockImplementationOnce(async () => response({ ...identity, kind: "Agent" })); await expect(GeneralWorldSession.open("http://127.0.0.1:3000", "test", f)).rejects.toThrow(); const c = await GeneralWorldSession.open("http://127.0.0.1:3000", "test", f); f.mockImplementationOnce(async () => response({ world_id: world, owner_principal_id: "2" })); await expect(c.select(world)).rejects.toThrow(); expect(c.world).toBeUndefined(); c.destroy(); });
it("DOM只在身份确认后显示操作，读取/连接/推进分开且断开清空", async () => { const f = transport(); render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>); expect(screen.queryByText("明确推进一个Tick")).toBeNull(); fireEvent.change(screen.getByLabelText("General能力令牌"), { target: { value: "test" } }); fireEvent.click(screen.getByText("验证既有身份")); await screen.findByText("读取所选世界"); expect((screen.getByText("明确推进一个Tick") as HTMLButtonElement).disabled).toBe(true); fireEvent.change(screen.getByLabelText("General世界ID"), { target: { value: world } }); fireEvent.click(screen.getByText("读取所选世界")); await screen.findByText(/Tick 0 · revision 0/); expect(f.mock.calls.filter(([, init]) => String(init?.body).includes('"action":"connect"'))).toHaveLength(0); fireEvent.click(screen.getByText("断开并清空")); await waitFor(() => expect(screen.queryByText(/Tick 0 · revision 0/)).toBeNull()); expect(screen.queryByText("读取所选世界")).toBeNull(); });
it("身份验证未完成时退出，不被晚来的成功回执复活", async () => { let finish!: (r: Response) => void; const f = vi.fn<typeof fetch>(() => new Promise(r => finish = r)); render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>); fireEvent.change(screen.getByLabelText("General能力令牌"), { target: { value: "test" } }); fireEvent.click(screen.getByText("验证既有身份")); fireEvent.click(screen.getByText("断开并清空")); finish(response(identity)); await waitFor(() => expect(screen.getByText(/身份已断开/)).toBeTruthy()); expect(screen.queryByText("读取所选世界")).toBeNull(); });
it("观察包含超JS精确范围的整数时拒绝展示而非四舍五入，响应和创世包也有硬界", async () => { const f = transport(), c = await GeneralWorldSession.open("http://127.0.0.1:3000", "test", f); await expect(c.create(new Uint8Array(131073))).rejects.toThrow(); f.mockImplementationOnce(async () => response({ world_id: world, owner_principal_id: identity.principal_id, world_profile: "General_Circle_v1", game_mode: "General_Controlled" })); f.mockImplementationOnce(async () => response({ ...observation, bodies: [{ serial: 1, body: { position_q32: [Number.MAX_SAFE_INTEGER + 1, 0] } }] })); await expect(c.select(world)).rejects.toThrow(); expect(c.observation).toBeUndefined(); c.destroy(); });

function ticking() {
    const f=transport(), original=f.getMockImplementation()!;let tick=0;const requests:any[]=[];
    f.mockImplementation(async(input,init)=>{
        const body=init?.body?JSON.parse(String(init.body)):undefined;
        if(body?.action!=="tick")return original(input,init);
        requests.push(body);tick++;
        const next={...observation,commit:{tick,revision:tick,state_root:tick.toString(16).padStart(64,"0")}};
        return response({commit:next.commit,observation:next});
    });
    return {f,requests};
}
async function controlled(f:typeof fetch){const c=await GeneralWorldSession.open("http://127.0.0.1:3000","test",f);await c.select(world);await c.connect();await c.acquire();return c;}
it("有界运行逐Tick串行确认与唯一幂等键，不自动续租安装或本地推进",async()=>{
    const {f,requests}=ticking(),c=await controlled(f),progress:number[]=[];
    await expect(c.runTicks(129,new AbortController().signal)).rejects.toThrow();
    expect(await c.runTicks(3,new AbortController().signal,(n)=>progress.push(n))).toBe(3);
    expect(progress).toEqual([1,2,3]);expect(c.observation?.commit.tick).toBe(3);
    expect(requests.map(b=>b.input.envelope.target_tick)).toEqual([1,2,3]);
    expect(new Set(requests.map(b=>b.input.envelope.idempotency_key)).size).toBe(3);
    expect(f.mock.calls.filter(([,o])=>String(o?.body).includes('"action":"acquire"'))).toHaveLength(1);
    f.mockImplementationOnce(async()=>response({commit:{tick:4,revision:4,state_root:"04".repeat(32)},observation:{...observation,commit:{tick:5,revision:5,state_root:"04".repeat(32)}}}));
    await expect(c.runTicks(1,new AbortController().signal)).rejects.toThrow(/未确认/);
    expect(c.observation).toBeUndefined();
    c.destroy();
});
it("有界运行确认后主动停止只停后续，清双fence且不回滚已确认Tick",async()=>{
    const {f,requests}=ticking(),c=await controlled(f),a=new AbortController();
    const error=await c.runTicks(3,a.signal,()=>a.abort()).catch(e=>e);
    expect(error).toBeInstanceOf(GeneralRunInterrupted);expect(error.confirmedTicks).toBe(1);expect(error.requestUnconfirmed).toBe(false);
    expect(requests).toHaveLength(1);expect(c.observation).toBeUndefined();expect(c.controlFence).toBeUndefined();c.destroy();
});
it("未确认请求不重试；私有失效中止运行，晚成功不复活观察且同会话拒并发操作",async()=>{
    const {f}=ticking(),c=await controlled(f);let finish!:(r:Response)=>void;
    f.mockImplementationOnce(()=>new Promise(r=>finish=r));
    const run=c.runTicks(3,new AbortController().signal).catch(e=>e);
    await expect(c.refresh()).rejects.toThrow(/繁忙/);c.invalidatePrivate();
    finish(response({commit:{tick:1,state_root:"01".repeat(32)},observation:{...observation,commit:{tick:1,revision:1,state_root:"01".repeat(32)}}}));
    const error=await run;expect(error.confirmedTicks).toBe(0);expect(error.requestUnconfirmed).toBe(true);expect(c.observation).toBeUndefined();expect(c.hostFence).toBeUndefined();c.destroy();
});
it("DOM有界运行开始后停止按钮仍可用；终止不显示虚假批次成功",async()=>{
    const {f}=ticking();render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>);
    fireEvent.change(screen.getByLabelText("General能力令牌"),{target:{value:"test"}});fireEvent.click(screen.getByText("验证既有身份"));await screen.findByText("读取所选世界");
    fireEvent.change(screen.getByLabelText("General世界ID"),{target:{value:world}});fireEvent.click(screen.getByText("读取所选世界"));await screen.findByText(/Tick 0 · revision 0/);
    fireEvent.click(screen.getByText("明确连接宿主"));await screen.findByText(/宿主已确认/);fireEvent.click(screen.getByText("取得Sovereign"));await screen.findByText(/已取得原Sovereign控制租约/);
    let finish!:(r:Response)=>void;f.mockImplementationOnce(()=>new Promise(r=>finish=r));
    fireEvent.click(screen.getByText("明确开始有界实付运行"));const stop=await screen.findByText("停止后续Tick");expect((stop as HTMLButtonElement).disabled).toBe(false);fireEvent.click(stop);
    finish(response({}));await screen.findByText(/有界运行停止：已确认0个Tick；末请求结果未确认/);
    expect(screen.queryByText(/Tick 0 · revision 0/)).toBeNull();
});

const runId="019aabbc-dddd-7eee-8fff-000000000002",submission="019aabbc-dddd-7eee-8fff-000000000003",opId="019aabbc-dddd-7eee-8fff-000000000004";
function durable(){const f=transport(),old=f.getMockImplementation()!,spec={submission_id:submission,expected_revision:0,expected_root:observation.commit.state_root,ticks:16,wall_seconds:120,interval_ms:500,max_energy_q:"1000000",consent:"run_installed_rules_with_bounded_budget" as const};let state:any={storage_backend:"memory",id:runId,world_id:world,owner:identity.principal_id,spec,base_tick:0,last_root:spec.expected_root,done:0,energy_used_q:0,status:"queued",generation:0,fence:0,attempt:0,node:null,lease_until_s:0,deadline_s:1800000000,error:null,operations:{}};
 f.mockImplementation(async(url,init)=>{const b=init?.body?JSON.parse(String(init.body)):undefined;if(b?.action==="start_run"){state={...state,spec:b.spec};return response(state);}if(String(url).includes("/general/runs")){if(init?.method==="POST"){state={...state,generation:state.generation+1,status:b.action==="pause"?"paused":b.action==="resume"?"queued":"cancelled",operations:{...state.operations,[b.operation_id]:b}};}return response(state);}return old(url,init);});return {f,spec};}
it("服务器耐久启动清原双fence，只读状态不重新认领或客户端Tick",async()=>{const {f,spec}=durable(),c=await controlled(f);const run=await c.startDurable(spec);expect(c.durableReceiptCurrent(run)).toBe(true);expect(c.observation).toBeUndefined();expect(c.controlFence).toBeUndefined();await c.durableStatus(runId);const paused=await c.durableControl(runId,{operation_id:opId,generation:0,action:"pause"});expect(paused.status).toBe("paused");expect(c.durableReceiptCurrent(run)).toBe(false);expect(f.mock.calls.filter(([,o])=>String(o?.body).includes('"action":"connect"'))).toHaveLength(1);expect(f.mock.calls.filter(([,o])=>String(o?.body).includes('"action":"tick"'))).toHaveLength(0);c.destroy();});
it("私有失效或断开后的迟到耐久回执拒绝且无法复活",async()=>{for(const destroy of [false,true]){const {f,spec}=durable(),c=await controlled(f);let finish!:(r:Response)=>void;const old=f.getMockImplementation()!;let result!:Response;f.mockImplementationOnce(async(u,o)=>{result=await old(u,o);return new Promise(r=>finish=r);});const pending=c.startDurable(spec);await waitFor(()=>expect(finish).toBeTypeOf("function"));if(destroy)c.destroy();else c.invalidatePrivate();finish(result);await expect(pending).rejects.toThrow();expect(c.observation).toBeUndefined();expect(c.hostFence).toBeUndefined();c.destroy();}});
it("DOM明确预算启动/原键对账/暂停恢复取消；隐藏清私有但不发cancel",async()=>{const {f}=durable();render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>);fireEvent.change(screen.getByLabelText("General能力令牌"),{target:{value:"test"}});fireEvent.click(screen.getByText("验证既有身份"));await screen.findByText("读取所选世界");fireEvent.change(screen.getByLabelText("General世界ID"),{target:{value:world}});fireEvent.click(screen.getByText("读取所选世界"));await screen.findByText(/Tick 0 · revision 0/);fireEvent.click(screen.getByText("明确连接宿主"));await screen.findByText(/宿主已确认/);fireEvent.click(screen.getByText("取得Sovereign"));await screen.findByText(/已取得原Sovereign控制租约/);
 const start=screen.getByText("明确启动服务器耐久运行") as HTMLButtonElement;expect(start.disabled).toBe(true);fireEvent.change(screen.getByLabelText("耐久提交键"),{target:{value:submission}});fireEvent.click(screen.getByLabelText("明确同意上述规则预算与服务器后台执行"));fireEvent.click(start);await screen.findByText(/服务器运行状态 queued/);expect(screen.queryByText(/Tick 0 · revision 0/)).toBeNull();
 for(const [label,status] of [["服务器暂停","paused"],["明确恢复原预算","queued"],["服务器取消意图","cancelled"]]){fireEvent.click(screen.getByText("明确准备新控制键"));fireEvent.click(screen.getByText(label!));await screen.findByText(new RegExp(`服务器已确认.*状态${status}`));}
 const before=f.mock.calls.length;window.dispatchEvent(new Event("offline"));await waitFor(()=>expect(screen.queryByText(/运行 .* · cancelled/)).toBeNull());expect(f.mock.calls.length).toBe(before);expect((screen.getByLabelText("耐久提交键") as HTMLInputElement).value).toBe(submission);
 fireEvent.click(screen.getByText("按原提交键只读对账"));await screen.findByText(/服务器运行状态 cancelled/);expect(f.mock.calls.filter(([,o])=>String(o?.body).includes('"action":"tick"'))).toHaveLength(0);
});
it("DOM失败控制保留原键/动作/代次，不自动重发；GET收据才消除未确认",async()=>{const {f}=durable(),c=await controlled(f);const {GeneralRunPanel}=await import("../src/GeneralRunPanel.js");let message="";const perform=async(work:()=>Promise<string>)=>{try{message=await work();}catch{message="unconfirmed";}};render(<GeneralRunPanel session={c} busy={false} perform={perform}/>);fireEvent.change(screen.getByLabelText("耐久运行ID"),{target:{value:runId}});fireEvent.click(screen.getByText("只读刷新运行状态"));await screen.findByText(/运行 .* · queued/);fireEvent.change(screen.getByLabelText("耐久控制键"),{target:{value:opId}});
 const old=f.getMockImplementation()!;f.mockImplementationOnce(async(u,o)=>{await old(u,o);throw new Error("lost receipt");});fireEvent.click(screen.getByText("服务器暂停"));await waitFor(()=>expect(message).toBe("unconfirmed"));expect(screen.getByText(/控制尚未确认/)).toBeTruthy();expect((screen.getByLabelText("耐久控制键") as HTMLInputElement).value).toBe(opId);fireEvent.click(screen.getByText("只读刷新运行状态"));await screen.findByText(/运行 .* · paused/);expect(screen.queryByText(/控制尚未确认/)).toBeNull();expect(f.mock.calls.filter(([,o])=>String(o?.body).includes('"action":"pause"'))).toHaveLength(1);c.destroy();});
