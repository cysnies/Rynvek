// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from "vitest";
import {RemoteStudioSession,StudioClient,type AuthoringView,type WorldPatch} from "../src/index.js";
const basis:AuthoringView={world_id:"w",revision:9,tick:4,state_root:"ab".repeat(32),rule_fault:null,baseline:{width:1,height:1,terrain:[0],genomes:{seed:"e30="},templates:["genome/seed"]}};
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
