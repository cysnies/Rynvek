// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// Real loopback service/Actor; only the audio device is a recording fake. No credentials in output/files/URLs.
import assert from 'node:assert/strict';
import {createPrivateKey,sign} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import ts from 'typescript';
import {PrivateAgentHubClient,signPrivateAudioPackage,agentDigest,pcmSamplePath} from '../dist/index.js';
import {ArenaClient,WebSocketTransport} from '../../client/dist/index.js';
import {parseSoundBank,createTechnicalPcm16Wav} from '../../audio/dist/index.js';
import {FakeAudioContext} from '../../audio/dist/testing.js';
let raw='';for await(const chunk of process.stdin)raw+=chunk;const c=JSON.parse(raw);raw='';
const alarm=setTimeout(()=>{console.error('音频真实消费快验超时');process.exit(1);},15000);
const hub=new PrivateAgentHubClient(c.base,c.principalId,()=>c.token),sdk=hub.audio();let arena=null,audio=null;
try {
 const key=createPrivateKey({key:Buffer.from('302e020100300506032b657004220420'+c.testSeed.toString(16).padStart(2,'0').repeat(32),'hex'),format:'der',type:'pkcs8'});
 const signer=async message=>new Uint8Array(sign(null,message,key));
 const clip=createTechnicalPcm16Wav(),sample=pcmSamplePath(clip);
 const input={id:'owner.audio-library',version:'1.0.0',name:'自有程序化测试配方',license:'CC0-1.0',role:'library',manifest:{schema:'rynvek.audio-manifest.v1',events:{'building.placed':{status:'asset',src:sample,license:'CC0-1.0',credit:'Rynvek self-generated CC0 technical signal',bus:'sfx_world',gain:.17,cooldown_ms:500,max_voices:1}}},recipes:{'building.placed':{freqStart:733,releaseMs:77}},samples:{[sample]:clip}};
 const lib=await sdk.publish(await signPrivateAudioPackage(c.principalId,input,signer));
 const rootInput={...input,id:'owner.audio-main',role:'soundpack',samples:{},manifest:{schema:'rynvek.audio-manifest.v1',events:{}},recipes:{'building.placed':{freqEnd:222}},dependencies:[{id:lib.package_id,version:lib.version,blake3_256:lib.artifact_cid}]};
 const upload=await signPrivateAudioPackage(c.principalId,rootInput,signer),root=await sdk.publish(upload);
 await assert.rejects(()=>sdk.publish(upload));
 const found=await sdk.list();assert.equal(found.length,2);
 const loaded=await sdk.load(found.find(m=>m.profile.role==='soundpack'));
 await assert.rejects(()=>sdk.download({...root,signature:'00'.repeat(64)}));
 await assert.rejects(()=>sdk.download({...root,artifact_cid:'00'.repeat(32)}));
 const downloaded=await sdk.download(root);assert.equal(agentDigest(downloaded),root.artifact_cid);
 const bank=parseSoundBank(loaded.manifest,loaded.recipes,loaded.samples);assert.equal(bank.recipes['building.placed'].freqStart,733);assert.equal(bank.recipes['building.placed'].freqEnd,222);assert.equal(bank.manifest.events['building.placed'].gain,.17);
 // Transpile the ACTUAL ArenaAudioSession, replacing its sole runtime bare import with the built audio package.
 const source=readFileSync(resolve('apps/web-platform/src/arena-audio.ts'),'utf8');
 const js=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText.replace('"@rynvek/audio"',JSON.stringify(pathToFileURL(resolve('packages/audio/dist/index.js')).href));
 const {ArenaAudioSession}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
 const contexts=[];audio=new ArenaAudioSession(()=>{},()=>{const ctx=new FakeAudioContext(48000,'suspended');contexts.push(ctx);return ctx.asAudioContext();},()=>({x:c.origin[0]-8,y:c.origin[1],halfWidth:32,halfHeight:24}));
 const ws=new WebSocket(`${c.base.replace(/^http/,'ws')}/api/v1/worlds/${c.world}/stream?format=arena-json`);
 await new Promise((ok,bad)=>{ws.addEventListener('open',ok,{once:true});ws.addEventListener('error',bad,{once:true});});
 const rawPort=new WebSocketTransport(ws),port={onMessage:h=>rawPort.onMessage(h),close:()=>rawPort.close(),send:m=>rawPort.send(m.type==='hello'?{...m,capability_token:c.token}:m)};
 arena=new ArenaClient(port,{principalId:c.principalId,kind:'Human',faction:0});audio.bind(arena);await arena.connect();await arena.acquireLease('Sovereign');if(!arena.observation)await arena.nextObservation();
 audio.setSoundBank(loaded.manifest,loaded.recipes,root.package_id,loaded.samples);assert.equal(contexts.length,0); // selecting never unlocks audio.
 await audio.enableFromGesture();assert.equal(audio.status.state,'running');
 const ack=await arena.command({type:'place_building',kind:'bld.signal_beacon',origin:c.origin});assert.equal(ack.type,'building_placed');
 for(let i=0;i<30&&!contexts[0].nodesOfType('bufferSource').length;i++)await arena.nextObservation();
 const sampleSource=contexts[0].nodesOfType('bufferSource')[0];assert.ok(sampleSource,'真实building_placed必须进入下载的PCM采样');
 assert.equal(sampleSource.buffer.sampleRate,16000);assert.equal(sampleSource.buffer.length,1280);
 assert.ok(sampleSource.buffer.getChannelData(0).some(v=>v!==0),'真实非零自生PCM，不是空音频');
 assert.ok(contexts[0].nodesOfType('stereoPanner').some(n=>n.pan.events.some(e=>e.value===.25)),'真实新建筑origin与受控相机相距8/32，pan=.25');
 assert.ok(contexts[0].nodesOfType('gain').some(n=>n.gain.events.some(e=>Math.abs(e.value-(.65*.17/(1+.25**2)))<1e-12)),'距离衰减/下载增益实际进入原混音图');
 sampleSource.onended?.();assert.equal(sampleSource.disconnected,true);
 const usedTick=arena.tick;audio.mute();assert.equal(contexts[0].state,'closed');
 await sdk.revoke(lib);await assert.rejects(()=>sdk.load(root));
 const newInput={...rootInput,version:'1.0.1'};await assert.rejects(async()=>sdk.publish(await signPrivateAudioPackage(c.principalId,newInput,signer)));
 await sdk.revoke(root);await assert.rejects(()=>sdk.download({...root,revoked:false}));
 console.log(JSON.stringify({root,lib,downloaded:true,actualArenaTick:usedTick,ack:ack.type,sampleFrames:1280,sampleRate:16000,actualPan:.25,actualTrim:.17,libraryRevocationEnforced:true}));
} finally {audio?.detach();arena?.dispose();hub.destroy();c.token='';clearTimeout(alarm);}
