#!/usr/bin/env node
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// 显式本地构建工具：只读指定源码，输出原规则图，拒绝覆盖旧资料。
import { openSync, readSync, closeSync, writeFileSync } from "node:fs";
import { compileKernelSource, MAX_TS_SOURCE_BYTES } from "../dist/ast.js";
try {
  const [source, out, ...extra] = process.argv.slice(2);
  if (!source || !out || extra.length) throw new Error("用法：rynvek-rule-ast <rule.ts> <rule.graph.json>（不覆盖已有文件）");
  const fd = openSync(source, "r");
  const buffer = Buffer.alloc(MAX_TS_SOURCE_BYTES + 1);
  let used = 0;
  try { while (used < buffer.length) { const n = readSync(fd, buffer, used, buffer.length - used, null); if (!n) break; used += n; } } finally { closeSync(fd); }
  if (used > MAX_TS_SOURCE_BYTES) throw new Error("源码超过64KiB");
  const text = new TextDecoder("utf-8", { fatal: true }).decode(buffer.subarray(0, used));
  const graph = compileKernelSource(text).toJSON();
  writeFileSync(out, graph, { flag: "wx", encoding: "utf8" });
} catch (e) {
  console.error(`AST构建拒绝${typeof e?.code === "number" ? ` 0x${e.code.toString(16)}` : ""}: ${e instanceof Error ? e.message : "输入非法"}`);
  process.exitCode = 1;
}
