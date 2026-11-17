// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { createRoot } from "react-dom/client";
import { LabWorkspace } from "./LabWorkspace.js";
import "./lab.css";
// 离开页面销毁凭据与请求；从往返缓存恢复时新挂载，不复用旧身份。
let root: ReturnType<typeof createRoot>|undefined;
function mount(){
  if(root)return;
  document.title="Rynvek Lab · 权威实验工作台";
  root=createRoot(document.querySelector("#app")!);root.render(<LabWorkspace/>);
}
mount();
window.addEventListener("pagehide",()=>{root?.unmount();root=undefined;});
window.addEventListener("pageshow",event=>{if(event.persisted)mount();});
