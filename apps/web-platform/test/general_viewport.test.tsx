// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import { it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, act, cleanup } from "@testing-library/react";
import { GeneralViewport } from "../src/GeneralViewport.js";
import type { GeneralWorldSession } from "../src/general-remote.js";
const mocks = vi.hoisted(() => ({ present: vi.fn(), dispose: vi.fn() }));
vi.mock("@rynvek/engine", () => ({ createGeneralRenderHost: () => ({ present: mocks.present, dispose: mocks.dispose }) }));
function setup() { vi.useFakeTimers(); vi.stubGlobal("Worker", class {
}); Object.defineProperty(HTMLCanvasElement.prototype, "transferControlToOffscreen", { value: () => ({}), configurable: true }); const listeners = new Set<() => void>(); const s = { world: "019aabbc-dddd-7eee-8fff-000000000001", renderFrame: vi.fn(async () => ({ tick: 1 })), onPrivateInvalidated: (f: () => void) => { listeners.add(f); return () => listeners.delete(f); }, invalidatePrivate: vi.fn(() => { for (const f of listeners)
        f(); }) }; const invalidated = vi.fn(); render(<GeneralViewport session={s as unknown as GeneralWorldSession} onInvalidated={invalidated}/>); return { s, invalidated, listeners }; }
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.clearAllMocks(); delete (HTMLCanvasElement.prototype as unknown as {
    transferControlToOffscreen?: unknown;
}).transferControlToOffscreen; });
it("显式开启才读帧，断线/授权失效即清画布并终止Worker，晚回执不复活", async () => { const { s, invalidated } = setup(); expect(s.renderFrame).not.toHaveBeenCalled(); await act(async () => fireEvent.click(screen.getByText("明确开启WebGPU只读显示"))); expect(mocks.present).toHaveBeenCalledTimes(1); s.renderFrame.mockRejectedValueOnce(new Error("403")); await act(async () => vi.advanceTimersByTimeAsync(5000)); expect(screen.queryByLabelText("General权威只读画布")).toBeNull(); expect(mocks.dispose).toHaveBeenCalledTimes(1); expect(s.invalidatePrivate).toHaveBeenCalled(); expect(invalidated).toHaveBeenCalled(); });
it("换世界通知/离线与失鲜超时清私有态，后台不会单步或降为假画布", async () => { const { s, listeners } = setup(); await act(async () => fireEvent.click(screen.getByText("明确开启WebGPU只读显示"))); act(() => { for (const f of listeners)
    f(); }); expect(screen.queryByLabelText("General权威只读画布")).toBeNull(); await act(async () => fireEvent.click(screen.getByText("明确开启WebGPU只读显示"))); act(() => window.dispatchEvent(new Event("offline"))); expect(s.invalidatePrivate).toHaveBeenCalled(); expect(screen.queryByLabelText("General权威只读画布")).toBeNull(); });
it("读帧挂起时最迟12秒授权失鲜清除，启动握手挂起也不会无限缓存私有帧", async () => { const { s } = setup(); await act(async () => fireEvent.click(screen.getByText("明确开启WebGPU只读显示"))); s.renderFrame.mockImplementationOnce(() => new Promise<never>(() => { })); await act(async () => vi.advanceTimersByTimeAsync(12001)); expect(screen.queryByLabelText("General权威只读画布")).toBeNull(); expect(mocks.dispose).toHaveBeenCalledTimes(1); expect(s.invalidatePrivate).toHaveBeenCalled(); });
