// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 四级热更新安全分类的判定与生效时机调度（规范 10.2 `LiveEditSafetyClass`）。
//
// 规范以举例方式给出每级覆盖的内容；`SetWorldParam` 的键路径命名空间到分级的映射按最佳实践补全，
// 未登记的命名空间一律按最保守的 Class 3 处理（ERRATA E-021）。

import {
  LiveEditSafetyClass,
  SAFETY_CLASS_NAMES,
  comparePatches,
  opKind,
  type LiveEditSafetyClassName,
  type WorldPatch,
  type WorldPatchOp,
} from "./patch.js";

/** Coupling Epoch 长度（tick % 4 == 0 为 Epoch 边界） */
export const COUPLING_EPOCH_TICKS = 4;

/** `SetWorldParam.key_path` 顶层命名空间 → 分级 */
export const WORLD_PARAM_NAMESPACE_CLASS: Readonly<Record<string, LiveEditSafetyClass>> = {
  // Class 0：只影响表现层
  presentation: LiveEditSafetyClass.HotSafePresentation,
  visual: LiveEditSafetyClass.HotSafePresentation,
  ui: LiveEditSafetyClass.HotSafePresentation,
  audio: LiveEditSafetyClass.HotSafePresentation,
  // Class 1：环境场参数（D_c、λ_c）、建筑造价、单位数值属性
  field: LiveEditSafetyClass.TickBoundarySafe,
  economy: LiveEditSafetyClass.TickBoundarySafe,
  buildings: LiveEditSafetyClass.TickBoundarySafe,
  units: LiveEditSafetyClass.TickBoundarySafe,
  // Class 2：神经网络拓扑规则、规则模块
  ecology: LiveEditSafetyClass.EpochBoundarySafe,
  neural: LiveEditSafetyClass.EpochBoundarySafe,
  rules: LiveEditSafetyClass.EpochBoundarySafe,
  // Class 3：Chunk 尺寸、空间维度、数值 Profile 等世界结构参数
  world: LiveEditSafetyClass.RequiresForkRestart,
};

/** 单个操作所需的最低安全分级 */
export function requiredClassOf(op: WorldPatchOp): LiveEditSafetyClass {
  switch (opKind(op)) {
    case "SetWorldParam": {
      const key = (op as Extract<WorldPatchOp, { SetWorldParam: unknown }>).SetWorldParam.key_path;
      const ns = key.split(".")[0] ?? "";
      return WORLD_PARAM_NAMESPACE_CLASS[ns] ?? LiveEditSafetyClass.RequiresForkRestart;
    }
    // 地形影响寻路与场边界，作为系统命令在 Tick 边界生效
    case "PaintTerrainRect":
      return LiveEditSafetyClass.TickBoundarySafe;
    case "RemoveEntityTemplate":
      return LiveEditSafetyClass.TickBoundarySafe;
    // 基因组决定神经拓扑发育；规则模块热重载（.rir）——规范明确为 Epoch 边界
    case "UpsertSeedGenome":
    case "UpdateRuleModule":
      return LiveEditSafetyClass.EpochBoundarySafe;
  }
}

/** 整个补丁所需的最低分级（各操作取最大） */
export function requiredSafetyClass(ops: readonly WorldPatchOp[]): LiveEditSafetyClass {
  return ops.reduce<LiveEditSafetyClass>((m, op) => Math.max(m, requiredClassOf(op)) as LiveEditSafetyClass, 0);
}

export function safetyClassName(c: LiveEditSafetyClass): LiveEditSafetyClassName {
  return SAFETY_CLASS_NAMES[c] as LiveEditSafetyClassName;
}

export function safetyClassValue(name: LiveEditSafetyClassName): LiveEditSafetyClass {
  return LiveEditSafetyClass[name];
}

/** 补丁声明的分级不得低于其操作所需分级（允许声明得更保守） */
export function isSafetyClassSufficient(patch: WorldPatch): boolean {
  return safetyClassValue(patch.safety_class) >= requiredSafetyClass(patch.operations);
}

/** 生效时机 */
export type ApplyTiming =
  | { readonly kind: "frame" }
  | { readonly kind: "tick"; readonly tick: number; readonly phase: "Phase01" }
  | { readonly kind: "epoch"; readonly tick: number }
  | { readonly kind: "fork" };

/** 给定当前已提交 Tick，计算补丁生效时机 */
export function scheduleFor(c: LiveEditSafetyClass, committedTick: number): ApplyTiming {
  switch (c) {
    case LiveEditSafetyClass.HotSafePresentation:
      return { kind: "frame" };
    case LiveEditSafetyClass.TickBoundarySafe:
      return { kind: "tick", tick: committedTick + 1, phase: "Phase01" };
    case LiveEditSafetyClass.EpochBoundarySafe: {
      // 严格晚于当前 Tick 的下一个 Epoch 边界
      const next = committedTick + 1;
      return { kind: "epoch", tick: Math.ceil(next / COUPLING_EPOCH_TICKS) * COUPLING_EPOCH_TICKS };
    }
    case LiveEditSafetyClass.RequiresForkRestart:
      return { kind: "fork" };
  }
}

interface Queued {
  readonly patch: WorldPatch;
  readonly timing: ApplyTiming;
}

/**
 * 实时编辑调度器：按分级把已合并的补丁分派到渲染帧、Tick Phase 01、Epoch 边界或 Fork 请求。
 * 同一批次内按 HLC 全序输出，保证所有协作端对运行中仿真施加完全相同的序列。
 */
export class LiveEditScheduler {
  private queue: Queued[] = [];
  private forks: WorldPatch[] = [];

  /** 入队；返回计算出的生效时机。分级声明不足的补丁被拒绝 */
  public submit(patch: WorldPatch, committedTick: number): ApplyTiming {
    if (!isSafetyClassSufficient(patch)) {
      throw new RangeError(`补丁声明的 ${patch.safety_class} 低于操作所需的 ${safetyClassName(requiredSafetyClass(patch.operations))}`);
    }
    const timing = scheduleFor(safetyClassValue(patch.safety_class), committedTick);
    if (timing.kind === "fork") this.forks.push(patch);
    else this.queue.push({ patch, timing });
    return timing;
  }

  public get pendingCount(): number {
    return this.queue.length;
  }

  /** 渲染帧：取出所有 Class 0 补丁 */
  public drainFrame(): WorldPatch[] {
    return this.take((q) => q.timing.kind === "frame");
  }

  /** 即将执行 `tick` 的 Phase 01：取出到期的 Class 1 与（Epoch 边界时）Class 2 补丁 */
  public drainTick(tick: number): WorldPatch[] {
    return this.take((q) => (q.timing.kind === "tick" || q.timing.kind === "epoch") && q.timing.tick <= tick);
  }

  /** 需要创建新 WorldFork 才能生效的补丁 */
  public drainForkRequests(): WorldPatch[] {
    const out = [...this.forks].sort(comparePatches);
    this.forks = [];
    return out;
  }

  private take(pred: (q: Queued) => boolean): WorldPatch[] {
    const due = this.queue.filter(pred);
    this.queue = this.queue.filter((q) => !pred(q));
    return due.map((q) => q.patch).sort(comparePatches);
  }
}
