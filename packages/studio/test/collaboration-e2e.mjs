// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import assert from 'node:assert/strict';
import {StudioClient,RemoteStudioSession,parameterRegistryOp,PARAMETER_TEMPLATE_EXAMPLE,PARAMETER_RULE_EXAMPLE} from '../dist/index.js';
import {ArenaClient,WebSocketTransport} from '../../client/dist/index.js';
let raw='';for await(const b of process.stdin)raw+=b;const c=JSON.parse(raw);raw='';
const alarm=setTimeout(()=>{console.error('Studio真实协作超时');process.exit(1)},30000);let arena;
try{
 const owner=new StudioClient(c.base,c.owner),editor=new StudioClient(c.base,c.editor),signal=new AbortController().signal;
 if(c.phase==='recover'){
  const v=await editor.view(c.world,signal);assert.equal(v.collaboration.grant_id,c.grant);assert.equal(JSON.parse(v.baseline.params['rules.instances.counter.one']).parameters['param.rate'],7);
  const page=await editor.changes(c.world,0,signal);assert.ok(page.to_revision>0);assert.ok(page.patches.length>0);console.log(JSON.stringify({recovered:true}));
 }else{
  await assert.rejects(()=>editor.view(c.world,signal),/404/);
  const g=await owner.changeEditor(c.world,c.editorId,null,true,signal);assert.equal((await owner.editors(c.world,signal))[0].grant_id,g.grant_id);
  const ws=new WebSocket(`${c.base.replace(/^http/,'ws')}/api/v1/worlds/${c.world}/stream?format=arena-json`);await new Promise((res,rej)=>{ws.addEventListener('open',res,{once:true});ws.addEventListener('error',rej,{once:true})});const rawPort=new WebSocketTransport(ws),port={onMessage:h=>rawPort.onMessage(h),close:()=>rawPort.close(),send:m=>rawPort.send(m.type==='hello'?{...m,capability_token:c.owner}:m)};arena=new ArenaClient(port,{principalId:c.ownerId,kind:'Human',faction:0});await arena.connect();
  let setup=new RemoteStudioSession(owner,await owner.view(c.world,signal),1);const hash=await owner.compile(c.world,PARAMETER_RULE_EXAMPLE,signal);
  await setup.edit([parameterRegistryOp('templates','model.counter',PARAMETER_TEMPLATE_EXAMPLE),{UpdateRuleModule:{module_id:'counter',rule_graph_json:PARAMETER_RULE_EXAMPLE,compiled_rir_blake3:hash}},parameterRegistryOp('instances','counter.one',{template_id:'model.counter',parameters:{'param.rate':3}}),parameterRegistryOp('bindings','counter.step',{module_id:'counter',template_id:'model.counter'})],signal);
  const a=new RemoteStudioSession(owner,await owner.view(c.world,signal),1),b=new RemoteStudioSession(editor,await editor.view(c.world,signal),2),stale=new RemoteStudioSession(owner,await owner.view(c.world,signal),3);
  const rate=n=>parameterRegistryOp('instances','counter.one',{template_id:'model.counter',parameters:{'param.rate':n}});
  await a.edit([rate(5)],signal);await b.sync(signal);assert.equal(JSON.parse(b.document.param('rules.instances.counter.one')).parameters['param.rate'],5);
  await b.edit([rate(7)],signal);await a.sync(signal);assert.equal(await a.undoEdit(signal),null,'不得撤销另一个主体胜出写');assert.equal(JSON.parse(a.document.param('rules.instances.counter.one')).parameters['param.rate'],7);
  await b.undoEdit(signal);await a.sync(signal);assert.equal(JSON.parse(a.document.param('rules.instances.counter.one')).parameters['param.rate'],5);await b.redoEdit(signal);await a.sync(signal);assert.equal(JSON.parse(a.document.param('rules.instances.counter.one')).parameters['param.rate'],7);
  await assert.rejects(()=>stale.edit([rate(9)],signal),/409/);assert.equal(stale.valid,false);
  for(const suffix of ['', '/commits']){const r=await fetch(`${c.base}/api/v1/worlds/${c.world}${suffix}`,{headers:{Authorization:`Bearer ${c.editor}`},redirect:'error'});assert.equal(r.status,404);}
  await assert.rejects(()=>editor.view(c.otherWorld,signal),/404/);await assert.rejects(()=>editor.editors(c.world,signal),/404/);
  const removed=await owner.changeEditor(c.world,c.editorId,g.grant_id,false,signal);await assert.rejects(()=>b.sync(signal),/404/);assert.equal(b.valid,false);
  const renewed=await owner.changeEditor(c.world,c.editorId,removed.grant_id,true,signal);await assert.rejects(()=>editor.view(c.world,signal),/404/,'旧会话不得静默绑定新准入');
  const fresh=new StudioClient(c.base,c.editor),v=await fresh.view(c.world,signal);assert.equal(v.collaboration.grant_id,renewed.grant_id);assert.equal(JSON.parse(v.baseline.params['rules.instances.counter.one']).parameters['param.rate'],7);assert.ok(v.parameter_state.entities['counter.one'].values['state.total']>0);
  console.log(JSON.stringify({grant:renewed.grant_id,oldGrant:g.grant_id,synced:true,undoIsolated:true}));
 }
}finally{arena?.dispose();c.owner='';c.editor='';clearTimeout(alarm);}
