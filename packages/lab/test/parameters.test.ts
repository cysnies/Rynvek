// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {readFileSync} from "node:fs";
import {expect,it} from "vitest";
import {parameterMetrics,parameterSweep,validateParameterModel,validateSpec,toRequestBody} from "../src/index.js";
const fixture=()=>JSON.parse(readFileSync(new URL("../../../examples/lab/parameter-counter.json",import.meta.url),"utf8"));
it("same immutable model scans actual param values and preserves request sources",()=>{
 const model=fixture();validateParameterModel(model);const variants=parameterSweep(model,"counter.one","param.rate",[-2,3]);
 const spec={name:"parameter",seeds:[57],ticks:4,sample_every:2,variants};validateSpec(spec);
 expect(variants.map(v=>v.model?.instances["counter.one"]?.parameters["param.rate"])).toEqual([-2,3]);
 expect(model.instances["counter.one"].parameters).toEqual({});expect((toRequestBody(spec).variants as unknown[])).toEqual(variants);
});
it("no unsafe/fractional/out-of-type values, state scans or oversized result budgets",()=>{
 const m=fixture();expect(()=>parameterSweep(m,"counter.one","state.total",[1])).toThrow();expect(()=>parameterSweep(m,"counter.one","param.rate",[2147483648])).toThrow();
 for(const bad of [1.5,Number.MAX_SAFE_INTEGER])expect(()=>parameterSweep(m,"counter.one","param.rate",[bad])).toThrow();
 const variants=parameterSweep(m,"counter.one","param.rate",[1]);expect(()=>validateSpec({name:"too large",seeds:[1],ticks:108000,sample_every:1,variants})).toThrow();
 m.outputs.push(m.outputs[0]);expect(()=>validateParameterModel(m)).toThrow();
});
it("CAS trajectory decoder keeps signed raw values and exact u64 ticks without fake RTS metrics",()=>{
 const row={run:0,tick:"9007199254740993",faction:null,score_milli:null,parameter_entity:"counter.one",parameter_component:"state.bias",parameter_type:"Q8_8",parameter_value:-128};
 expect(parameterMetrics([row])[0]).toEqual({run:0,tick:9007199254740993n,instance_id:"counter.one",component:"state.bias",ty:"Q8_8",raw:-128});
 expect(()=>parameterMetrics([{...row,faction:0}])).toThrow();expect(()=>parameterMetrics([{...row,parameter_value:32768}])).toThrow();
 expect(()=>parameterMetrics([{...row,tick:9007199254740992}])).toThrow();
});
