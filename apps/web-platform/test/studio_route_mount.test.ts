// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment jsdom
import {expect,it,vi} from "vitest";
it("/studio实际挂载且不建立Arena练习身份",async()=>{document.body.innerHTML='<div id="app"></div>';history.replaceState(null,"","/studio");const fetcher=vi.fn();vi.stubGlobal("fetch",fetcher);await import("../src/platform-browser.js");await vi.waitFor(()=>expect(document.body.textContent).toContain("权威创作工作台"));expect(fetcher).not.toHaveBeenCalled();expect(document.querySelector("canvas")).toBeNull();window.dispatchEvent(new Event("pagehide"));vi.unstubAllGlobals()});
