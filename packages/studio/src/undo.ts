// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 本地撤销 / 重做（动作 34 `studio.undo_patch`、35 `studio.redo_patch`）。
//
// 协作环境下撤销不能“回滚历史”，只能追加一个逆补丁：由 `AuthoringDocument.inverseOf`
// 计算，只回退仍由本人补丁胜出的寄存器。重做即对撤销补丁再取一次逆。

import type { AuthoringDocument, InverseResult, IntegrateResult } from "./authoring.js";
import type { HybridLogicalClock } from "./hlc.js";
import { patchIdHex, type WorldPatch, type WorldPatchOp } from "./patch.js";

/** 16 字节补丁 ID 生成器（缺省使用 Web Crypto） */
export type PatchIdGenerator = () => number[];

export const randomPatchId: PatchIdGenerator = () => Array.from(globalThis.crypto.getRandomValues(new Uint8Array(16)));

/** 撤销栈深度上限 */
export const UNDO_LIMIT = 256;

export interface LocalEdit {
  readonly patch: WorldPatch;
  readonly result: IntegrateResult;
  /** 撤销 / 重做时被跳过的寄存器 */
  readonly skipped: InverseResult["skipped"];
}

export class UndoManager {
  private readonly undoStack: string[] = [];
  private readonly redoStack: string[] = [];

  public constructor(
    private readonly doc: AuthoringDocument,
    private readonly clock: HybridLogicalClock,
    private readonly nextId: PatchIdGenerator = randomPatchId,
  ) {}

  public get canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  public get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  /** 提交一次本地编辑；返回需要广播给协作者的补丁 */
  public commit(operations: readonly WorldPatchOp[]): LocalEdit {
    const edit = this.emit(operations, []);
    this.push(this.undoStack, patchIdHex(edit.patch.patch_id));
    this.redoStack.length = 0;
    return edit;
  }

  /** 撤销最近一次本地编辑；没有可回退内容时返回 null */
  public undo(): LocalEdit | null {
    return this.flip(this.undoStack, this.redoStack);
  }

  public redo(): LocalEdit | null {
    return this.flip(this.redoStack, this.undoStack);
  }

  private flip(from: string[], to: string[]): LocalEdit | null {
    while (from.length > 0) {
      const id = from.pop() as string;
      const inverse = this.doc.inverseOf(id);
      if (inverse.operations.length === 0) continue; // 已被他人完全覆盖，跳到更早的一次
      const edit = this.emit(inverse.operations, inverse.skipped);
      this.push(to, patchIdHex(edit.patch.patch_id));
      return edit;
    }
    return null;
  }

  private emit(operations: readonly WorldPatchOp[], skipped: InverseResult["skipped"]): LocalEdit {
    const patch = this.doc.createPatch(this.clock, operations, this.nextId());
    const result = this.doc.integrate(patch);
    return { patch, result, skipped };
  }

  private push(stack: string[], id: string): void {
    stack.push(id);
    if (stack.length > UNDO_LIMIT) stack.shift();
  }
}
