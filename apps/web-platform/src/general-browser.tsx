// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { createRoot } from "react-dom/client";
import { GeneralWorkspace } from "./GeneralWorkspace.js";
import "./lab.css";
let root: ReturnType<typeof createRoot> | undefined;
function mount() { if (root)
    return; document.title = "Rynvek General · 权威世界"; root = createRoot(document.querySelector("#app")!); root.render(<GeneralWorkspace />); }
mount();
window.addEventListener("pagehide", () => { root?.unmount(); root = undefined; });
window.addEventListener("pageshow", e => { if (e.persisted)
    mount(); });
