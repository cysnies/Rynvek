// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// 真实Rust会话生成的跨语言夹具 + 录制型AudioContext；不冒称DSP离线渲染/浏览器策略/硬件听验。
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {afterEach,expect,it,vi} from "vitest";
import {ArenaClient,type ArenaServerMessage,type ArenaTransport,type FactionObservation,type RtsEvent} from "@rynvek/client";
import {FakeAudioContext,FakeOscillatorNode,FakeGainNode} from "@rynvek/audio/testing";
import {ArenaAudioSession,planArenaAudio,spatialAudio,AUDIO_STALE_MS} from "../src/arena-audio.js";
const transcript=JSON.parse(readFileSync(resolve(process.cwd(),"../../tests/arena/opening.transcript.json"),"utf8"));
const observations=transcript.flatMap((s:{recv:{to:string;message:ArenaServerMessage}[]})=>s.recv.filter(r=>r.to==="human:p0"&&r.message.type==="observation").map(r=>r.message)) as Extract<ArenaServerMessage,{type:"observation"}>[];
const first=observations[0]!,building=observations[2]!;
function harness(){
 let receiver:((m:ArenaServerMessage)=>void)|undefined;
 const t:ArenaTransport={send:()=>{},close:()=>{},onMessage:h=>{receiver=h;return ()=>{receiver=undefined;}}};
 const client=new ArenaClient(t,{principalId:"123",kind:"Human",faction:0});
 return {client,receive:(m:ArenaServerMessage)=>receiver!(m)};
}
afterEach(()=>vi.useRealTimers());
it("真实跨语言窗口分离spiking/continuous且声音不受敌方事件、ghost消失或全世界指标影响",()=>{
 const a=planArenaAudio(building.observation,building.events,first.observation);
 expect(a.cues.map(c=>c.id)).toEqual(["building.completed"]);expect(a.neural.ringDensity).toBe(24/(32*64));expect(a.neural.continuousLevel).toBe(64/256);
 const altered: FactionObservation={...building.observation,ghosts:[],units:building.observation.units.filter(u=>u.faction===0)};
 const hidden:RtsEvent={type:"building_destroyed",id:999,faction:1,kind:"bld.nexus_core"};
 expect(planArenaAudio(altered,[...building.events,hidden],first.observation)).toEqual(a);
 const continuous=observations[4]!.observation;expect(continuous.neural.spiking_neurons).toBe(0);
 expect(planArenaAudio(continuous,[],null).neural.ringDensity).toBe(0);
 expect(()=>planArenaAudio({...altered,neural:{...altered.neural,sample_tick:0}},[],null)).toThrow(/投影非法/);
});
it("默认不创图；手势后真实观察驱动原配方与神经声化，重复Tick和静音期间事件不补播",async()=>{
 const h=harness(),made:FakeAudioContext[]=[];const audio=new ArenaAudioSession(()=>{},()=>{const c=new FakeAudioContext(48000,"suspended");made.push(c);return c.asAudioContext();},()=>({x:32,y:32,halfWidth:32,halfHeight:32}));
 audio.bind(h.client);h.receive(first);expect(made).toHaveLength(0);audio.setNeural(true);
 await audio.enableFromGesture();const c=made[0]!;expect(audio.status.state).toBe("running");expect(c.nodesOfType("oscillator")).toHaveLength(0);
 h.receive(building);expect(c.nodesOfType("oscillator")).toHaveLength(3); // 合并同类completed一条+两个真实状态声化载波。
 const gain=c.nodesOfType<FakeGainNode>("gain");expect(gain.some(g=>g.gain.events.some(e=>e.value===24/(32*64)*.08))).toBe(true);
 h.receive(building);expect(c.nodesOfType("oscillator")).toHaveLength(3);
 audio.mute();expect(c.state).toBe("closed");expect(c.nodesOfType<FakeGainNode>("gain").some(g=>g.disconnected)).toBe(true);
 h.receive(observations[3]!);await audio.enableFromGesture();expect(made[1]!.nodesOfType("oscillator")).toHaveLength(0);
 audio.setNeural(false);audio.detach();h.receive(observations[4]!);expect(made[1]!.state).toBe("closed");
});
it("挂起/隐藏/神经关闭/过期/终局都同步切断旧图；错误或换身份不影响世界且不保留观察",async()=>{
 vi.useFakeTimers();const h=harness(),contexts:FakeAudioContext[]=[];const audio=new ArenaAudioSession(()=>{},()=>{const c=new FakeAudioContext();contexts.push(c);return c.asAudioContext();},()=>({x:32,y:32,halfWidth:32,halfHeight:32}));
 audio.bind(h.client);h.receive(first);audio.setNeural(true);await audio.enableFromGesture();h.receive(building);const c=contexts[0]!;
 audio.setNeural(false);expect(c.nodesOfType<FakeOscillatorNode>("oscillator").slice(-2).every(o=>o.stopTime!==null&&o.disconnected)).toBe(true);
 audio.suspend();expect(c.state).toBe("closed");expect(audio.status.state).toBe("suspended");
 h.receive(observations[3]!);await audio.enableFromGesture();const newer=contexts[1]!;newer.state="suspended";newer.onstatechange?.();expect(newer.state).toBe("closed");
 await audio.enableFromGesture();vi.advanceTimersByTime(AUDIO_STALE_MS);expect(audio.status.state).toBe("suspended");expect(audio.status.ready).toBe(false);
 h.receive(observations[4]!);await audio.enableFromGesture();const final=contexts.at(-1)!;
 h.receive({type:"observation",observation:{...observations[4]!.observation,outcome:{result:"draw",tick:1531}},events:[]});expect(final.state).toBe("closed");expect(audio.status.state).toBe("ended");
 audio.detach();expect(audio.status.bound).toBe(false);h.receive(first);expect(audio.status.ready).toBe(false);
});
it("迟到resume不能复活已换世界的图，创建失败不会阻塞权威客户端",async()=>{
 const a=harness(),b=harness(),old=new FakeAudioContext(48000,"suspended");let resume!:()=>void;
 old.resume=()=>new Promise<void>(r=>{resume=()=>{old.state="running";r();};});
 const audio=new ArenaAudioSession(()=>{},()=>old.asAudioContext());audio.bind(a.client);a.receive(first);
 const pending=audio.enableFromGesture();audio.bind(b.client);b.receive(first);resume();await pending;
 expect(audio.status.state).toBe("off");expect(old.state).toBe("closed");expect(old.nodesOfType<FakeGainNode>("gain").some(g=>g.disconnected)).toBe(true);audio.detach();
 const bad=new ArenaAudioSession(()=>{},()=>{throw new Error("No audio device");});bad.bind(b.client);b.receive(building);await bad.enableFromGesture();expect(bad.status.state).toBe("error");expect(b.client.observation?.tick).toBe(331);bad.detach();
});
it("应用真实私有配方不创图，替换切断旧图且换世界丢弃包，仍仅消费原已授权本人事件",async()=>{
 const h=harness(),made:FakeAudioContext[]=[];const audio=new ArenaAudioSession(()=>{},()=>{const c=new FakeAudioContext();made.push(c);return c.asAudioContext();},()=>({x:32,y:32,halfWidth:32,halfHeight:32}));audio.bind(h.client);h.receive(first);
 const manifest={schema:"rynvek.audio-manifest.v1",events:{"building.completed":{status:"placeholder",bus:"sfx_world",gain:.17}}};
 audio.setSoundBank(manifest,{"building.completed":{freqStart:733}},"owner.audio@1.0.0");expect(made).toHaveLength(0);
 await audio.enableFromGesture();h.receive(building);expect(made[0]!.nodesOfType<FakeOscillatorNode>("oscillator")[0]!.frequency.events.some(e=>e.value===733)).toBe(true);
 audio.setSoundBank(manifest,{"building.completed":{freqStart:744}},"owner.audio@1.0.1");expect(made[0]!.state).toBe("closed");expect(audio.status.state).toBe("off");expect(made).toHaveLength(1);
 const next=harness();audio.bind(next.client);next.receive(first);await audio.enableFromGesture();next.receive(building);expect(made[1]!.nodesOfType<FakeOscillatorNode>("oscillator")[0]!.frequency.events[0]!.value).toBe(523);audio.detach();
});

it("空间声像来自当前真实源/相机，fog记忆/ghost/已消失源不能提供音源坐标",()=>{
 const plan=planArenaAudio(building.observation,building.events,first.observation);
 const cue=plan.cues.find(c=>c.id==="building.completed")!;expect(cue.position).toEqual({x:17,y:17});
 const near=spatialAudio(cue.position!,{x:9,y:17,halfWidth:32,halfHeight:24})!;
 expect(near.pan).toBe(.25);expect(near.attenuation).toBe(1/(1+.25**2));
 expect(spatialAudio(cue.position!,{x:25,y:17,halfWidth:32,halfHeight:24})!.pan).toBe(-.25);
 expect(spatialAudio(cue.position!,null)).toBeNull();
 const hidden={...building.observation,visibility:building.observation.visibility.map(()=>2)};
 expect(planArenaAudio(hidden,building.events,first.observation).cues).toEqual([]);
 expect(planArenaAudio({...building.observation,buildings:[]},building.events,first.observation).cues).toEqual([]);
 const enemy:RtsEvent={type:"building_completed",id:12,faction:1,kind:"bld.biomass_harvester"};
 expect(planArenaAudio(building.observation,[enemy],first.observation).cues).toEqual([]);
});
