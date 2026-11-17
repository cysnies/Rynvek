// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from "vitest";
import {AudioDirector,AudioMixer,parseSoundBank,PLACEHOLDER_SYNTH_RECIPES} from "../src/index.js";
import {FakeAudioContext,FakeOscillatorNode} from "../src/testing.js";
const manifest={schema:"rynvek.audio-manifest.v1",events:{"building.placed":{status:"placeholder",bus:"sfx_world",gain:.17,cooldown_ms:500,max_voices:1}}};
it("私有音效数据实际驱动原Director音色/包络/冷却且不改冻结缺省配方",()=>{
 const bank=parseSoundBank(manifest,{"building.placed":{freqStart:733,freqEnd:222,releaseMs:77}});const c=new FakeAudioContext(),director=new AudioDirector(c.asAudioContext(),{mixer:new AudioMixer(c.asAudioContext()),...bank});
 const played=director.play("building.placed");expect(played.played).toBe(true);expect(c.nodesOfType<FakeOscillatorNode>("oscillator")[0]!.frequency.events[0]!.value).toBe(733);expect(played).toEqual({played:true,source:"synth",endTime:(2+65+77)/1000});expect(director.play("building.placed")).toEqual({played:false,reason:"cooldown"});expect(PLACEHOLDER_SYNTH_RECIPES["building.placed"]!.freqStart).toBe(220);
});
it("音效参数/未知原型字段/媒体URL/总线越界在客户端同样拒绝",()=>{
 for(const patch of [{"building.placed":{releaseMs:501}},{"building.placed":{filterQ:100}},{"building.placed":{freqStart:Infinity}},{"building.placed":{bus:"voice_alert"}},JSON.parse('{"__proto__":{"freqStart":440}}'),{"unknown.event":{}},{"building.placed":{constructor:440}}])expect(()=>parseSoundBank(manifest,patch)).toThrow();
 expect(()=>parseSoundBank({...manifest,events:{"building.placed":{status:"asset",bus:"sfx_world",src:"https://example.invalid/x.ogg",license:"CC0-1.0"}}},{})).toThrow();
 expect(()=>parseSoundBank({...manifest,events:{"building.placed":{status:"placeholder",bus:"voice_alert"}}},{})).toThrow();
});
