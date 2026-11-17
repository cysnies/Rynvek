// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from "vitest";
import {RemoteStudioSession,StudioClient,type AuthoringView,type WorldPatch} from "../src/index.js";
const basis:AuthoringView={world_id:"w",revision:9,tick:4,state_root:"ab".repeat(32),rule_fault:null,parameter_state:{next_serial:0,entities:{},fault:null},baseline:{width:1,height:1,terrain:[0],genomes:{seed:"e30="},templates:["genome/seed"]}};
it("共享模板删除/恢复/重做沿同一CRDT寄存器和真实修订回执，不把补丁数当世界版本",async()=>{
 const sent:WorldPatch[]=[];
 const fetcher:typeof fetch=async(_url,init)=>{const p=JSON.parse(String(init?.body)) as WorldPatch;sent.push(p);expect(init?.cache).toBe("no-store");return Response.json({status:"applied",world_id:"w",world_revision:p.base_authoring_revision+2,effective_tick:8,state_root:"cd".repeat(32)})};
 const s=new RemoteStudioSession(new StudioClient("http://srv","owner",fetcher),basis,1),signal=new AbortController().signal;
 await s.edit([{RemoveEntityTemplate:{template_id:"genome/seed"}}],signal);expect(s.document.genome("seed")).toBeUndefined();
 await s.undoEdit(signal);expect(s.document.genome("seed")).toBe("e30=");
 await s.redoEdit(signal);expect(s.document.genome("seed")).toBeUndefined();
 expect(sent.map(p=>p.base_authoring_revision)).toEqual([9,11,13]);expect(sent[1]?.operations[0]).toHaveProperty("UpsertSeedGenome");expect(s.document.revision).toBe(15);
});
it("冲突或网络不确定即失效，不重试或把本地编辑冒充服务器成功",async()=>{
 let calls=0;const s=new RemoteStudioSession(new StudioClient("http://srv","owner",async()=>{calls++;return Response.json({message:"修订冲突"},{status:409})}),basis,1);const signal=new AbortController().signal;
 await expect(s.edit([{PaintTerrainRect:{x0:0,y0:0,width:1,height:1,tile_kind:4}}],signal)).rejects.toThrow("409");expect(s.valid).toBe(false);
 await expect(s.undoEdit(signal)).rejects.toThrow("刷新");expect(calls).toBe(1);
});
it("同一基因目录删除与更新任意到达顺序收敛，撤销不能覆盖另一作者新值",()=>{
 const client=new StudioClient("http://srv","owner");const a=new RemoteStudioSession(client,basis,1).document,b=new RemoteStudioSession(client,basis,2).document;
 const remove:WorldPatch={patch_id:new Array(16).fill(1),base_authoring_revision:9,hlc:{wall_time_ms:1,logical_counter:0,author_node_id:1},safety_class:"TickBoundarySafe",operations:[{RemoveEntityTemplate:{template_id:"genome/seed"}}]};
 const update:WorldPatch={...remove,patch_id:new Array(16).fill(2),hlc:{...remove.hlc,author_node_id:2},safety_class:"EpochBoundarySafe",operations:[{UpsertSeedGenome:{genome_slug:"seed",genome_bytes_b64:"AQ=="}}]};
 a.integrate(remove);a.integrate(update);b.integrate(update);b.integrate(remove);expect(a.canonicalState()).toBe(b.canonicalState());expect(a.genome("seed")).toBe("AQ==");expect(a.inverseOf(remove.patch_id).operations).toEqual([]);
});

it("首次规则创建撤销/重做与显式删除恢复均发送Epoch逆补丁并等待服务器修订",async()=>{
 const sent:WorldPatch[]=[];const source={rule_graph_json:"exact original",compiled_rir_blake3:new Array(32).fill(26)};
 const fetcher:typeof fetch=async(_url,init)=>{const p=JSON.parse(String(init?.body)) as WorldPatch;sent.push(p);return Response.json({status:"applied",world_id:"w",world_revision:p.base_authoring_revision+4,effective_tick:sent.length*4,state_root:"cd".repeat(32)})};
 const s=new RemoteStudioSession(new StudioClient("http://srv","owner",fetcher),basis,1),signal=new AbortController().signal;
 const up={UpdateRuleModule:{module_id:"control",...source}},del={RemoveRuleModule:{module_id:"control"}};
 await s.edit([up],signal);await s.undoEdit(signal);expect(s.document.ruleModule("control")).toBeUndefined();
 await s.redoEdit(signal);expect(s.document.ruleModule("control")).toEqual(source);
 await s.edit([del],signal);expect(JSON.parse(s.document.canonicalState()).rules).toEqual({});
 await s.undoEdit(signal);expect(s.document.ruleModule("control")).toEqual(source);
 await s.redoEdit(signal);expect(s.document.ruleModule("control")).toBeUndefined();
 expect(sent.map(p=>p.operations)).toEqual([[up],[del],[up],[del],[up],[del]]);
 expect(sent.map(p=>p.base_authoring_revision)).toEqual([9,13,17,21,25,29]);expect(sent.every(p=>p.safety_class==="EpochBoundarySafe")).toBe(true);expect(s.document.revision).toBe(33);
});
it("基线规则墓碑不复活、重复写一次逆操作，删除与更新乱序收敛且保留他人胜出值",()=>{
 const rule={rule_graph_json:"baseline",compiled_rir_blake3:new Array(32).fill(1)};
 const view={...basis,baseline:{...basis.baseline,rules:{control:rule}}};
 const client=new StudioClient("http://srv","owner"),s=new RemoteStudioSession(client,view,1);
 const del:WorldPatch={patch_id:new Array(16).fill(1),base_authoring_revision:9,hlc:{wall_time_ms:1,logical_counter:0,author_node_id:1},safety_class:"EpochBoundarySafe",operations:[{RemoveRuleModule:{module_id:"control"}}]};
 s.document.integrate(del);expect(s.document.ruleModule("control")).toBeUndefined();expect(JSON.parse(s.document.canonicalState()).rules).toEqual({});expect(s.document.inverseOf(del.patch_id).operations).toEqual([{UpdateRuleModule:{module_id:"control",...rule}}]);
 const up:WorldPatch={...del,patch_id:new Array(16).fill(2),hlc:{...del.hlc,wall_time_ms:2},operations:[{UpdateRuleModule:{module_id:"control",...rule}},{UpdateRuleModule:{module_id:"control",...rule}}]};
 s.document.integrate(up);expect(s.document.inverseOf(up.patch_id).operations).toEqual(del.operations); // 不从墓碑回落到基线
 const other={...up,patch_id:new Array(16).fill(3),hlc:{...up.hlc,author_node_id:2},operations:[{UpdateRuleModule:{module_id:"control",...rule,rule_graph_json:"other"}}]};
 s.document.integrate(other);expect(s.document.inverseOf(up.patch_id).skipped).toEqual([{register:{kind:"rule",moduleId:"control"},reason:"superseded"}]);
 const b=new RemoteStudioSession(client,view,2).document;b.integrate(other);b.integrate(up);b.integrate(del);expect(b.canonicalState()).toBe(s.document.canonicalState());expect(b.ruleModule("control")?.rule_graph_json).toBe("other");
});
