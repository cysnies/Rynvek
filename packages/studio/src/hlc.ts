// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 混合逻辑时钟（Kulkarni et al., 2014）：物理时间 + 逻辑计数，作者 ID 作末位决胜，
// 给多创作者补丁一个与因果一致的全序（规范 10.2 `HlcTimestamp`）。

import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import type { HlcTimestamp } from "./patch.js";

/** 远端时间戳领先本地物理时钟的最大容忍量；超出视为时钟故障或恶意节点 */
export const HLC_MAX_DRIFT_MS = 60_000;
const U32_MAX = 0xffff_ffff;

export class HybridLogicalClock {
  private wall = 0;
  private counter = 0;

  public constructor(
    public readonly nodeId: number,
    private readonly physicalNow: () => number = () => Date.now(),
    private readonly maxDriftMs = HLC_MAX_DRIFT_MS,
  ) {
    if (!Number.isInteger(nodeId) || nodeId < 0 || nodeId > U32_MAX) throw new RangeError("author_node_id 必须是 u32");
  }

  /** 最近一次发出或合并后的时间戳 */
  public peek(): HlcTimestamp {
    return { wall_time_ms: this.wall, logical_counter: this.counter, author_node_id: this.nodeId };
  }

  /** 本地事件 / 发送：生成严格递增的新时间戳 */
  public now(): HlcTimestamp {
    const pt = this.physical();
    if (pt > this.wall) {
      this.wall = pt;
      this.counter = 0;
    } else {
      this.bump();
    }
    return this.peek();
  }

  /** 接收远端时间戳：推进本地时钟，使之后的本地事件因果地晚于该远端事件 */
  public receive(remote: HlcTimestamp): HlcTimestamp {
    const pt = this.physical();
    if (remote.wall_time_ms - pt > this.maxDriftMs) {
      throw new RynvekError(
        RynvekErrorCode.SimulationInvariantViolated,
        `HLC 远端时间领先本地 ${remote.wall_time_ms - pt} ms，超过 ${this.maxDriftMs} ms`,
      );
    }
    const wall = Math.max(this.wall, remote.wall_time_ms, pt);
    if (wall === this.wall && wall === remote.wall_time_ms) {
      this.counter = Math.max(this.counter, remote.logical_counter);
      this.bumpAt(wall);
    } else if (wall === this.wall) {
      this.bumpAt(wall);
    } else if (wall === remote.wall_time_ms) {
      this.wall = wall;
      this.counter = remote.logical_counter;
      this.bumpAt(wall);
    } else {
      this.wall = wall;
      this.counter = 0;
    }
    return this.peek();
  }

  private physical(): number {
    const pt = Math.floor(this.physicalNow());
    if (!Number.isSafeInteger(pt) || pt < 0) throw new RangeError("物理时钟必须是非负整数毫秒");
    return pt;
  }

  private bumpAt(wall: number): void {
    this.wall = wall;
    this.bump();
  }

  /** 计数器 +1；u32 溢出时借位到物理时间（极端情况下保持严格单调） */
  private bump(): void {
    if (this.counter === U32_MAX) {
      this.wall += 1;
      this.counter = 0;
    } else {
      this.counter += 1;
    }
  }
}
