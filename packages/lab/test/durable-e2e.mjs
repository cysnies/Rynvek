// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// Rust定向集成通过stdin传一次性本机测试身份，stdout只回非凭据收据。
import {LabClient} from "../dist/index.js";
import {randomUUID} from "node:crypto";
import assert from "node:assert/strict";
let raw="";for await(const part of process.stdin)raw+=part;const input=JSON.parse(raw);
const client=new LabClient({baseUrl:input.base,token:input.token});
const spec={name:"持久SDK作业",seeds:[57,58],ticks:3,sample_every:1};
if(input.phase==="submit"){
 const one=await client.submit(spec,input.key),two=await client.submit(spec,input.key);assert.equal(one.exp_id,two.exp_id);assert.equal(typeof one.owner_principal_id,"string");
 await assert.rejects(()=>client.submit({...spec,name:"同键不同规格"},input.key),e=>e.status===409);
 assert.equal((await client.listExperiments()).experiments.length,1);
 const cancel={operation_id:randomUUID(),generation:0,action:"cancel"};const stopped=await client.control(one.exp_id,cancel);assert.equal(stopped.status,"cancelled");assert.equal((await client.control(one.exp_id,cancel)).status,"cancelled");
 const retry={operation_id:randomUUID(),generation:0,action:"retry"};assert.equal((await client.control(one.exp_id,retry)).generation,1);assert.equal((await client.control(one.exp_id,retry)).generation,1);
 assert.equal((await client.control(one.exp_id,cancel)).status,"queued");await assert.rejects(()=>client.control(one.exp_id,{...cancel,operation_id:randomUUID()}),e=>e.status===409);
 process.stdout.write(JSON.stringify({id:one.exp_id,owner:one.owner_principal_id,generation:1}));
}else{
 const all=await client.downloadAll(input.id,{intervalMs:25,timeoutMs:15000});const e=await client.getExperiment(input.id);assert.equal(e.status,"completed");assert.equal(Object.keys(e.tables).length,4);
 const found=(await client.listExperiments()).experiments.find(j=>j.exp_id===input.id);assert.equal(found?.status,"completed");
 const page=await client.rows(input.id,"energy");assert.equal(page.cid,all.energy.cid);assert.equal(typeof page.page.rows[0].received_q,"string");
 process.stdout.write(JSON.stringify({id:e.exp_id,status:e.status,attempt:e.attempt,runs_done:e.runs_done,tables:Object.keys(all).sort(),cid:all.metrics.cid}));
}
