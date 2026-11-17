// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import assert from 'node:assert/strict';
import {ArenaClient,WebSocketTransport} from '../../client/dist/index.js';
import {StudioClient,RemoteStudioSession,parameterRegistryOp,PARAMETER_TEMPLATE_EXAMPLE,PARAMETER_RULE_EXAMPLE} from '../dist/index.js';
let raw='';for await(const chunk of process.stdin)raw+=chunk;const c=JSON.parse(raw);raw='';const alarm=setTimeout(()=>{console.error('参数registry实际SDK消费超时');process.exit(1);},25000);
let arena;
try{const ws=new WebSocket(`${c.base.replace(/^http/,'ws')}/api/v1/worlds/${c.world}/stream?format=arena-json`);await new Promise((resolve,reject)=>{ws.addEventListener('open',resolve,{once:true});ws.addEventListener('error',reject,{once:true});});const rawPort=new WebSocketTransport(ws),port={onMessage:h=>rawPort.onMessage(h),close:()=>rawPort.close(),send:m=>rawPort.send(m.type==='hello'?{...m,capability_token:c.token}:m)};arena=new ArenaClient(port,{principalId:c.principalId,kind:'Human',faction:0});await arena.connect();
const client=new StudioClient(c.base,c.token),signal=new AbortController().signal;const view=await client.view(c.world,signal);const s=new RemoteStudioSession(client,view,3);const hash=await client.compile(c.world,PARAMETER_RULE_EXAMPLE,signal);
const ack=await s.edit([parameterRegistryOp('templates','model.counter',PARAMETER_TEMPLATE_EXAMPLE),{UpdateRuleModule:{module_id:'counter',rule_graph_json:PARAMETER_RULE_EXAMPLE,compiled_rir_blake3:hash}},parameterRegistryOp('instances','counter.one',{template_id:'model.counter',parameters:{'param.rate':3}}),parameterRegistryOp('bindings','counter.step',{module_id:'counter',template_id:'model.counter'})],signal);
assert.ok(s.document.hasTemplate('parameter/model.counter'));let actual=await client.view(c.world,signal);assert.ok(actual.parameter_state.entities['counter.one'].values['state.total']>=3);const serial=actual.parameter_state.entities['counter.one'].serial;
await s.edit([parameterRegistryOp('instances','counter.one',{template_id:'model.counter',parameters:{'param.rate':7}})],signal);actual=await client.view(c.world,signal);assert.equal(actual.parameter_state.entities['counter.one'].serial,serial);const beforeUndo=actual.parameter_state.entities['counter.one'].values['state.total'];assert.equal(JSON.parse(actual.baseline.params['rules.instances.counter.one']).parameters['param.rate'],7);
await s.undoEdit(signal);actual=await client.view(c.world,signal);assert.equal(JSON.parse(actual.baseline.params['rules.instances.counter.one']).parameters['param.rate'],3);assert.ok(actual.parameter_state.entities['counter.one'].values['state.total']>beforeUndo,'撤销定义不回滚动态态/仿真时间');
await s.redoEdit(signal);actual=await client.view(c.world,signal);assert.equal(JSON.parse(actual.baseline.params['rules.instances.counter.one']).parameters['param.rate'],7);assert.equal(actual.parameter_state.entities['counter.one'].serial,serial);
console.log(JSON.stringify({ack,actual,templateDiscovered:s.document.hasTemplate('parameter/model.counter'),stateContinues:true}));
}finally{arena?.dispose();c.token='';clearTimeout(alarm);}
