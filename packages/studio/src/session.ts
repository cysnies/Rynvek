// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 实时协作编辑会话：把 HLC、CRDT 文档、撤销管理与热更新调度串成一条管线。
//
//   本地编辑 → UndoManager.commit → 整合进文档 → 按分级入调度队列 → 广播
//   远端补丁 → HLC.receive → 整合（幂等）→ 按分级入调度队列
//
// 运行中的仿真通过 `drainFrame / drainTick / drainForkRequests` 在正确的时机取出补丁。

import { AuthoringDocument, type AuthoringBaseline, type IntegrateResult, type WriteConflict } from "./authoring.js";
import { HybridLogicalClock } from "./hlc.js";
import type { WorldPatch, WorldPatchOp } from "./patch.js";
import { LiveEditScheduler, type ApplyTiming } from "./safety.js";
import { UndoManager, randomPatchId, type LocalEdit, type PatchIdGenerator } from "./undo.js";

export interface LiveEditSessionOptions {
  readonly baseline: AuthoringBaseline;
  readonly authorNodeId: number;
  /** 当前已提交的 Tick（通常取 `EngineHost.lastTick`） */
  readonly committedTick: () => number;
  readonly physicalNow?: () => number;
  readonly patchIds?: PatchIdGenerator;
  /** 本地产生的补丁需要广播给协作者 */
  readonly broadcast?: (patch: WorldPatch) => void;
}

export interface SessionEdit {
  readonly patch: WorldPatch;
  readonly timing: ApplyTiming | null;
  readonly conflicts: readonly WriteConflict[];
}

export class LiveEditSession {
  public readonly document: AuthoringDocument;
  public readonly clock: HybridLogicalClock;
  public readonly undo: UndoManager;
  public readonly scheduler = new LiveEditScheduler();

  public constructor(private readonly options: LiveEditSessionOptions) {
    this.document = new AuthoringDocument(options.baseline);
    this.clock = new HybridLogicalClock(options.authorNodeId, options.physicalNow);
    this.undo = new UndoManager(this.document, this.clock, options.patchIds ?? randomPatchId);
  }

  /** 本地编辑 */
  public edit(operations: readonly WorldPatchOp[]): SessionEdit {
    return this.publish(this.undo.commit(operations));
  }

  /** 把撤销 / 重做 / 热重载产生的本地编辑接入调度与广播 */
  public publish(edit: LocalEdit): SessionEdit {
    const timing = this.schedule(edit.patch, edit.result);
    this.options.broadcast?.(edit.patch);
    return { patch: edit.patch, timing, conflicts: edit.result.conflicts };
  }

  /** 接收远端补丁；重复到达返回 timing = null */
  public receive(patch: WorldPatch): SessionEdit {
    this.clock.receive(patch.hlc);
    const result = this.document.integrate(patch);
    return { patch, timing: this.schedule(patch, result), conflicts: result.conflicts };
  }

  private schedule(patch: WorldPatch, result: IntegrateResult): ApplyTiming | null {
    return result.applied ? this.scheduler.submit(patch, this.options.committedTick()) : null;
  }
}
