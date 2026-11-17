// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
import {expect,it,vi} from "vitest";
import {AgentRunClient,ControlledAgentAdapter,type AgentRunRecord,type DecisionProvider} from "../src/agent-run.js";
it("取消不遵循abort的注入测试provider也不派发；候选不是工具结果",async()=>{
 let row={run_id:"run",owner_id:"42",revision:0,status:"running",fence:1,tools:["query_visible_clusters"],budget:{turns:2,tool_calls:2,wall_seconds:60},events:[],lease:{controller_id:"",fence:1,expires_at_unix_s:9999999999},memory:{},last_result:null} as unknown as AgentRunRecord;
 const transport=vi.fn(async(path:string,method:string)=>{if(path.endsWith("/context"))return Response.json({run:row,observation:{tick:30},tools:[],external_provider_configured:false,current_host_fence:"1",execution_authorization_valid:true});if(method==="POST")row={...row,revision:row.revision+1};return Response.json(row);});
 const provider:DecisionProvider={id:"noncooperative-explicit-test",boundary:"explicit-local-test",decide:vi.fn(()=>new Promise(()=>{}))};
 const adapter=new ControlledAgentAdapter(new AgentRunClient("42",transport),"run",provider);row.lease!.controller_id=adapter.controllerId;
 const pending=adapter.step();const assertion=expect(pending).rejects.toThrow("取消");await vi.waitFor(()=>expect(provider.decide).toHaveBeenCalledOnce());const supplied=vi.mocked(provider.decide).mock.calls[0]![0];expect(supplied.run).not.toHaveProperty("owner_id");expect(supplied.run).not.toHaveProperty("lease");expect(supplied.run).not.toHaveProperty("control_fence");adapter.stopLocal();await assertion;
 expect(transport.mock.calls.some(([p])=>p.endsWith("/step"))).toBe(false);
});
