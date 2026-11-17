// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { resolvePlatformSurface } from "./index.js";
// Lab真实挂载而不是只返回路由描述；不初始化Arena画布/练习身份或加载其WASM。
const route=resolvePlatformSurface(location.pathname);
if(route.surface==="lab")void import("./lab-browser.js");
else if(route.surface==="arena")void import("./browser.js");
else {
  const app=document.querySelector("#app")!;const text=document.createElement("p");
  text.textContent=`${route.titleZh}的通用浏览器挂载尚未实现；不会拿Arena或假数据冒充。`;
  const link=document.createElement("a");link.href="/lab";link.textContent="打开真实Lab工作台";app.replaceChildren(text,link);
}
