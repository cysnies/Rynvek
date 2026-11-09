// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * Command / Observation / Event 协议信封，JSON 形状与 Rust `rynvek_protocol::envelope` 的 serde 输出一致。
 *
 * 64 位整数字段在 JSON 中为数字，TypeScript 端要求其不超过 `Number.MAX_SAFE_INTEGER`。
 */

/** 客户端 / Agent 提交给权威世界的命令信封 */
export interface CommandEnvelope<P> {
  /** UUIDv7 幂等键 */
  readonly idempotency_key: string;
  readonly principal_id: number;
  readonly issued_at_tick: number;
  readonly target_tick: number;
  readonly payload: P;
}

/** 命令可接受的 Tick 时间窗 */
export interface CommandTickWindow {
  readonly maxPastTicks: number;
  readonly maxFutureTicks: number;
}

/** 时间窗校验结果（`Expired` / `TooFarInFuture` 对应错误码 `0x8003`） */
export type CommandWindowCheck = "Ok" | "Expired" | "TooFarInFuture";

/** 校验 `target_tick` 是否落在以 `currentTick` 为中心的可接受时间窗内（边界包含） */
export function checkTickWindow(
  command: Pick<CommandEnvelope<unknown>, "target_tick">,
  currentTick: number,
  window: CommandTickWindow,
): CommandWindowCheck {
  if (command.target_tick + window.maxPastTicks < currentTick) {
    return "Expired";
  }
  if (command.target_tick > currentTick + window.maxFutureTicks) {
    return "TooFarInFuture";
  }
  return "Ok";
}

/** 权威世界发布给观察者的只读观测信封 */
export interface ObservationEnvelope<P> {
  readonly world_id: string;
  readonly tick: number;
  readonly world_revision: number;
  /** 64 位小写十六进制 BLAKE3 状态根 */
  readonly state_root_hash: string;
  readonly observer_principal_id: number;
  readonly payload: P;
}

/** 事件因果全序键 */
export interface EventKey {
  readonly tick: number;
  readonly phase_id: number;
  readonly emitter_serial: number;
  readonly sequence: number;
}

/** 已提交的权威事件信封 */
export interface EventEnvelope<P> {
  readonly key: EventKey;
  readonly event_kind: string;
  readonly payload: P;
}

/** 按因果全序比较两个事件键：Tick → 阶段 → 发射者 → 序号（与 Rust `Ord` 派生一致） */
export function compareEventKeys(a: EventKey, b: EventKey): number {
  return a.tick - b.tick || a.phase_id - b.phase_id || a.emitter_serial - b.emitter_serial || a.sequence - b.sequence;
}
