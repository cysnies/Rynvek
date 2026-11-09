// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

// Tier-1 规则共享夹具：Rust 宿主（rquickjs / QuickJS-NG）与 TS 宿主（quickjs-ng WASM）必须给出逐字节相同的输出。
// 约定：定义全局纯函数 tick(input)；随机数只能来自宿主注入的 Rynvek.philox4x32_10。
function tick(input) {
  const out = [];
  for (const e of input.entities) {
    const r = Rynvek.philox4x32_10([e.serial >>> 0, 7, input.tick >>> 0, 0], [input.seed >>> 0, 0]);
    const jitter = r[0] >>> 24;
    const next = e.energy - 64 + (e.food ? 512 : 0) + jitter;
    out.push({ id: e.id, energy: Math.max(0, Math.min(25600, next)), jitter });
  }
  return { tick: input.tick + 1, entities: out };
}
