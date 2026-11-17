// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// 真回环测试，身份仅stdin；不接外部模型，不输出凭据。
import assert from 'node:assert/strict';
import {PrivateAgentHubClient,PrivatePackageProvider,ScriptedTestProvider,ControlledAgentAdapter} from '../dist/index.js';
import {ArenaClient,WebSocketTransport} from '../../client/dist/index.js';
let raw='';for await(const p of process.stdin)raw+=p;const c=JSON.parse(raw);raw='';
const timeout=setTimeout(()=>{console.error('受控运行真实回环超时');process.exit(1);},20000);let arena;
let lastProposal=null;const fetcher=async(url,init)=>{if(String(url).endsWith('/step'))lastProposal=JSON.parse(String(init.body));return fetch(url,init);};
const hub=new PrivateAgentHubClient(c.base,c.principalId,()=>c.token,fetcher);
try{
 const meta=await hub.get('run.agent','1.0.0');const api=hub.runs();
 const ws=new WebSocket(`${c.base.replace(/^http/,'ws')}/api/v1/worlds/${c.world}/stream?format=arena-json`);
 await new Promise((resolve,reject)=>{ws.addEventListener('open',resolve,{once:true});ws.addEventListener('error',reject,{once:true});});
 const rawPort=new WebSocketTransport(ws);const port={onMessage:h=>rawPort.onMessage(h),close:()=>rawPort.close(),send:m=>rawPort.send(m.type==='hello'?{...m,capability_token:c.token}:m)};
 arena=new ArenaClient(port,{principalId:c.principalId,kind:'Human',faction:0});await arena.connect();await arena.acquireLease('Sovereign');
 const wait=async(tick)=>{while(arena.tick<tick)await arena.nextObservation();};
 const make=()=>({submission_id:crypto.randomUUID(),world_id:c.world,package_id:meta.package_id,version:meta.version,artifact_cid:meta.artifact_cid,control_fence:String(arena.lease.fencing_token),tools:[...meta.profile.tools],budget:{turns:2,tool_calls:2,wall_seconds:300},consent:'private-pve-tools/1'});
 const scripted=new ScriptedTestProvider([{message:'明确本地测试provider查询',memory:{phase:1},call:{name:'query_visible_clusters',arguments:{include_enemy:false}}},{message:'明确本地测试provider实际战略工具',memory:{phase:2},call:{name:'issue_strategic_intent',arguments:{unit_archetype:'all',order_kind:'HoldPosition',target_world_x:1,target_world_y:1}}}]);
 if(c.phase==='start'){
  const body=make();let r=await api.create(body);assert.equal((await api.create(body)).run_id,r.run_id);await assert.rejects(()=>api.create({...body,budget:{...body.budget,turns:3}}));
  const events=[];const adapter=new ControlledAgentAdapter(api,r.run_id,new PrivatePackageProvider(hub,meta),e=>events.push(e));r=await adapter.run();assert.equal(r.status,'completed');assert.equal(r.calls_used,2);assert.equal(r.memory.saw_real_result,true);assert.equal(r.memory.io,true);
  const results=r.events.filter(e=>e.kind==='tool_execution_end');assert.equal(results.length,2);assert.equal(results[0].data.outcome.kind,'broker_result');assert.equal(results[1].data.outcome.response.error,undefined);assert.ok(results[1].data.outcome.response.result.ack.units>0);assert.ok(events.some(e=>e.kind==='tool_dispatch_intent'));
  const firstId=r.run_id;await wait(r.last_result.response.tick+30);
  const second=await api.create(make());const testAdapter=new ControlledAgentAdapter(api,second.run_id,scripted);let partial=await testAdapter.step();assert.equal(partial.calls_used,1);assert.equal(partial.last_result.kind,'broker_result');assert.equal(partial.last_result.response.error,undefined);
  const cancel={operation_id:crypto.randomUUID(),expected_revision:partial.revision,action:'cancel'};partial=await api.control(second.run_id,cancel);assert.equal(partial.status,'cancelled');assert.equal((await api.control(second.run_id,cancel)).status,'cancelled');
  process.stdout.write(JSON.stringify({first:firstId,resumable:partial.run_id,calls:partial.calls_used,status:partial.status,events:partial.events.length}));
 }else{
  let r=(await api.list()).find(r=>r.run_id===c.runId);assert.equal(r.status,'cancelled');assert.equal(r.calls_used,1);assert.equal(r.memory.phase,1);await wait(r.last_result.response.tick+30);
  const context=await api.context(r.run_id);r=await api.control(r.run_id,{operation_id:crypto.randomUUID(),expected_revision:r.revision,action:'resume',host_fence:context.current_host_fence,control_fence:String(arena.lease.fencing_token)});
  r=await new ControlledAgentAdapter(api,r.run_id,scripted).step();assert.equal(r.status,'completed');assert.equal(r.calls_used,2);assert.equal(r.turns_used,2);assert.equal(r.last_result.kind,'broker_result');assert.equal(r.last_result.response.error,undefined);assert.ok(r.last_result.response.result.ack.units>0);
  const replay=await api.step(r.run_id,lastProposal);assert.equal(replay.revision,r.revision);assert.equal(replay.calls_used,2);assert.equal(replay.pending,null);
  process.stdout.write(JSON.stringify({id:r.run_id,status:r.status,calls:r.calls_used,turns:r.turns_used,real_units:r.last_result.response.result.ack.units,events:r.events.length}));
 }
}finally{arena?.dispose();hub.destroy();c.token='';clearTimeout(timeout);}
