// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
import { describe, it, expect } from "vitest";
import { RynvekErrorCode } from "@rynvek/core";
import { compileKernelSource, MAX_TS_SOURCE_BYTES } from "../src/ast.js";
import { Kernel } from "../src/kernel.js";
const wrap = (body: string) => `import { Kernel } from "@rynvek/rules";
export default Kernel.define("ast", { reads: { "a.v": "Q8_8" }, writes: { "a.v": "Q8_8" } }, (k) => { ${body} });`;
describe("真实TypeScript AST受控前端", () => {
  it("原Builder逐字节同图并可复现，不执行作者JS", () => {
    const source = wrap('const a = k.load("a.v"); k.write("a.v", a.add(k.q8(-0.5)));');
    const expected = Kernel.define("ast", { reads: { "a.v": "Q8_8" }, writes: { "a.v": "Q8_8" } }, (k) => k.write("a.v", k.load("a.v").add(k.q8(-0.5)))).toJSON();
    expect(compileKernelSource(source).toJSON()).toBe(expected);
    expect(compileKernelSource(source).toJSON()).toBe(compileKernelSource(source).toJSON());
  });
  it("原loop/Phi及外部作用域复用，不重新实现循环语义", () => {
    const source = wrap('const acc = k.loop(4, [k.q8(0)], (i, carried) => { const b = k.loadNeighbor("a.v", i); return [carried[0]!.add(b)]; }); k.write("a.v", acc[0]!);');
    const expected = Kernel.define("ast", { reads: { "a.v": "Q8_8" }, writes: { "a.v": "Q8_8" } }, (k) => { const acc = k.loop(4, [k.q8(0)], (i, carried) => [carried[0]!.add(k.loadNeighbor("a.v", i))]); k.write("a.v", acc[0]!); }).toJSON();
    expect(compileKernelSource(source).toJSON()).toBe(expected);
  });
  it("逐项覆盖原36指令构建器出口", () => {
    const source = `import {Kernel} from "@rynvek/rules"; export default Kernel.define("ops", {reads:{"a":"I32"},writes:{"a":"I32","field.nutrient":"Q8_8"}}, (k)=>{
      const a=k.i32(1);const b=k.i32(2); const q=k.q8(0.5);const wide=k.q32(1);const flag=k.bool(true);
      a.add(b);a.sub(b);a.mul(b);a.div(b);a.min(b);a.max(b);a.neg();a.abs();a.clamp(a,b);
      a.eq(b);a.ne(b);a.lt(b);a.le(b);a.gt(b);a.ge(b);flag.and(flag);flag.or(flag);flag.not();flag.select(a,b);
      q.toI32();a.toQ8_8();q.toQ32_32();k.load("a");k.write("a",a);k.sampleField("nutrient");k.deposit("nutrient",q);
      k.neighborCount();k.loadNeighbor("a",a);k.random(7);const result=k.loop(1,[a],(i,carry)=>[carry[0]!.add(i)]);
    });`;
    const ops = new Set(compileKernelSource(source).graph.nodes.map((n) => n.op)); expect(ops.size).toBe(36);
  });
  it.each([
    'globalThis.__rynvek_ast_executed = true;', 'process.exit(0);', 'import("node:fs");',
    'eval("1");', 'new Function("return 1")();', 'while(true){}', 'for(;;){}',
    'let a = k.i32(1);', 'const a = k["i32"](1);', 'const a = k.constructor;',
    'const a = k.i32(1); a.constructor();', 'const a = k.i32(1); a.add = k.load;',
    'const a = k.i32(1 as number);', 'const a = k.i32(...[1]);',
    'const a = k.i32(1); const a = k.i32(2);',
    'k.loop(1, [], async (i, carried) => []);',
    'k.loop(1, [], (i, carried) => { return []; k.i32(1); });',
    'k.loop(1, [k.i32(0)], (i, carried) => [carried?.[0]]);',
  ])("拒绝宿主能力/动态语法且无执行：%s", (body) => {
    expect(() => compileKernelSource(wrap(body))).toThrow();
    expect((globalThis as Record<string, unknown>)["__rynvek_ast_executed"]).toBeUndefined();
  });
  it.each([
    'import {Kernel} from "elsewhere"; export default Kernel.define("x",{},k=>{});',
    'import {Kernel as K} from "@rynvek/rules"; export default K.define("x",{},k=>{});',
    'import {Kernel} from "@rynvek/rules"; globalThis.x=1; export default Kernel.define("x",{},k=>{});',
    'import {Kernel} from "@rynvek/rules"; export default Kernel.define("x",{reads:{...{}}},k=>{});',
    'import {Kernel} from "@rynvek/rules"; export default Kernel.define("x",{reads:{get x(){return "I32"}}},k=>{});',
    'import {Kernel} from "@rynvek/rules"; export default Kernel.define("x",{reads:{"a":"I32","a":"Bool"}},k=>{});',
    'import {Kernel} from "@rynvek/rules"; export default Kernel.define("x",{reads:{__proto__:"I32"}},k=>{});',
  ])("整个模块先拒绝动态来源/重复声明：%s", (source) => expect(() => compileKernelSource(source)).toThrow());
  it("沿原错误码拒绝越界循环、未声明写和非法常量", () => {
    expect(() => compileKernelSource(wrap('k.loop(65,[],(i,c)=>[]);'))).toThrowError(expect.objectContaining({ code: RynvekErrorCode.RuleLoopBoundExceeded }));
    expect(() => compileKernelSource(wrap('k.write("bad",k.i32(1));'))).toThrowError(expect.objectContaining({ code: RynvekErrorCode.RuleUndeclaredWriteAccess }));
    for (const x of ['k.q8(0.1);','k.i32(2147483648);','k.random(65536);','k.i32(9007199254740993);','k.load("bad");']) expect(() => compileKernelSource(wrap(x))).toThrow();
  });
  it("拒绝语法/源码/AST深度与节点超界，不泄露原始宿主异常", () => {
    for (const text of [wrap('const = ;'), ' '.repeat(MAX_TS_SOURCE_BYTES + 1), wrap(`k.i32(${'('.repeat(120)}1${')'.repeat(120)});`), wrap('k.i32(1);'.repeat(1800))]) expect(() => compileKernelSource(text)).toThrowError(expect.objectContaining({ code: RynvekErrorCode.RuleStaticTypeCheckFailed }));
  });
});
