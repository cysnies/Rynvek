// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { parseAudioManifest, createPlaceholderManifest, type AudioManifest } from "./manifest.js";
import { PLACEHOLDER_SYNTH_RECIPES, type SynthRecipe } from "./placeholder_synth.js";
export interface SoundBank {readonly manifest:AudioManifest;readonly recipes:Readonly<Record<string,SynthRecipe>>}
/** Bounded data-only patch; cannot change the fixed semantic event -> bus permission map. */
export function parseSoundBank(manifest:unknown,patches:unknown):SoundBank {
 const parsed=parseAudioManifest(manifest);if(Object.keys(parsed.events).length>24)throw new Error("最多24音频事件");
 for(const [id,e]of Object.entries(parsed.events))if(e.status!=="placeholder"||!Object.hasOwn(PLACEHOLDER_SYNTH_RECIPES,id)||e.max_voices!==undefined&&e.max_voices>16)throw new Error("本profile仅支持有界程序化音效，不加载媒体或URL");
 if(!patches||typeof patches!=="object"||Array.isArray(patches)||Object.keys(patches).length>24)throw new Error("配方覆写须为有界对象");
 const recipes:Record<string,SynthRecipe>={...PLACEHOLDER_SYNTH_RECIPES};
 for(const [id,raw]of Object.entries(patches)){
  const base=PLACEHOLDER_SYNTH_RECIPES[id];if(!Object.hasOwn(PLACEHOLDER_SYNTH_RECIPES,id)||!base||!raw||typeof raw!=="object"||Array.isArray(raw))throw new Error("未知配方或非法覆写");
  const out:Record<string,unknown>={...base};for(const [key,value]of Object.entries(raw)){
   if(key==="oscType"||key==="filterType"){if(typeof value!=="string"||!(key==="oscType"?["sine","square","sawtooth","triangle"]:["lowpass","highpass","bandpass"]).includes(value))throw new Error("非法音色类型");}
   else {const limits:Record<string,readonly[number,number]>={freqStart:[20,8000],freqEnd:[20,8000],filterFreq:[20,8000],attackMs:[1,50],decayMs:[1,500],releaseMs:[1,500],sustainLevel:[0,1],filterQ:[.1,5],noiseMix:[0,.8]};const bound=limits[key];if(!Object.hasOwn(limits,key)||!bound||typeof value!=="number"||!Number.isFinite(value)||value<bound[0]||value>bound[1])throw new Error("配方字段/参数越界");}
   out[key]=value;
  }recipes[id]=Object.freeze(out) as unknown as SynthRecipe;
 }
 return Object.freeze({manifest:{schema:parsed.schema,events:Object.freeze({...createPlaceholderManifest().events,...parsed.events})},recipes:Object.freeze(recipes)});
}
