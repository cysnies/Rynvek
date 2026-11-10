// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 实验设计：流式构建器、全因子扫描（sweep）与单因子消融（ablation）。
 * 产出的规格总是先经过 {@link validateSpec}，保证提交后不会被服务端以 400 拒绝。
 */

import {
  BASELINE_VARIANT,
  type ExperimentSpec,
  type LabMode,
  type PolicyKind,
  SpecError,
  type VariantSpec,
  validateSpec,
} from "./spec.js";

/** 流式实验构建器 */
export class ExperimentBuilder {
  #seeds: number[] = [];
  #ticks = 0;
  #sampleEvery: number | undefined;
  #variants: VariantSpec[] = [];

  constructor(readonly name: string) {}

  seeds(...seeds: number[]): this {
    this.#seeds.push(...seeds);
    return this;
  }

  ticks(ticks: number): this {
    this.#ticks = ticks;
    return this;
  }

  sampleEvery(ticks: number): this {
    this.#sampleEvery = ticks;
    return this;
  }

  variant(v: VariantSpec): this {
    this.#variants.push(v);
    return this;
  }

  variants(vs: Iterable<VariantSpec>): this {
    for (const v of vs) this.#variants.push(v);
    return this;
  }

  build(): ExperimentSpec {
    const spec: ExperimentSpec = {
      name: this.name,
      seeds: [...this.#seeds],
      ticks: this.#ticks,
      ...(this.#sampleEvery === undefined ? {} : { sample_every: this.#sampleEvery }),
      ...(this.#variants.length === 0 ? {} : { variants: [...this.#variants] }),
    };
    validateSpec(spec);
    return spec;
  }
}

/** 开始构建实验 */
export function experiment(name: string): ExperimentBuilder {
  return new ExperimentBuilder(name);
}

/** 扫描轴 */
export interface SweepAxes {
  readonly mode?: readonly LabMode[];
  readonly policy?: readonly PolicyKind[];
  readonly players?: readonly number[];
}

const playersValid = (mode: LabMode, p: number): boolean => (mode === "pve" ? p === 1 || p === 2 : p === 2);

/**
 * 全因子扫描：模式 × 策略 × 玩家数的笛卡尔积（自动剔除不合法组合，例如双人局 1 名玩家）。
 * 标签形如 `pve-economy-p1`，顺序按轴声明顺序稳定。
 */
export function sweep(axes: SweepAxes): VariantSpec[] {
  const modes = axes.mode ?? ["pve"];
  const policies = axes.policy ?? ["economy"];
  const out: VariantSpec[] = [];
  for (const mode of modes) {
    const players = axes.players ?? [mode === "pve" ? 1 : 2];
    for (const policy of policies) {
      for (const p of players) {
        if (!playersValid(mode, p)) continue;
        out.push({ label: `${mode}-${policy}-p${p}`, mode, policy, players: p });
      }
    }
  }
  if (out.length === 0) throw new SpecError("扫描轴组合后没有合法变体");
  return out;
}

/**
 * 单因子消融：基线变体 + 每次只改变一个因子的变体。
 * `factors` 的键为新变体标签，值为相对基线的覆盖字段。
 */
export function ablation(
  factors: Readonly<Record<string, Omit<Partial<VariantSpec>, "label">>>,
  baseline: VariantSpec = BASELINE_VARIANT,
): VariantSpec[] {
  const out: VariantSpec[] = [baseline];
  for (const [label, override] of Object.entries(factors)) {
    out.push({ ...baseline, ...override, label });
  }
  return out;
}
