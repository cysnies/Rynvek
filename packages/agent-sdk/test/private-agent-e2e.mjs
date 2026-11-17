// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// 由Rust loopback夹具启动；测试身份只经stdin传递，不写文件/URL/日志，不作为包输入。
import { PrivateAgentHubClient,PrivateAgentRunner,agentDigest } from '../dist/index.js';
import { ArenaClient,WebSocketTransport } from '../../client/dist/index.js';
let input='';for await(const part of process.stdin)input+=part;
const c=JSON.parse(input);input='';
const deadline=setTimeout(()=>{console.error('真实SDK入口超时，未自动重试');process.exit(1);},12000);
let client=null;const hub=new PrivateAgentHubClient(c.base,c.principalId,()=>c.token);
try {
 if(c.mode==='publish')console.log(JSON.stringify(await hub.publish(c.upload)));
 else {
  const list=await hub.list();const meta=list.find(m=>m.package_id===c.id&&m.version==='1.0.0');if(!meta)throw new Error('真实私有发现缺少目标');
  const downloaded=await hub.download(meta);if(agentDigest(downloaded)!==meta.artifact_cid)throw new Error('真实制品下载不一致');
  const ws=new WebSocket(`${c.base.replace(/^http/,'ws')}/api/v1/worlds/${c.world}/stream?format=arena-json`);
  await new Promise((resolve,reject)=>{ws.addEventListener('open',resolve,{once:true});ws.addEventListener('error',reject,{once:true});});
  const raw=new WebSocketTransport(ws);
  const port={onMessage:handler=>raw.onMessage(handler),close:()=>raw.close(),send:message=>raw.send(message.type==='hello'?{...message,capability_token:c.token}:message)};
  client=new ArenaClient(port,{principalId:c.principalId,kind:'Human',faction:0});await client.connect();await client.acquireLease('Sovereign');
  const runner=new PrivateAgentRunner(hub,meta,c.world,client);try{const step=await runner.step();while(client.tick<61)await client.nextObservation();const mutation=await runner.step();while(client.tick<mutation.receipt.tick)await client.nextObservation();console.log(JSON.stringify({downloaded:true,...step,mutation,orders:client.observation.units.map(u=>u.order)}));}finally{runner.stop();}
 }
} finally {client?.dispose();hub.destroy();c.token='';clearTimeout(deadline);}
