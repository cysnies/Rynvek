// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import {expect,it,vi} from "vitest";
it("/lab实际挂载React工作台，不启动Arena练习身份或生成示范数据",async()=>{
  document.body.innerHTML='<div id="app"></div>';history.replaceState(null,"","/lab");
  const fetcher=vi.fn();vi.stubGlobal("fetch",fetcher);
  await import("../src/platform-browser.js");
  await vi.waitFor(()=>expect(document.body.textContent).toContain("RYNVEK / LAB"));
  expect(document.querySelector("canvas")).toBeNull();expect(document.body.textContent).toContain("尚无实验");expect(fetcher).not.toHaveBeenCalled();
  window.dispatchEvent(new Event("pagehide"));
  expect(document.querySelector("#app")?.textContent).toBe("");
  window.dispatchEvent(new PageTransitionEvent("pageshow",{persisted:true}));
  await vi.waitFor(()=>expect(document.body.textContent).toContain("尚无实验"));
  expect(document.querySelector('input[type="password"]')?.getAttribute("value")).toBe("");
  window.dispatchEvent(new Event("pagehide"));vi.unstubAllGlobals();
});
