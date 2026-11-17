// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from 'vitest';
import {AuthoringDocument,RemoteStudioSession,StudioClient,type WorldPatch,type AuthoringView} from '../src/index.js';
const patch=(id:number,wall:number):WorldPatch=>({patch_id:new Array(16).fill(id),base_authoring_revision:0,hlc:{wall_time_ms:wall,logical_counter:0,author_node_id:id},safety_class:'EpochBoundarySafe',operations:[{SetWorldParam:{key_path:'rules.instances.x',new_value_json:String(id)}}]});
it('在线提交顺序压过伪未来HLC，异序消费仍收敛；不改变离线CRDT原规则',()=>{
 const a=new AuthoringDocument({width:1,height:1,terrain:new Uint8Array(1)}),b=new AuthoringDocument({width:1,height:1,terrain:new Uint8Array(1)});const one=patch(1,999999999999),two=patch(2,1);
 a.integrate(one);a.integrateCommitted(one,5);a.acknowledgeRevision(5);a.integrateCommitted(two,6);a.acknowledgeRevision(6);b.integrateCommitted(two,6);b.integrateCommitted(one,5);b.acknowledgeRevision(6);
 expect(a.param('rules.instances.x')).toBe('2');expect(a.canonicalState()).toBe(b.canonicalState());expect(a.inverseOf(one.patch_id).operations).toEqual([]);expect(a.revision).toBe(6);
});
it('本地ack后补消费先前日志不虚增revision，撤权使会话失效且无自动重试',async()=>{
 const basis:AuthoringView={world_id:'w',revision:3,tick:3,state_root:'ab'.repeat(32),rule_fault:null,parameter_state:{next_serial:0,entities:{},fault:null},collaboration:{principal_id:'2',role:'studio-editor',grant_id:'grant-a'},baseline:{width:1,height:1,terrain:[0]}};let local:WorldPatch|undefined,calls=0;let revoked=false;
 const client=new StudioClient('http://srv','private',async(url,init)=>{calls++;if(revoked)return Response.json({message:'准入不存在'},{status:404});if(String(url).endsWith('/authoring'))return Response.json(basis);expect(new Headers(init?.headers).get('X-Rynvek-Studio-Grant')).toBe('grant-a');if(init?.method==='POST'){local=JSON.parse(String(init.body));return Response.json({status:'applied',world_id:'w',world_revision:8,effective_tick:8,state_root:'cd'.repeat(32)})}return Response.json({world_id:'w',from_revision:3,to_revision:9,tick:9,state_root:'ef'.repeat(32),patches:[{revision:8,patch:local}],has_more:false})});
 const signal=new AbortController().signal,s=new RemoteStudioSession(client,await client.view('w',signal),2);await s.edit(patch(1,1).operations,signal);await s.sync(signal);expect(s.document.revision).toBe(9);expect(s.stateRoot).toBe('ef'.repeat(32));revoked=true;await expect(s.sync(signal)).rejects.toThrow('404');const n=calls;await expect(s.edit(patch(2,2).operations,signal)).rejects.toThrow();expect(calls).toBe(n);
});
