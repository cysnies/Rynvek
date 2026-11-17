// @vitest-environment jsdom
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it,vi} from "vitest";
import {mountPrivateAudioPanel,type PrivateAudioContext} from "../src/private-audio-panel.js";
import type {ArenaAudioSession} from "../src/arena-audio.js";
const meta={owner_id:"42",package_id:"owner.audio",version:"1.0.0",artifact_cid:"a".repeat(64),profile:{role:"soundpack"},dependencies:[],verification:{trust_level:3},revoked:false};
const settle=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
it("原入口显式发现/校验应用，不隐式手势启用；撤销切图，身份清理私有状态",async()=>{
 const root=document.createElement("section");document.body.append(root);const loaded={metadata:meta,manifest:{events:{}},recipes:{}};const hub={list:vi.fn(async()=>[meta]),load:vi.fn(async()=>loaded),revoke:vi.fn(async()=>{})};const audio={setSoundBank:vi.fn(),clearSoundBank:vi.fn(),enableFromGesture:vi.fn()};let ctx:PrivateAudioContext|null={worldId:"world-a",principalId:"42",hub:hub as never};const panel=mountPrivateAudioPanel(root,()=>ctx,audio as unknown as ArenaAudioSession);const click=(k:string)=>(root.querySelector(`[data-audio="${k}"]`) as HTMLButtonElement).click();
 expect(hub.list).not.toHaveBeenCalled();click("discover");await settle();expect(root.textContent).toContain("owner.audio");click("apply");await settle();expect(audio.setSoundBank).toHaveBeenCalledWith(loaded.manifest,loaded.recipes,expect.stringContaining("owner.audio"));expect(audio.enableFromGesture).not.toHaveBeenCalled();click("revoke");await settle();expect(audio.clearSoundBank).toHaveBeenCalled();ctx=null;panel.update();expect(root.textContent).not.toContain("owner.audio");expect((root.querySelector('[data-audio="apply"]') as HTMLButtonElement).disabled).toBe(true);root.remove();
});
it("换世界后迟到的私有下载不得应用到新音频会话",async()=>{
 const root=document.createElement("section");let release!:(value:unknown)=>void;const hub={list:vi.fn(async()=>[meta]),load:vi.fn(()=>new Promise(r=>{release=r;}))};const audio={setSoundBank:vi.fn()};let ctx:PrivateAudioContext|null={worldId:"world-a",principalId:"42",hub:hub as never};const panel=mountPrivateAudioPanel(root,()=>ctx,audio as unknown as ArenaAudioSession);(root.querySelector('[data-audio="discover"]') as HTMLButtonElement).click();await settle();(root.querySelector('[data-audio="apply"]') as HTMLButtonElement).click();await settle();ctx={...ctx!,worldId:"world-b"};panel.update();release({metadata:meta,manifest:{events:{}},recipes:{}});await settle();expect(audio.setSoundBank).not.toHaveBeenCalled();expect(root.textContent).not.toContain("owner.audio");
});
