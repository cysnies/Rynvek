// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it} from "vitest";
import {AuthoringDocument,HybridLogicalClock,UndoManager,parameterRegistryOp,PARAMETER_TEMPLATE_EXAMPLE,requiredClassOf,LiveEditSafetyClass} from "../src/index.js";
it("一般模板复用同一参数LWW寄存器，原生删除与SetWorldParam删除收敛且可选择性撤销",()=>{
 const a=new AuthoringDocument({width:1,height:1,terrain:new Uint8Array(1)}),u=new UndoManager(a,new HybridLogicalClock(3));
 const first=u.commit([parameterRegistryOp("templates","model.counter",PARAMETER_TEMPLATE_EXAMPLE)]);expect(a.hasTemplate("parameter/model.counter")).toBe(true);
 const del={RemoveEntityTemplate:{template_id:"parameter/model.counter"}} as const;expect(requiredClassOf(del)).toBe(LiveEditSafetyClass.EpochBoundarySafe);u.commit([del]);expect(a.hasTemplate("parameter/model.counter")).toBe(false);const undo=u.undo();expect(undo?.patch.operations[0]).toEqual(parameterRegistryOp("templates","model.counter",PARAMETER_TEMPLATE_EXAMPLE));expect(a.hasTemplate("parameter/model.counter")).toBe(true);
 const b=new AuthoringDocument({width:1,height:1,terrain:new Uint8Array(1)});b.integrateAll([...a.allPatches()].reverse());expect(b.canonicalState()).toBe(a.canonicalState());expect(JSON.parse(a.canonicalState()).templates).toContain("parameter/model.counter");expect(first.patch.safety_class).toBe("EpochBoundarySafe");
});
it("模板/实例/绑定共用原CRDT选择性撤销，不重写已被远端覆盖的参数",()=>{
 const a=new AuthoringDocument({width:1,height:1,terrain:new Uint8Array(1)});const raw=a.createPatch(new HybridLogicalClock(1),[parameterRegistryOp("instances","counter.one",{template_id:"model.counter",parameters:{"param.rate":3}})],new Array(16).fill(1));const one={...raw,hlc:{wall_time_ms:1,logical_counter:0,author_node_id:1}};a.integrate(one);
 const two={...one,patch_id:new Array(16).fill(2),hlc:{wall_time_ms:2,logical_counter:0,author_node_id:2},operations:[parameterRegistryOp("instances","counter.one",{template_id:"model.counter",parameters:{"param.rate":7}})]};a.integrate(two);expect(a.inverseOf(one.patch_id).operations).toEqual([]);expect(a.inverseOf(one.patch_id).skipped[0]?.reason).toBe("superseded");expect(()=>parameterRegistryOp("templates","../bad",null)).toThrow();
});
