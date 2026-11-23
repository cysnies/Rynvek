// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from "vitest";
import {writeRview} from "@rynvek/assets";
import {DEFAULT_ARENA_THEME,cssPackedColor,privateVisualTheme} from "../src/arena-visual.js";
import type {LoadedPrivateVisual,PrivateVisualProfile,PrivateVisualVersion} from "@rynvek/agent-sdk";
const enc=new TextEncoder();
const entry=(path:string,kind:"palette"|"theme",value:unknown)=>({path,kind,media_type:"application/json",data:enc.encode(JSON.stringify(value))});
const bytes=(id:string,entries:Parameters<typeof writeRview>[0]["entries"],license:"CC0-1.0"|"CC-BY-4.0"="CC0-1.0")=>writeRview({id,version:"1.0.0",name:id,license,engine_compat:"rynvek-private-theme/1",entries});
const meta=(id:string,role:PrivateVisualProfile["role"],credit="",cid="a".repeat(64)):PrivateVisualVersion=>({owner_id:"42",package_id:id,version:"1.0.0",artifact_cid:cid,publisher_key:"b".repeat(64),signature:"c".repeat(128),profile:{schema:"rynvek.private-visual.v1",runtime:"opaque-palette/1",role,credit,dependencies:[]},dependencies:[],verification:{trust_level:3},revoked:false,visibility:"Private"});
const loaded=(packages:{id:string;bytes:Uint8Array;role:PrivateVisualProfile["role"];credit?:string}[]):LoadedPrivateVisual=>({metadata:meta(packages.at(-1)!.id,packages.at(-1)!.role),packages:packages.map(p=>({metadata:meta(p.id,p.role,p.credit??""),bytes:p.bytes}))});
it("真实受限主题解析为不透明世界/阵营色，根覆盖library，默认色与内置一致",()=>{
 const lib=bytes("owner.visual-library",[entry("palette.json","palette",{colors:{"world-bg":"#000000","faction-7":"#abcdef"}})]);
 const root=bytes("owner.visual-main",[entry("theme.json","theme",{tokens:{"--rv-world-bg":"#102040","--rv-faction-1":"#e01080","--rv-faction-2":"#20c080","--rv-faction-3":"#ff7f00","--rv-faction-4":"#00ff7f","--rv-faction-5":"#7f00ff","--rv-faction-6":"#00ffff","--rv-faction-7":"#010203"}})]);
 const theme=privateVisualTheme(loaded([{id:"owner.visual-library",bytes:lib,role:"library"},{id:"owner.visual-main",bytes:root,role:"theme"}]));
 expect(theme.background).toBe(0xFF402010);
 expect(theme.factions).toEqual([0xFF8010E0,0xFF80C020,0xFF007FFF,0xFF7FFF00,0xFFFF007F,0xFFFFFF00,0xFF030201]);
 expect(cssPackedColor(theme.background)).toBe("#102040");
 expect(DEFAULT_ARENA_THEME.factions).toHaveLength(7);
 expect((DEFAULT_ARENA_THEME.background>>>24)).toBe(255);
});
it("拒绝着色器、纹理、越权令牌、缺署名与非法许可证",()=>{
 const shader=writeRview({id:"owner.visual-bad",version:"1.0.0",name:"bad",license:"CC0-1.0",engine_compat:"rynvek-private-theme/1",entries:[{path:"shaders/main.wgsl",kind:"shader",media_type:"text/wgsl",data:enc.encode("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }")}]});
 expect(()=>privateVisualTheme(loaded([{id:"owner.visual-bad",bytes:shader,role:"theme"}]))).toThrow();
 const escaped=bytes("owner.visual-bad",[entry("theme.json","theme",{tokens:{"--rv-world-bg":"#102040","--rv-color-accent":"#ffffff"}})]);
 expect(()=>privateVisualTheme(loaded([{id:"owner.visual-bad",bytes:escaped,role:"theme"}]))).toThrow();
 const by=bytes("owner.visual-bad",[entry("theme.json","theme",{tokens:{"--rv-world-bg":"#102040"}})],"CC-BY-4.0");
 expect(()=>privateVisualTheme(loaded([{id:"owner.visual-bad",bytes:by,role:"theme",credit:""}]))).toThrow();
});
