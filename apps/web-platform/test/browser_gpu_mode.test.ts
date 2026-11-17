// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
it("显式GPU入口不先占用2D上下文；服务失败仍明确显示且不制造Worker/世界", async () => {
  window.history.replaceState({}, "", "/arena?renderer=webgpu"); document.body.innerHTML = '<div id="app"></div>';
  const get = vi.spyOn(HTMLCanvasElement.prototype, "getContext");
  const worker = vi.fn(); vi.stubGlobal("Worker", worker); vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("开发服务未启动")));
  await import("../src/browser.js");
  expect(get).not.toHaveBeenCalled(); expect(document.getElementById("renderer")!.textContent).toContain("WebGPU");
  (document.getElementById("start") as HTMLButtonElement).click();
  await vi.waitFor(() => expect(document.getElementById("log")!.textContent).toContain("开发服务未启动"));
  expect(worker).not.toHaveBeenCalled(); expect(document.getElementById("biomass")!.textContent).toBe("—");
  vi.restoreAllMocks(); vi.unstubAllGlobals();
});
