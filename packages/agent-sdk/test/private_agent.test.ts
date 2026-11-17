// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
import {readFileSync} from "node:fs";
import {generateKeyPairSync,sign} from "node:crypto";
import {expect,it,vi} from "vitest";
import {buildPrivateAgentPackage,agentDigest,privateAgentSigningMessage,signPrivateAgentPackage,PrivateAgentHubClient,PrivateAgentRunner,type AgentPackageInput,type PrivateAgentVersion,type AgentDecision} from "../src/index.js";
const fixture=JSON.parse(readFileSync(new URL("../../../tests/golden_vectors/private-agent-v1.json",import.meta.url),"utf8")) as {input:AgentPackageInput;artifact_b64:string;artifact_cid:string;signing_message_hex_for_42:string};
const world="a0000000-0000-4000-8000-000000000001";
function data(){
 const bytes=buildPrivateAgentPackage(fixture.input);const keys=generateKeyPairSync("ed25519");
 const signature=sign(null,privateAgentSigningMessage("42",fixture.input.id,fixture.input.version,bytes),keys.privateKey).toString("hex");
 const meta:PrivateAgentVersion={owner_id:"42",package_id:fixture.input.id,version:"1.0.0",artifact_cid:agentDigest(bytes),publisher_key:keys.publicKey.export({type:"spki",format:"der"}).subarray(-32).toString("hex"),signature,profile:{schema:"rynvek.agent.v1",runtime:"quickjs-tools/1",role:"agent",entrypoint:"main.js",tools:["query_visible_clusters"]},dependencies:[],verification:{trust_level:3},revoked:false,visibility:"Private"};
 const decision:AgentDecision={package_id:meta.package_id,version:meta.version,artifact_cid:meta.artifact_cid,world_id:world,principal_id:"42",faction:0,tick:30,output:{call:{name:"query_visible_clusters",arguments:{include_enemy:true}},memory:{step:1}},gas_used:1,executed:false};
 const arena={identity:{principalId:"42",faction:0},welcome:{mode:"PvE_Survival"},lease:{fencing_token:7},callTool:vi.fn(async()=>({tool:"query_visible_clusters",tick:31,result:{clusters:[]}}))};
 return {bytes,keys,meta,decision,arena};
}
it("TS容器与Rust共用逐字节夹具；外部签名回调只接收owner绑定消息",async()=>{
 const {bytes,keys}=data();expect(Buffer.from(bytes).toString("base64")).toBe(fixture.artifact_b64);expect(agentDigest(bytes)).toBe(fixture.artifact_cid);expect(Buffer.from(privateAgentSigningMessage("42",fixture.input.id,"1.0.0",bytes)).toString("hex")).toBe(fixture.signing_message_hex_for_42);
 const signer=vi.fn(async(message:Uint8Array)=>new Uint8Array(sign(null,message,keys.privateKey)));const upload=await signPrivateAgentPackage("42",fixture.input,signer);expect(signer).toHaveBeenCalledTimes(1);expect(upload.artifact_cid).toBe(fixture.artifact_cid);expect(Object.keys(upload).sort()).toEqual(["artifact_b64","artifact_cid","owner_id","package_id","signature","version"]);
 expect(()=>buildPrivateAgentPackage({...fixture.input,source:"const password='test-only';"})).toThrow("凭据");expect(()=>buildPrivateAgentPackage({...fixture.input,version:"1.0.0+old-format"})).toThrow("非法");
});
it("私有下载验BLAKE3和owner域Ed25519；只有头带凭据，禁止重定向且断开失效",async()=>{
 const {bytes,meta}=data();const fetcher=vi.fn(async(_url:unknown,_init?:RequestInit)=>new Response(new Uint8Array(bytes).buffer));const hub=new PrivateAgentHubClient("https://private.example","42",()=>"test-memory",fetcher);
 expect(await hub.download(meta)).toEqual(bytes);const [url,init]=fetcher.mock.calls[0]!;expect(url).not.toContain("test-memory");expect(init?.headers).toMatchObject({Authorization:"Bearer test-memory"});expect(init?.redirect).toBe("error");expect(init?.cache).toBe("no-store");
 await expect(hub.download({...meta,signature:"00".repeat(64)})).rejects.toThrow("验签");const corrupted=new Uint8Array(bytes);corrupted[corrupted.length-1]=corrupted[corrupted.length-1]!^1;fetcher.mockImplementation(async()=>new Response(corrupted.buffer));await expect(hub.download(meta)).rejects.toThrow("BLAKE3");hub.destroy();await expect(hub.list()).rejects.toThrow("断开");
});
it("同一runner只发一个本人当前租约工具，服务端拒绝不能报accepted或暗中循环",async()=>{
 const {meta,decision,arena}=data();const fetcher=vi.fn(async()=>Response.json(decision));const hub=new PrivateAgentHubClient("http://local","42",()=>"test",fetcher);const runner=new PrivateAgentRunner(hub,meta,world,arena);
 const result=await runner.step();expect(result.dispatched).toBe(true);expect(result.accepted).toBe(true);expect(arena.callTool).toHaveBeenCalledExactlyOnceWith("query_visible_clusters",{include_enemy:true});expect(fetcher).toHaveBeenCalledTimes(1);const denied=new PrivateAgentRunner(hub,meta,world,{...arena,callTool:async()=>({tool:"query_visible_clusters",tick:32,error:{code:0x8001,name:"Denied",message:"refused"}})});expect((await denied.step()).accepted).toBe(false);denied.stop();runner.stop();await expect(runner.step()).rejects.toThrow("停止");
});
it("HTTP迟到候选在stop/换fence后丢弃，不执行未知回执也不重复提交",async()=>{
 const {meta,decision,arena}=data();let finish!:(r:Response)=>void;const fetcher=vi.fn(()=>new Promise<Response>(r=>{finish=r;}));const hub=new PrivateAgentHubClient("http://local","42",()=>"test",fetcher);const runner=new PrivateAgentRunner(hub,meta,world,arena);
 const first=runner.step();await expect(runner.step()).rejects.toThrow("尚未结束");runner.stop();finish(Response.json(decision));await expect(first).rejects.toThrow("停止");expect(arena.callTool).not.toHaveBeenCalled();
 const next=new PrivateAgentRunner(hub,meta,world,arena);const waiting=next.step();arena.lease.fencing_token=8;finish(Response.json(decision));await expect(waiting).rejects.toThrow("租约已改变");expect(arena.callTool).not.toHaveBeenCalled();next.stop();
});
it("私有metadata owner或Agent候选身份/declared tools不符时不发送工具",async()=>{
 const {meta,decision,arena}=data();let reply:unknown={...decision,principal_id:"43"};const fetcher=vi.fn(async()=>Response.json(reply));const hub=new PrivateAgentHubClient("http://local","42",()=>"test",fetcher);const runner=new PrivateAgentRunner(hub,meta,world,arena);
 await expect(runner.step()).rejects.toThrow("不一致");reply={...decision,output:{...decision.output,call:{name:"raw_command",arguments:{}}}};await expect(runner.step()).rejects.toThrow("越权");expect(arena.callTool).not.toHaveBeenCalled();reply={visibility:"Private",packages:[{...meta,owner_id:"43"}]};await expect(hub.list()).rejects.toThrow("归属");runner.stop();
});
