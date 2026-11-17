// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
/** Strict little-endian PCM16 mono RIFF. Never invokes browser codecs or fetches a URL. */
export interface PcmClip {readonly rate:number;readonly frames:Float32Array}
export const PCM_PATH=/^samples\/[0-9a-f]{64}\.wav$/;
export function decodePcm16Wav(b:Uint8Array):PcmClip {
 if(!(b instanceof Uint8Array)||b.length<46||b.length>192044)throw new Error("PCM尺寸越界");
 const v=new DataView(b.buffer,b.byteOffset,b.byteLength),u16=(i:number)=>v.getUint16(i,true),u32=(i:number)=>v.getUint32(i,true),tag=(i:number,n:number)=>String.fromCharCode(...b.subarray(i,i+n));
 const rate=u32(24),count=(b.length-44)/2;
 if(tag(0,4)!=="RIFF"||tag(8,8)!=="WAVEfmt "||tag(36,4)!=="data"||u32(4)!==b.length-8||u32(16)!==16||u16(20)!==1||u16(22)!==1||![8000,16000,22050,24000,44100,48000].includes(rate)||u32(28)!==rate*2||u16(32)!==2||u16(34)!==16||u32(40)!==b.length-44||!Number.isInteger(count)||count>rate*2)throw new Error("须规范PCM16 mono WAV，最多2秒且无额外块");
 const frames=new Float32Array(count);for(let i=0;i<count;i++)frames[i]=v.getInt16(44+i*2,true)/32768;
 return {rate,frames};
}
export function parsePcmSamples(raw:unknown):Readonly<Record<string,PcmClip>> {
 if(!raw||typeof raw!=="object"||Array.isArray(raw)||Object.keys(raw).length>8)throw new Error("PCM表须至多8项");
 const entries=Object.entries(raw);let encoded=0;
 for(const [path,text]of entries){if(!PCM_PATH.test(path)||typeof text!=="string")throw new Error("PCM路径/类型非法");encoded+=text.length;if(text.length>256060||encoded>524288)throw new Error("PCM解码前资源超限");}
 let frames=0;const clips:Record<string,PcmClip>=Object.create(null);
 for(const [path,text]of entries){const b=atob(text as string);const clip=decodePcm16Wav(Uint8Array.from(b,c=>c.charCodeAt(0)));frames+=clip.frames.length;if(frames>192000)throw new Error("PCM总帧数超限");clips[path]=clip;}
 return Object.freeze(clips);
}
/** Self-generated technical calibration chirp, not music/third-party timbre. Output explicitly CC0-1.0. */
export function createTechnicalPcm16Wav(frequency=733,seconds=.08,rate=16000):Uint8Array {
 if(!Number.isFinite(frequency)||frequency<20||frequency>rate/4||!Number.isFinite(seconds)||seconds<=0||seconds>2||![8000,16000,22050,24000,44100,48000].includes(rate))throw new Error("测试信号参数越界");
 const n=Math.max(1,Math.floor(rate*seconds)),bytes=new Uint8Array(44+n*2),v=new DataView(bytes.buffer),tag=(offset:number,s:string)=>bytes.set(new TextEncoder().encode(s),offset);
 tag(0,"RIFF");tag(8,"WAVEfmt ");tag(36,"data");v.setUint32(4,bytes.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,rate,true);v.setUint32(28,rate*2,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,n*2,true);
 for(let i=0;i<n;i++)v.setInt16(44+i*2,Math.round(32767*.2*Math.sin(2*Math.PI*frequency*i/rate)*Math.sin(Math.PI*i/Math.max(1,n-1))**2),true);
 return bytes;
}
