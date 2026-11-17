// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import { it,expect,vi } from "vitest";
it("真实入口挂载操作面板，服务失败时明确错误而非生成假世界",async()=>{
  document.body.innerHTML='<div id="app"></div>';
  const context={setTransform:vi.fn(),fillRect:vi.fn(),fillText:vi.fn()} as unknown as CanvasRenderingContext2D;
  vi.spyOn(HTMLCanvasElement.prototype,"getContext").mockReturnValue(context);
  vi.stubGlobal("requestAnimationFrame",vi.fn(()=>1));
  vi.stubGlobal("fetch",vi.fn().mockRejectedValue(new Error("开发服务未启动")));
  await import("../src/browser.js");
  expect(document.querySelector("canvas")?.getAttribute("aria-label")).toContain("战争迷雾");
  expect(document.body.textContent).toContain("Canvas2D");
  const start=document.getElementById("start") as HTMLButtonElement;
  start.click();
  await vi.waitFor(()=>expect(document.getElementById("log")?.textContent).toContain("开发服务未启动"));
  expect((document.getElementById("select") as HTMLButtonElement).disabled).toBe(true);
  expect(document.getElementById("biomass")?.textContent).toBe("—");
  expect(start.disabled).toBe(false);
  vi.restoreAllMocks();vi.unstubAllGlobals();
});
