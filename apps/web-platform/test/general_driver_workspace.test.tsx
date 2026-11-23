// @vitest-environment jsdom
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent, waitFor } from "@testing-library/react";
import { GeneralWorkspace } from "../src/GeneralWorkspace.js";
import { GeneralWorldSession } from "../src/general-remote.js";
const world = "019aabbc-dddd-7eee-8fff-000000000001", grant = "019aabbc-dddd-7eee-8fff-000000000002", pid = "18446744073709551615";
const identity = { principal_id: pid, kind: "Human", status: "Active", ed25519_pubkey: "ab".repeat(32) };
const frame = (tick = 0) => ({ format: "rynvek-general-frame/1", worldId: world.replaceAll("-", ""), tick, revision: tick, stateRoot: tick.toString(16).padStart(64, "0"), bodies: [{ serial: 1, positionQ32: [4294967296, 4294967296], radiusQ32: 1073741824 }] });
const grantView = { principal_id: "2", grant_id: grant, active: true, purpose: "general_rule_drive", consent: "general-installed-rule-drive/1" };
function transport() {
    let tick = 0;
    return vi.fn<typeof fetch>(async (input, init) => {
        const path = new URL(String(input)).pathname;
        if (path === "/api/v1/auth/session")
            return Response.json(identity);
        if (path !== `/api/v1/worlds/${world}/general/drive`)
            throw new Error("不得请求owner接口");
        const b = JSON.parse(String(init?.body));
        expect(b.grant_id).toBe(grant);
        if (b.frame)
            return Response.json({ world_id: world, grant_id: grant, frame: frame(tick) });
        const c = b.command;
        if (c.action === "connect")
            return Response.json({ host_fence: 1, frame: frame(tick) });
        if (c.action === "acquire")
            return Response.json({ host_fence: 1, lease: { holder: { id: pid, kind: "Human" }, faction: 0, level: "Sovereign", fencing_token: 2 } });
        if (c.action === "tick") {
            tick++;
            const f = frame(tick);
            return Response.json({ duplicate: false, commit: { tick, revision: tick, state_root: f.stateRoot }, frame: f });
        }
        if (c.action === "release")
            return Response.json({ released: true, closed: false, commit: { tick, revision: tick, state_root: frame(tick).stateRoot } });
        throw new Error("未知动作");
    });
}
async function bound(f: typeof fetch) { const c = await GeneralWorldSession.openRuleDriver("http://127.0.0.1:3000", "临时测试能力", f); await c.join(world, grant); return c; }
async function dom(f: typeof fetch) { render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>); fireEvent.click(screen.getByText("规则协作者模式")); fireEvent.change(screen.getByLabelText("协作者能力令牌"), { target: { value: "临时测试能力" } }); fireEvent.click(screen.getByText("验证本人协作身份")); await screen.findByText("绑定目的并只读"); fireEvent.change(screen.getByLabelText("协作世界ID"), { target: { value: world } }); fireEvent.change(screen.getByLabelText("协作准入版本"), { target: { value: grant } }); fireEvent.click(screen.getByText("绑定目的并只读")); await screen.findByText(/协作Tick 0/); }
afterEach(() => { cleanup(); vi.useRealTimers(); });
it("协作会话共用正式身份而不探测owner；绑定只读，不取得控制", async () => { const f = transport(), c = await bound(f); expect(c.identity.principal_id).toBe(pid); expect(c.frame?.tick).toBe(0); expect(c.hostFence).toBeUndefined(); expect(f.mock.calls).toHaveLength(2); expect(Object.hasOwn(c, "observation")).toBe(false); expect(Object.hasOwn(c, "rules")).toBe(false); const visible = await c.renderFrame(); visible.bodies.length = 0; expect(c.frame?.bodies).toHaveLength(1); c.destroy(); await expect(c.refresh()).rejects.toThrow(); });
it("明确连接/控制后仅一个真实命令，回执帧与根必须相符；交还不关闭", async () => { const f = transport(), c = await bound(f); await expect(c.advance()).rejects.toThrow(); await c.refresh(); await c.connect(); await c.acquire(); await c.advance(); expect(c.frame?.tick).toBe(1); const writes = f.mock.calls.map(([, o]) => o?.body ? JSON.parse(String(o.body)).command : undefined).filter(Boolean); expect(writes.map(w => w.action)).toEqual(["connect", "acquire", "tick"]); expect(writes[2].input.envelope).toMatchObject({ principal_id: pid, target_tick: 1, payload: [] }); await c.release(); expect(c.controlFence).toBeUndefined(); expect(c.frame?.tick).toBe(1); c.destroy(); });
it("错提交根、额外owner观察和撤权失败都清帧及双fence，不盲目重试", async () => {
    for (const bad of [Response.json({ duplicate: false, commit: { tick: 1, revision: 1, state_root: "ab".repeat(32) }, frame: frame(1) }), Response.json({ duplicate: false, commit: { tick: 1, revision: 1, state_root: frame(1).stateRoot }, frame: frame(1), observation: { private: true } }), new Response("秘密", { status: 404 })]) {
        const f = transport(), c = await bound(f);
        await c.connect();
        await c.acquire();
        f.mockResolvedValueOnce(bad);
        await expect(c.advance()).rejects.toThrow();
        expect(c.frame).toBeUndefined();
        expect(c.hostFence).toBeUndefined();
        expect(c.controlFence).toBeUndefined();
        expect(f.mock.calls).toHaveLength(5);
        c.destroy();
    }
});
it("私有失效/销毁拒并发与晚成功，旧响应不能复活帧", async () => { const f = transport(), c = await bound(f); let finish!: (v: Response) => void; f.mockImplementationOnce(() => new Promise(r => finish = r)); const connecting = c.connect(); await expect(c.acquire()).rejects.toThrow("繁忙"); c.invalidatePrivate(); finish(Response.json({ host_fence: 1, frame: frame() })); await expect(connecting).rejects.toThrow(); expect(c.frame).toBeUndefined(); c.destroy(); });
it("DOM角色互斥且无owner源码/私包/维护/运行按钮，绑定只读后明确单步", async () => { const f = transport(); await dom(f); expect(screen.queryByText("发现私有规则版本")).toBeNull(); expect(screen.queryByText("读取所选世界")).toBeNull(); expect(screen.queryByText("明确开始有界实付运行")).toBeNull(); expect(f.mock.calls).toHaveLength(2); fireEvent.click(screen.getByText("明确连接协作宿主")); await screen.findByText(/原宿主已确认/); fireEvent.click(screen.getByText("明确取得协作控制")); await screen.findByText(/已取得原Sovereign租约/); fireEvent.click(screen.getByText("明确驱动一个规则Tick")); await screen.findByText(/协作Tick 1/); fireEvent.click(screen.getByText("所有者模式")); expect(screen.queryByText(/协作Tick/)).toBeNull(); expect(screen.getByLabelText("General能力令牌")).toHaveProperty("value", ""); expect(screen.queryByText("读取所选世界")).toBeNull(); });
it("DOM不启GPU也在离线/隐藏清最小私有态与控制", async () => { const f = transport(); await dom(f); fireEvent(window, new Event("offline")); await waitFor(() => expect(screen.queryByText(/协作Tick 0/)).toBeNull()); expect((screen.getByText("明确驱动一个规则Tick") as HTMLButtonElement).disabled).toBe(true); });
it("DOM绑定请求未完成就断开，晚回执不复活身份/私有帧", async () => { const f = transport(); render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>); fireEvent.click(screen.getByText("规则协作者模式")); fireEvent.change(screen.getByLabelText("协作者能力令牌"), { target: { value: "临时测试能力" } }); fireEvent.click(screen.getByText("验证本人协作身份")); await screen.findByText("绑定目的并只读"); fireEvent.change(screen.getByLabelText("协作世界ID"), { target: { value: world } }); fireEvent.change(screen.getByLabelText("协作准入版本"), { target: { value: grant } }); let finish!: (v: Response) => void; f.mockImplementationOnce(() => new Promise(r => finish = r)); fireEvent.click(screen.getByText("绑定目的并只读")); fireEvent.click(screen.getByText("断开协作并清空")); finish(Response.json({ world_id: world, grant_id: grant, frame: frame() })); await waitFor(() => expect(screen.queryByText(/协作Tick/)).toBeNull()); expect(screen.queryByText("绑定目的并只读")).toBeNull(); });
it("所有者DOM须显式consent/CAS，仅管理正确目的且撤销保版本", async () => {
    const f = vi.fn<typeof fetch>(async (input, init) => {
        const url = String(input);
        if (url.endsWith("auth/session"))
            return Response.json(identity);
        if (url.endsWith(`/worlds/${world}`))
            return Response.json({ world_id: world, owner_principal_id: pid, world_profile: "General_Circle_v1", game_mode: "General_Controlled" });
        if (url.endsWith("/general"))
            return Response.json({ world_id: world, visibility: "private_owner_only", execution: "server-authoritative-single-chunk", commit: { tick: 0, revision: 0, state_root: "00".repeat(32) }, bodies: [] });
        if (url.endsWith("/drivers")) {
            const b = init?.body ? JSON.parse(String(init.body)) : undefined;
            if (!b)
                return Response.json({ world_id: world, grants: [] });
            return Response.json({ ...grantView, active: b.active });
        }
        throw new Error("未知请求");
    });
    render(<GeneralWorkspace origin="http://127.0.0.1:3000" fetcher={f}/>);
    fireEvent.change(screen.getByLabelText("General能力令牌"), { target: { value: "临时测试能力" } });
    fireEvent.click(screen.getByText("验证既有身份"));
    await screen.findByText("读取所选世界");
    fireEvent.change(screen.getByLabelText("General世界ID"), { target: { value: world } });
    fireEvent.click(screen.getByText("读取所选世界"));
    await screen.findByText("明确授予规则驱动");
    fireEvent.change(screen.getByLabelText("规则协作者主体"), { target: { value: "2" } });
    expect((screen.getByText("明确授予规则驱动") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByLabelText("明确同意规则驱动目的"));
    fireEvent.click(screen.getByText("明确授予规则驱动"));
    await screen.findByText("撤销规则驱动 2");
    let body = JSON.parse(String(f.mock.calls.at(-1)?.[1]?.body));
    expect(body).toMatchObject({ principal_id: "2", expected_grant_id: null, active: true, consent: grantView.consent });
    fireEvent.click(screen.getByText("撤销规则驱动 2"));
    await screen.findByText(/目的已撤销/);
    body = JSON.parse(String(f.mock.calls.at(-1)?.[1]?.body));
    expect(body.expected_grant_id).toBe(grant);
    expect(body.active).toBe(false);
});
it("修改世界/目的草稿立即清旧态，必须重新绑定，不能误推进旧世界", async () => { const f = transport(); await dom(f); fireEvent.click(screen.getByText("明确连接协作宿主")); await screen.findByText(/原宿主已确认/); fireEvent.click(screen.getByText("明确取得协作控制")); await screen.findByText(/已取得原Sovereign租约/); const before = f.mock.calls.length; fireEvent.change(screen.getByLabelText("协作世界ID"), { target: { value: grant } }); expect(screen.queryByText(/协作Tick/)).toBeNull(); expect((screen.getByText("明确连接协作宿主") as HTMLButtonElement).disabled).toBe(true); expect((screen.getByText("明确驱动一个规则Tick") as HTMLButtonElement).disabled).toBe(true); expect(f.mock.calls).toHaveLength(before); });
it("未启GPU也只在12秒内展示授权最小帧，不靠永久静态缓存保留协作态", async () => { vi.useFakeTimers(); const c = await bound(transport()); vi.advanceTimersByTime(11999); expect(c.frame?.tick).toBe(0); vi.advanceTimersByTime(1); expect(c.frame).toBeUndefined(); expect(c.hostFence).toBeUndefined(); expect(c.controlFence).toBeUndefined(); c.destroy(); });
