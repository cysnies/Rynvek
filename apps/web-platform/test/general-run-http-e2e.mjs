// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// 真实Node SDK/Web控制器→服务器后台；不是浏览器、PG或OS断电实测。
import assert from "node:assert/strict";
import {GeneralWorldSession} from "../dist/general-remote.js";
import {GeneralRunClient,GENERAL_RUN_CONSENT} from "@rynvek/agent-sdk";
let input="";for await(const b of process.stdin)input+=b;const p=JSON.parse(input);input="";
let c;const calls=[];const timer=setTimeout(()=>{console.error("耐久客户端HTTP闭环超时");process.exit(1)},25000);
const transport=(url,o)=>{calls.push({path:new URL(String(url)).pathname,method:o?.method,body:o?.body?JSON.parse(String(o.body)):null});return fetch(url,o)};
async function open(){return GeneralWorldSession.open(p.base,p.token,transport)}
async function prepare(){await c.connect();await c.acquire();return {submission_id:uuid(),expected_revision:c.observation.commit.revision,expected_root:c.observation.commit.state_root,ticks:3,wall_seconds:120,interval_ms:200,max_energy_q:"1000000000",consent:GENERAL_RUN_CONSENT};}
function uuid(){const b=crypto.getRandomValues(new Uint8Array(16));let t=BigInt(Date.now());for(let i=5;i>=0;i--){b[i]=Number(t&255n);t>>=8n;}b[6]=(b[6]&15)|112;b[8]=(b[8]&63)|128;const s=Buffer.from(b).toString("hex");return `${s.slice(0,8)}-${s.slice(8,12)}-${s.slice(12,16)}-${s.slice(16,20)}-${s.slice(20)}`;}
async function completed(read){for(let i=0;i<150;i++){const j=await read();if(j.status==="completed")return j;assert.ok(!["blocked","expired"].includes(j.status),JSON.stringify(j));await new Promise(r=>setTimeout(r,30));}throw Error("有界等待耗尽");}
try{
 c=await open();await c.select(p.world);const genesis=c.observation;const spec=await prepare();
 const sdk=new GeneralRunClient(c.identity.principal_id,(path,method,body,signal)=>transport(p.base+path,{method,body:body===undefined?undefined:JSON.stringify(body),signal,headers:{Authorization:`Bearer ${p.token}`,"Content-Type":"application/json"},redirect:"error",cache:"no-store",credentials:"omit"}));
 const first=await sdk.start(p.world,c.hostFence,c.controlFence,spec);c.destroy();c=undefined;
 // 客户端断开后只有GET：推进实为后台而非客户端循环。
 const end=await completed(()=>sdk.get(p.world,first.id));assert.equal(end.done,3);assert.ok(end.energy_used_q>0);assert.equal((await sdk.lookup(p.world,spec.submission_id)).id,first.id);
 c=await open();await c.select(p.world);assert.equal(c.observation.commit.state_root,end.last_root);
 const next={...await prepare(),ticks:128,interval_ms:500};let job=await c.startDurable(next);assert.equal(c.controlFence,undefined);assert.equal(c.observation,undefined);assert.equal(job.storage_backend,"memory");const originalDeadline=job.deadline_s;
 for(const action of ["pause","resume","pause"]){job=await c.durableControl(job.id,{operation_id:uuid(),generation:job.generation,action});assert.equal(job.deadline_s,originalDeadline);if(action==="pause"){const before=job.done;await new Promise(r=>setTimeout(r,550));assert.equal((await c.durableStatus(job.id)).done,before);}}
 const deadline=job.deadline_s,done=job.done,energy=job.energy_used_q;c.destroy();c=await open();await c.select(p.world);assert.equal(c.controlFence,undefined);job=await c.durableStatus(next.submission_id,true);assert.equal(job.done,done);assert.equal(job.energy_used_q,energy);job=await c.durableControl(job.id,{operation_id:uuid(),generation:job.generation,action:"cancel"});assert.equal(job.status,"cancelled");assert.equal(job.deadline_s,deadline);
 const final=await c.startDurable(await prepare());const finish=await completed(()=>c.durableStatus(final.id));await c.refresh();assert.equal(c.observation.commit.state_root,finish.last_root);assert.equal(c.observation.life.universe_energy_q,genesis.life.universe_energy_q);assert.ok(finish.energy_used_q>0);
 assert.equal(calls.filter(x=>x.body?.action==="tick").length,0);assert.equal(calls.filter(x=>x.body?.action==="start_run").length,3);
 console.log(JSON.stringify({tick:c.observation.commit.tick,root:finish.last_root,sdkPaid:end.energy_used_q,webPaid:finish.energy_used_q,storage:finish.storage_backend,requests:calls.length}));
}finally{c?.destroy();p.token="";clearTimeout(timer);}
