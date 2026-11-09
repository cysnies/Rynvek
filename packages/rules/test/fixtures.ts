// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

// 与 Rust `rynvek-rules` 共享的规则夹具（`tests/rules/`）。

import { fileURLToPath } from "node:url";

import { Kernel, type RuleKernel } from "../src/index.js";

/** 仓库根目录下的共享夹具目录 */
export const SHARED_FIXTURE_DIR = fileURLToPath(new URL("../../../tests/rules/", import.meta.url));

/**
 * 觅食者代谢规则：采食营养场、按代谢率与拥挤度消耗能量、邻居能量均值扩散、随机抖动，
 * 并把采食量从营养场中扣除。
 */
export function foragerMetabolism(): RuleKernel {
  return Kernel.define(
    "forager.metabolism",
    {
      reads: { "energy.value": "Q8_8", "genome.metabolic_rate": "Q8_8" },
      writes: { "energy.value": "Q8_8", "field.nutrient": "Q8_8" },
    },
    (k) => {
      const energy = k.load("energy.value");
      const rate = k.load("genome.metabolic_rate");
      const uptake = k.sampleField("nutrient").mul(k.q8(0.25));
      // 拥挤成本：邻居数（I32）× 1/16，由编译器隐式提升为 Q8.8
      const neighbors = k.neighborCount();
      const crowd = neighbors.mul(k.q8(0.0625));
      // 邻居能量之和（最多 8 个邻居）
      const [sum] = k.loop(8, [k.q8(0)], (i, [acc]) => [acc!.add(k.loadNeighbor("energy.value", i))]);
      const hasNeighbors = neighbors.gt(k.i32(0));
      const diffusion = hasNeighbors.select(sum!.div(neighbors).sub(energy).mul(k.q8(0.125)), k.q8(0));
      // 抖动：[0, 1) 随机数 > 0.5 时额外消耗 1/64
      const jitter = k.random(1).gt(k.q8(0.5)).select(k.q8(1 / 64), k.q8(0));
      const next = energy.add(uptake).add(diffusion).sub(rate.add(crowd).add(jitter));
      k.write("energy.value", next.clamp(k.q8(0), k.q8(100)));
      k.deposit("nutrient", uptake.neg());
    },
  );
}
