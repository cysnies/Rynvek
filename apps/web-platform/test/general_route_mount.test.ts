// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import {expect,it,vi} from "vitest";
it("/general真实挂载且不会加载Arena练习/Worker或自动请求，pagehide清身份",async()=>{
 document.body.innerHTML='<div id="app"></div>';history.replaceState(null,"","/general");
 const fetcher=vi.fn();vi.stubGlobal("fetch",fetcher);await import("../src/platform-browser.js");
 await vi.waitFor(()=>expect(document.body.textContent).toContain("General · 私有权威世界工作台"));
 expect(fetcher).not.toHaveBeenCalled();expect(document.querySelector("canvas")).toBeNull();
 window.dispatchEvent(new Event("pagehide"));expect(document.querySelector("#app")?.textContent).toBe("");
 window.dispatchEvent(new PageTransitionEvent("pageshow",{persisted:true}));await vi.waitFor(()=>expect(document.body.textContent).toContain("尚未验证身份"));
 expect(document.querySelector('input[type="password"]')?.getAttribute("value")).toBe("");
 window.dispatchEvent(new Event("pagehide"));vi.unstubAllGlobals();
});
