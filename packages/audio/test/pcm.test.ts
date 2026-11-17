// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from "vitest";
import {createTechnicalPcm16Wav,decodePcm16Wav,parseSoundBank,AudioDirector} from "../src/index.js";
import {FakeAudioContext,FakeAudioBufferSourceNode,FakeGainNode,FakeStereoPannerNode} from "../src/testing.js";
const path=`samples/${"a".repeat(64)}.wav`;
const b64=(b:Uint8Array)=>btoa(String.fromCharCode(...b));
const event={status:"asset",bus:"sfx_world",src:path,license:"CC0-1.0",gain:.2,max_voices:1,cooldown_ms:0};
const manifest={schema:"rynvek.audio-manifest.v1",events:{"building.placed":event}};
it("严格PCM仅有界单声道整数数据；拒绝压缩/立体声/额外块/伪尺寸",()=>{
 const bytes=createTechnicalPcm16Wav(),pcm=decodePcm16Wav(bytes);expect(pcm.frames.length).toBe(1280);expect(pcm.rate).toBe(16000);expect(pcm.frames.some(v=>v!==0)).toBe(true);
 for(const [i,value]of [[20,3],[22,2],[34,32],[40,255],[4,0],[16,18]]){const bad=bytes.slice();bad[i!]=value!;expect(()=>decodePcm16Wav(bad)).toThrow();}
 expect(()=>decodePcm16Wav(new Uint8Array(192045))).toThrow();expect(()=>createTechnicalPcm16Wav(440,3)).toThrow();
});
it("采样表预分配限制、引用/署名/总线隔离真实执行，不允许URL与缺失采样",()=>{
 const samples={[path]:b64(createTechnicalPcm16Wav())};expect(parseSoundBank(manifest,{},samples).samples[path]!.frames.length).toBe(1280);
 for(const e of [{...event,src:"https://example.invalid/a.wav"},{...event,license:"CC-BY-4.0"},{...event,bus:"voice_alert"}])expect(()=>parseSoundBank({...manifest,events:{"building.placed":e}},{},samples)).toThrow();
 expect(()=>parseSoundBank(manifest,{})).toThrow();expect(()=>parseSoundBank(manifest,{},{[path]:"A".repeat(524289)})).toThrow();
});
it("真实Director采样节点、增益/声像/并发门控及结束回收，不冒称硬件DSP",async()=>{
 const c=new FakeAudioContext(),bank=parseSoundBank(manifest,{},{[path]:b64(createTechnicalPcm16Wav())});
 const d=new AudioDirector(c.asAudioContext(),{manifest:bank.manifest,loader:async src=>{const p=bank.samples[src]!;const b=c.createBuffer(1,p.frames.length,p.rate);b.getChannelData(0).set(p.frames);return b as unknown as AudioBuffer;}});
 expect(await d.preload()).toEqual([]);expect(d.play("building.placed",{intensity:.6,pan:.5,attenuation:.5}).played).toBe(true);
 const s=c.nodesOfType<FakeAudioBufferSourceNode>("bufferSource")[0]!;expect(s.buffer!.length).toBe(1280);expect(s.startTime).toBe(0);
 expect(c.nodesOfType<FakeGainNode>("gain").some(n=>n.gain.events.some(e=>e.value===.06))).toBe(true);
 expect(c.nodesOfType<FakeStereoPannerNode>("stereoPanner")[0]!.pan.events[0]!.value).toBe(.5);
 expect(d.play("building.placed")).toEqual({played:false,reason:"voice_limit"});s.onended?.();expect(s.disconnected).toBe(true);
});
