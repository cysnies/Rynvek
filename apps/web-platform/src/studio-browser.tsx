// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {createRoot} from "react-dom/client";
import {StudioApp} from "./StudioApp.js";
let root:ReturnType<typeof createRoot>|undefined;
function mount(){if(root)return;document.title="Rynvek Studio · 权威创作";root=createRoot(document.querySelector("#app")!);root.render(<StudioApp/>)}
mount();window.addEventListener("pagehide",()=>{root?.unmount();root=undefined});window.addEventListener("pageshow",e=>{if(e.persisted)mount()});
