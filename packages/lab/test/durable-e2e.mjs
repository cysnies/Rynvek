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
}else if(input.phase==="download"){
 const all=await client.downloadAll(input.id,{intervalMs:25,timeoutMs:15000});const e=await client.getExperiment(input.id);assert.equal(e.status,"completed");assert.equal(Object.keys(e.tables).length,4);
 const found=(await client.listExperiments()).experiments.find(j=>j.exp_id===input.id);assert.equal(found?.status,"completed");
 const page=await client.rows(input.id,"energy");assert.equal(page.cid,all.energy.cid);assert.equal(typeof page.page.rows[0].received_q,"string");
 process.stdout.write(JSON.stringify({id:e.exp_id,status:e.status,attempt:e.attempt,runs_done:e.runs_done,tables:Object.keys(all).sort(),cid:all.metrics.cid}));
}
else if(input.phase==="parameter-submit"){
 const {StudioClient,RemoteStudioSession,parameterRegistryOp,PARAMETER_RULE_EXAMPLE,PARAMETER_TEMPLATE_EXAMPLE}=await import("../../studio/dist/index.js");
 const {parameterSweep}=await import("../dist/index.js");
 const studio=new StudioClient(input.base,input.token),signal=new AbortController().signal;
 const world=await studio.create("私有真实参数Lab消费",57,undefined,signal);
 const remote=new RemoteStudioSession(studio,await studio.view(world,signal),38);
 const digest=await studio.compile(world,PARAMETER_RULE_EXAMPLE,signal);
 await remote.edit([{UpdateRuleModule:{module_id:"counter",rule_graph_json:PARAMETER_RULE_EXAMPLE,compiled_rir_blake3:digest}},
   parameterRegistryOp("templates","model.counter",PARAMETER_TEMPLATE_EXAMPLE),
   parameterRegistryOp("instances","counter.one",{template_id:"model.counter",parameters:{}}),
   parameterRegistryOp("bindings","counter.step",{module_id:"counter",template_id:"model.counter"})],signal);
 const capture=await client.captureParameterModel(world,signal);assert.equal(capture.world_id,world);assert.equal(typeof capture.revision,"string");
 const spec={name:"Studio原模型持久真实扫描",seeds:[57],ticks:4,sample_every:2,
   variants:parameterSweep(capture.model,"counter.one","param.rate",[1,3])};
 const one=await client.submit(spec,input.key);assert.equal((await client.submit(spec,input.key)).exp_id,one.exp_id);
 assert.equal(one.kernel_profile,"rynvek-lab-kernel/5");
 await assert.rejects(()=>client.submit({...spec,variants:spec.variants.map((v,i)=>i? v:{...v,model:{...v.model,outputs:[...v.model.outputs,...v.model.outputs]}})},randomUUID()));
 process.stdout.write(JSON.stringify({id:one.exp_id,world,digest:capture.model_digest}));
}else if(input.phase==="parameter-download"){
 const {parameterMetrics}=await import("../dist/index.js");
 const all=await client.downloadAll(input.id,{intervalMs:25,timeoutMs:15000});
 const [runs,metrics,lineage,energy]=await Promise.all(["runs","metrics","lineage","energy"].map(t=>client.rows(input.id,t)));
 assert.equal(lineage.page.total_rows,0);assert(energy.page.rows.every(r=>r.enabled===false&&r.live_bodies===0));
 assert(runs.page.rows.every(r=>r.mode==="parameters"&&r.players===0&&r.end_reason==="tick_budget"&&r.parameter_fault===null&&r.natural_births===0));
 assert.notEqual(runs.page.rows[0].model_digest,runs.page.rows[1].model_digest);
 const trajectory=parameterMetrics(metrics.page.rows);assert.deepEqual(trajectory.map(r=>[r.run,r.tick.toString(),r.raw]),[[0,"2",2],[0,"4",4],[1,"2",6],[1,"4",12]]);
 assert(metrics.page.rows.every(r=>r.score_milli===null&&r.units===null&&r.biomass_milli===null));
 assert.equal(all.metrics.cid,metrics.cid);assert.equal(runs.page.rows[0].final_state_root,metrics.page.rows[1].state_root);
 process.stdout.write(JSON.stringify({id:input.id,tables:Object.keys(all).sort(),samples:trajectory.length,lineage:lineage.page.total_rows,energy_enabled:false}));
}
