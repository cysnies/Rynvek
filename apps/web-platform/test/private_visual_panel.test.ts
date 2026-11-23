// @vitest-environment jsdom
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it,vi} from "vitest";
import {mountPrivateVisualPanel,type PrivateVisualContext} from "../src/private-visual-panel.js";
const meta={owner_id:"42",package_id:"owner.visual-main",version:"1.0.0",artifact_cid:"a".repeat(64),profile:{role:"theme",credit:""},dependencies:[],verification:{trust_level:3},revoked:false};
const settle=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
it("原入口显式发现/校验应用，撤销恢复内置；身份清理私有状态",async()=>{
 const root=document.createElement("section");document.body.append(root);
 const loaded={metadata:meta,packages:[]};
 const hub={list:vi.fn(async()=>[meta]),load:vi.fn(async()=>loaded),revoke:vi.fn(async()=>{})};
 const visual={apply:vi.fn(async()=>{}),reset:vi.fn()};
 let ctx:PrivateVisualContext|null={worldId:"world-a",principalId:"42",hub:hub as never};
 const panel=mountPrivateVisualPanel(root,()=>ctx,visual);
 const click=(k:string)=>(root.querySelector(`[data-visual="${k}"]`) as HTMLButtonElement).click();
 expect(hub.list).not.toHaveBeenCalled();click("discover");await settle();expect(root.textContent).toContain("owner.visual-main");
 click("apply");await settle();expect(visual.apply).toHaveBeenCalledWith(loaded,expect.stringContaining("owner.visual-main"));
 click("revoke");await settle();expect(visual.reset).toHaveBeenCalled();
 ctx=null;panel.update();expect(root.textContent).not.toContain("owner.visual-main");
 expect((root.querySelector('[data-visual="apply"]') as HTMLButtonElement).disabled).toBe(true);root.remove();
});
it("换世界后迟到的私有下载不得应用到新视觉会话",async()=>{
 const root=document.createElement("section");let release!:(value:unknown)=>void;
 const hub={list:vi.fn(async()=>[meta]),load:vi.fn(()=>new Promise(r=>{release=r;}))};
 const visual={apply:vi.fn(async()=>{}),reset:vi.fn()};
 let ctx:PrivateVisualContext|null={worldId:"world-a",principalId:"42",hub:hub as never};
 const panel=mountPrivateVisualPanel(root,()=>ctx,visual);
 (root.querySelector('[data-visual="discover"]') as HTMLButtonElement).click();await settle();
 (root.querySelector('[data-visual="apply"]') as HTMLButtonElement).click();await settle();
 ctx={...ctx!,worldId:"world-b"};panel.update();release({metadata:meta,packages:[]});await settle();
 expect(visual.apply).not.toHaveBeenCalled();expect(root.textContent).not.toContain("owner.visual-main");
});
