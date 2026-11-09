// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `600 APM` 预算的客户端镜像（与 Rust `rynvek_agent::ApmLimiter` 同一滑动窗口语义）：
 * 任意连续 `1,800 Ticks` 内至多 600 个动作。权威计数在服务端；客户端只用于 HUD 显示与本地预拦截，
 * 避免把注定被 `0x8004` 拒绝的命令发上网络。
 */

export const APM_WINDOW_TICKS = 1800;
export const APM_MAX_ACTIONS = 600;

export class ApmBudget {
  private readonly actions: number[] = [];

  constructor(
    readonly windowTicks = APM_WINDOW_TICKS,
    readonly maxActions = APM_MAX_ACTIONS,
  ) {}

  private expire(tick: number): void {
    while (this.actions.length > 0 && this.actions[0]! + this.windowTicks <= tick) this.actions.shift();
  }

  used(tick: number): number {
    this.expire(tick);
    return this.actions.length;
  }

  remaining(tick: number): number {
    return this.maxActions - this.used(tick);
  }

  /** 记录一个已被服务端接受的动作 */
  record(tick: number): void {
    this.expire(tick);
    this.actions.push(tick);
  }
}
