// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/** 受限TypeScript源码前端：只遍历真实TS AST，不eval/import/transpile执行作者代码。
 * 图节点/作用域/读写/循环语义仍由唯一KernelBuilder提供；完整类型与Gas由原Rust编译器决定。
 */
import ts from "typescript";
import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { Kernel, KernelBuilder, RuleValue, type KernelDeclaration, type RuleKernel } from "./kernel.js";
import { FIELD_CHANNEL_NAMES, type FieldChannel, type ValueType } from "./ir.js";

export const TS_AST_PROFILE = "rynvek.ts-kernel-ast.v1";
export const TS_AST_PARSER_VERSION = "5.8.3";
export const MAX_TS_SOURCE_BYTES = 65_536;
const MAX_AST_NODES = 8192;
const MAX_AST_DEPTH = 96;
type Bound = KernelBuilder | RuleValue | readonly RuleValue[];
type Environment = Map<string, Bound>;

function reject(node: ts.Node | undefined, message: string): never {
  const where = node === undefined ? "" : (() => {
    const f = node.getSourceFile();
    const p = f.getLineAndCharacterOfPosition(node.getStart(f));
    return ` (${p.line + 1}:${p.character + 1})`;
  })();
  throw new RynvekError(RynvekErrorCode.RuleStaticTypeCheckFailed, `${TS_AST_PROFILE}${where}: ${message}`);
}
function string(node: ts.Node | undefined): string {
  if (node === undefined || !ts.isStringLiteral(node)) reject(node, "此处只接受字符串字面量");
  return node.text;
}
function number(node: ts.Node | undefined): number {
  let sign = 1;
  if (node !== undefined && ts.isPrefixUnaryExpression(node)) {
    if (node.operator !== ts.SyntaxKind.MinusToken) reject(node, "只允许负数字面量");
    sign = -1; node = node.operand;
  }
  if (node === undefined || !ts.isNumericLiteral(node)) reject(node, "此处只接受数值字面量");
  const n = sign * Number(node.text);
  if (!Number.isFinite(n) || Math.abs(n) > Number.MAX_SAFE_INTEGER) reject(node, "数值不可精确表示");
  return n;
}
function properties(node: ts.Node | undefined): Map<string, ts.Expression> {
  if (node === undefined || !ts.isObjectLiteralExpression(node)) reject(node, "声明须为直接对象字面量");
  const out = new Map<string, ts.Expression>();
  for (const p of node.properties) {
    if (!ts.isPropertyAssignment(p) || !(ts.isIdentifier(p.name) || ts.isStringLiteral(p.name))) reject(p, "不接受展开、getter、简写或计算属性");
    const name = p.name.text;
    if (name === "__proto__" || name === "constructor" || name === "prototype" || out.has(name)) reject(p, "重复或保留属性");
    out.set(name, p.initializer);
  }
  return out;
}
function declaration(node: ts.Node | undefined): KernelDeclaration {
  const root = properties(node);
  for (const key of root.keys()) if (key !== "reads" && key !== "writes") reject(node, "未知读写声明字段");
  const group = (key: string): Record<string, ValueType> => {
    const result: Record<string, ValueType> = Object.create(null) as Record<string, ValueType>;
    if (!root.has(key)) return result;
    for (const [component, expr] of properties(root.get(key))) {
      const ty = string(expr);
      if (!["Bool", "I32", "Q8_8", "Q32_32"].includes(ty)) reject(expr, "组件类型不受支持");
      result[component] = ty as ValueType;
    }
    return result;
  };
  return { reads: group("reads"), writes: group("writes") };
}
function arrow(node: ts.Node | undefined, count: number): ts.ArrowFunction {
  if (node === undefined || !ts.isArrowFunction(node) || node.modifiers?.length || node.typeParameters || node.type || node.parameters.length !== count) reject(node, "只允许固定参数的同步箭头函数");
  const names = new Set<string>();
  for (const p of node.parameters) {
    if (!ts.isIdentifier(p.name) || p.modifiers?.length || p.type || p.initializer || p.dotDotDotToken || p.questionToken || names.has(p.name.text)) reject(p, "回调参数只能是不重复的简单标识符");
    names.add(p.name.text);
  }
  return node;
}
function bind(env: Environment, node: ts.Node, name: string, value: Bound): void {
  if (env.has(name) || ["Kernel", "__proto__", "constructor", "prototype"].includes(name)) reject(node, "绑定重复、遮蔽或使用保留名");
  env.set(name, value);
}
function value(node: ts.Expression, env: Environment): RuleValue {
  const v = expression(node, env);
  if (!(v instanceof RuleValue)) reject(node, "此处需要规则SSA值");
  return v;
}
function values(node: ts.Node | undefined, env: Environment): RuleValue[] {
  if (node === undefined || !ts.isArrayLiteralExpression(node)) reject(node, "循环携带值必须为直接数组");
  return node.elements.map((n) => value(n, env));
}
function channel(node: ts.Node | undefined): FieldChannel {
  const ch = string(node);
  if (!(FIELD_CHANNEL_NAMES as readonly string[]).includes(ch)) reject(node, "未知场通道");
  return ch as FieldChannel;
}
function expression(node: ts.Expression, env: Environment): Bound | undefined {
  if (ts.isParenthesizedExpression(node)) return expression(node.expression, env);
  if (ts.isIdentifier(node)) {
    const v = env.get(node.text);
    if (v === undefined) reject(node, "未绑定的标识符");
    return v;
  }
  if (ts.isNonNullExpression(node) && ts.isElementAccessExpression(node.expression)) return expression(node.expression, env);
  if (ts.isElementAccessExpression(node)) {
    if (node.questionDotToken) reject(node, "不接受可选索引");
    const array = expression(node.expression, env);
    const index = number(node.argumentExpression);
    if (!Array.isArray(array) || !Number.isInteger(index) || index < 0 || index >= array.length) reject(node, "仅可用常量索引访问循环携带数组");
    return array[index] as RuleValue;
  }
  if (!ts.isCallExpression(node) || node.questionDotToken || node.typeArguments || !ts.isPropertyAccessExpression(node.expression) || node.expression.questionDotToken) reject(node, "仅接受受控Builder/SSA方法调用");
  const receiver = expression(node.expression.expression, env);
  const method = node.expression.name.text;
  const args = node.arguments;
  const arity = (n: number): void => { if (args.length !== n) reject(node, "方法参数数量不符"); };
  const v = (i: number): RuleValue => { const a = args[i]; if (!a) reject(node, "缺少规则值"); return value(a, env); };
  if (receiver instanceof KernelBuilder) {
    switch (method) {
      case "i32": arity(1); return receiver.i32(number(args[0]));
      case "q8": arity(1); return receiver.q8(number(args[0]));
      case "q32": arity(1); return receiver.q32(number(args[0]));
      case "bool": {
        arity(1); const a = args[0];
        if (a?.kind !== ts.SyntaxKind.TrueKeyword && a?.kind !== ts.SyntaxKind.FalseKeyword) reject(a, "bool须为字面量");
        return receiver.bool(a.kind === ts.SyntaxKind.TrueKeyword);
      }
      case "load": arity(1); return receiver.load(string(args[0]));
      case "write": arity(2); receiver.write(string(args[0]), v(1)); return;
      case "sampleField": arity(1); return receiver.sampleField(channel(args[0]));
      case "deposit": arity(2); receiver.deposit(channel(args[0]), v(1)); return;
      case "neighborCount": arity(0); return receiver.neighborCount();
      case "loadNeighbor": arity(2); return receiver.loadNeighbor(string(args[0]), v(1));
      case "random": arity(1); return receiver.random(number(args[0]));
      case "loop": {
        arity(3); const limit = number(args[0]); const initial = values(args[1], env); const fn = arrow(args[2], 2);
        return receiver.loop(limit, initial, (index, carried) => {
          const local = new Map(env);
          bind(local, fn, (fn.parameters[0]!.name as ts.Identifier).text, index);
          bind(local, fn, (fn.parameters[1]!.name as ts.Identifier).text, carried);
          return ts.isBlock(fn.body) ? statements(fn.body, local, true)! : values(fn.body, local);
        });
      }
      default: reject(node, "Builder方法不在允许集合中");
    }
  }
  if (!(receiver instanceof RuleValue)) reject(node, "接收者不是规则Builder或SSA值");
  switch (method) {
    case "add": arity(1); return receiver.add(v(0));
    case "sub": arity(1); return receiver.sub(v(0));
    case "mul": arity(1); return receiver.mul(v(0));
    case "div": arity(1); return receiver.div(v(0));
    case "min": arity(1); return receiver.min(v(0));
    case "max": arity(1); return receiver.max(v(0));
    case "eq": arity(1); return receiver.eq(v(0));
    case "ne": arity(1); return receiver.ne(v(0));
    case "lt": arity(1); return receiver.lt(v(0));
    case "le": arity(1); return receiver.le(v(0));
    case "gt": arity(1); return receiver.gt(v(0));
    case "ge": arity(1); return receiver.ge(v(0));
    case "and": arity(1); return receiver.and(v(0));
    case "or": arity(1); return receiver.or(v(0));
    case "neg": arity(0); return receiver.neg();
    case "abs": arity(0); return receiver.abs();
    case "not": arity(0); return receiver.not();
    case "toI32": arity(0); return receiver.toI32();
    case "toQ8_8": arity(0); return receiver.toQ8_8();
    case "toQ32_32": arity(0); return receiver.toQ32_32();
    case "clamp": arity(2); return receiver.clamp(v(0), v(1));
    case "select": arity(2); return receiver.select(v(0), v(1));
    default: reject(node, "SSA方法不在允许集合中");
  }
}
function statements(block: ts.Block, env: Environment, needsReturn: boolean): RuleValue[] | undefined {
  for (let i = 0; i < block.statements.length; i++) {
    const stmt = block.statements[i]!;
    if (ts.isVariableStatement(stmt)) {
      if (stmt.modifiers?.length || stmt.declarationList.flags !== ts.NodeFlags.Const || stmt.declarationList.declarations.length !== 1) reject(stmt, "只允许单个const规则绑定");
      const d = stmt.declarationList.declarations[0]!;
      if (!ts.isIdentifier(d.name) || d.type || !d.initializer || d.exclamationToken) reject(d, "const须为有初始化值的简单规则绑定");
      const v = expression(d.initializer, env); if (v === undefined) reject(d, "不能绑定无返回值的写操作");
      bind(env, d, d.name.text, v);
    } else if (ts.isExpressionStatement(stmt)) {
      if (!ts.isCallExpression(stmt.expression)) reject(stmt, "只允许方法调用语句");
      expression(stmt.expression, env);
    } else if (ts.isReturnStatement(stmt) && needsReturn && i === block.statements.length - 1) {
      return values(stmt.expression, env);
    } else reject(stmt, "不支持此语句；业务循环只能使用k.loop");
  }
  if (needsReturn) reject(block, "循环回调必须返回携带数组");
  return;
}
/** 编译真实TS语法树到原RuleGraph；不运行作者回调，不进行模块加载或文件系统解析。 */
export function compileKernelSource(source: string): RuleKernel {
  if (ts.version !== TS_AST_PARSER_VERSION) reject(undefined, "解析器版本不符，拒绝非固定工具链");
  let bytes = 0;
  for (const ch of source) {
    const cp = ch.codePointAt(0)!;
    if (cp >= 0xd800 && cp <= 0xdfff) reject(undefined, "源码含未配对代理字符");
    bytes += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    if (bytes > MAX_TS_SOURCE_BYTES) reject(undefined, "源码超过64KiB");
  }
  try {
    const file = ts.createSourceFile("rule.ts", source, ts.ScriptTarget.ES2022, true, ts.ScriptKind.TS);
    // 公开语法诊断API只解析虚拟内存文件；禁用默认lib/resolve，绝不读取作者指定模块。
    const host: ts.CompilerHost = {
      getSourceFile: (name) => name === "rule.ts" ? file : undefined,
      getDefaultLibFileName: () => "", writeFile: () => { throw new Error("禁写"); }, getCurrentDirectory: () => "",
      getDirectories: () => [], fileExists: (name) => name === "rule.ts", readFile: () => undefined,
      getCanonicalFileName: (name) => name, useCaseSensitiveFileNames: () => true, getNewLine: () => "\n",
    };
    const diagnostics = ts.createProgram(["rule.ts"], { noLib: true, noResolve: true }, host).getSyntacticDiagnostics(file);
    if (diagnostics.length) reject(file, "TypeScript语法非法");
    let count = 0;
    const bound = (node: ts.Node, depth: number): void => {
      if (++count > MAX_AST_NODES || depth > MAX_AST_DEPTH) reject(node, "AST节点/深度超过剖面上限");
      ts.forEachChild(node, (child) => bound(child, depth + 1));
    }; bound(file, 0);
    if (file.statements.length !== 2) reject(file, "仅允许一条Kernel导入和一个export default定义");
    const [imp, exp] = file.statements;
    if (!imp || !ts.isImportDeclaration(imp) || imp.modifiers?.length || imp.attributes || string(imp.moduleSpecifier) !== "@rynvek/rules") reject(imp, "只允许静态Kernel导入声明");
    const clause = imp.importClause; const bindings = clause?.namedBindings;
    if (!clause || clause.isTypeOnly || clause.name || !bindings || !ts.isNamedImports(bindings) || bindings.elements.length !== 1) reject(imp, "只允许import { Kernel }");
    const imported = bindings.elements[0]!;
    if (imported.isTypeOnly || imported.propertyName || imported.name.text !== "Kernel") reject(imp, "不接受Kernel别名或type导入");
    if (!exp || !ts.isExportAssignment(exp) || exp.isExportEquals || exp.modifiers?.length) reject(exp, "必须export default Kernel.define");
    const call = exp.expression;
    if (!ts.isCallExpression(call) || call.typeArguments || call.questionDotToken || !ts.isPropertyAccessExpression(call.expression) || call.expression.questionDotToken || !ts.isIdentifier(call.expression.expression) || call.expression.expression.text !== "Kernel" || call.expression.name.text !== "define" || call.arguments.length !== 3) reject(call, "必须直接调用Kernel.define(name,declaration,body)");
    const fn = arrow(call.arguments[2], 1); if (!ts.isBlock(fn.body)) reject(fn.body, "顶层规则回调必须为块");
    return Kernel.define(string(call.arguments[0]), declaration(call.arguments[1]), (builder) => {
      const env: Environment = new Map(); bind(env, fn, (fn.parameters[0]!.name as ts.Identifier).text, builder);
      statements(fn.body as ts.Block, env, false);
    });
  } catch (e) {
    if (e instanceof RynvekError) throw e;
    // 含极深括号导致解析器栈溢出等情况，也不暴露宿主异常/源码内容。
    reject(undefined, "AST构建失败或输入超过解析器能力边界");
  }
}
