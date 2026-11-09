// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 测试辅助：确定性补丁 ID、伪随机数与全排列。

import type { AuthoringBaseline } from "../src/index.js";

/** 由计数器生成可复现的 16 字节补丁 ID（首字节为作者，避免不同作者碰撞） */
export function idGen(author: number): () => number[] {
  let n = 0;
  return () => {
    n++;
    return [author & 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, (n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff];
  };
}

/** mulberry32：测试用确定性 PRNG */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function permutations<T>(xs: readonly T[]): T[][] {
  if (xs.length <= 1) return [xs.slice()];
  const out: T[][] = [];
  xs.forEach((x, i) => {
    for (const rest of permutations([...xs.slice(0, i), ...xs.slice(i + 1)])) out.push([x, ...rest]);
  });
  return out;
}

export function baseline(): AuthoringBaseline {
  return {
    width: 8,
    height: 6,
    terrain: new Uint8Array(48),
    params: { "field.nutrient.diffusion": "0.1", "presentation.grid_alpha": "0.5" },
    genomes: { "amoeba-basic": "AA==" },
    rules: { "rts.economy": { ts_source: "v1", compiled_rir_blake3: new Array(32).fill(1) } },
    templates: ["legacy-turret", "spore-pod"],
  };
}

/** 可手动推进的物理时钟 */
export function manualClock(start = 1_000): { now: () => number; advance: (ms: number) => void } {
  let t = start;
  return { now: () => t, advance: (ms) => void (t += ms) };
}
