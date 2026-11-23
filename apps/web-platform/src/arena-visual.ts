// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {readRview,loadRview,TrustLevel} from "@rynvek/assets";
import {rviewThemeLayers,packedWorldPalette} from "@rynvek/visualization";
import {resolveTheme} from "@rynvek/ui";
import type {LoadedPrivateVisual} from "@rynvek/agent-sdk";
export interface ArenaVisualTheme {readonly background:number;readonly factions:readonly number[]}
export const DEFAULT_ARENA_THEME:ArenaVisualTheme=(()=>{const p=packedWorldPalette(resolveTheme({base:"neutral-dark"}));return Object.freeze({background:p.background,factions:Object.freeze([...p.factions])});})();
const allowed=(k:string)=>k==="--rv-world-bg"||/^--rv-faction-[1-7]$/.test(k);
/** Runs only after explicit signed SDK load. Reuses existing RYVW reader/loader, five-layer resolver and GPU palette bridge. */
export function privateVisualTheme(loaded:LoadedPrivateVisual):ArenaVisualTheme {
 if(!loaded.packages.length||loaded.packages.length>9)throw Error("视觉图越界");
 const layers=[];let total=0;
 for(const {bytes,metadata}of loaded.packages){total+=bytes.length;if(bytes.length>65536||total>589824)throw Error("视觉资源越界");
  const archive=readRview(bytes),m=archive.manifest;
  if(m.id!==metadata.package_id||m.version!==metadata.version||m.engine_compat!=="rynvek-private-theme/1"||m.entries.length<1||m.entries.length>2||!["theme","library"].includes(metadata.profile.role)||!["CC0-1.0","CC-BY-4.0"].includes(m.license)||(m.license==="CC-BY-4.0"&&!metadata.profile.credit.trim()))throw Error("视觉profile/署名/许可证非法");
  for(const e of m.entries){if(!((e.path==="palette.json"&&e.kind==="palette")||(e.path==="theme.json"&&e.kind==="theme"))||e.media_type!=="application/json"||e.length>4096)throw Error("只许受限色板/主题，无shader/纹理");}
  const pkg=loadRview(bytes,{trustLevel:TrustLevel.UntrustedEphemeral});
  for(const p of pkg.palettes.values())if(Object.keys(p.colors).length>8||Object.keys(p.colors).some(k=>!allowed(`--rv-${k}`)))throw Error("色板令牌越权");
  for(const t of pkg.themes.values())if(Object.keys(t.tokens).length>8||Object.entries(t.tokens).some(([k,v])=>!allowed(k)||!/^#[0-9a-fA-F]{6}$/.test(v)))throw Error("主题令牌越权/透明颜色");
  const converted=rviewThemeLayers(pkg);if(converted.rejected.length)throw Error("视觉主题层拒绝");layers.push(...converted.layers);
 }
 const p=packedWorldPalette(resolveTheme({base:"neutral-dark",layers}));return Object.freeze({background:p.background,factions:Object.freeze([...p.factions])});
}
export function cssPackedColor(color:number):string{return `#${[color&255,(color>>>8)&255,(color>>>16)&255].map(c=>c.toString(16).padStart(2,"0")).join("")}`;}
