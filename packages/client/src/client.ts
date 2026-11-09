// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `ArenaClient`：一个 Principal（人类 / 脚本 / Agent）的对局连接。
 *
 * - 维护单调递增的请求序号 `seq`，按序号把 `ack` / `lease` / `tool_result` 关联回 Promise；
 * - 持有当前 `ControlLease`，每条命令自动附带 Fencing Token；被人类接管（`lease_revoked`）后立即作废本地租约；
 * - 本地预检：控制等级不足（`0x5006`）、未持有租约（`0x8001`）与 APM 用尽（`0x8004`）在发出前直接拒绝，
 *   这些拒绝与服务端使用相同错误码，但不占用网络与权威 APM；
 * - 缓存最新观测、事件流与解码后的地形。
 */

import { RynvekErrorCode } from "@rynvek/core";

import { ApmBudget } from "./apm.js";
import {
  ARENA_PROTOCOL,
  CONTROL_LEVEL_ORDER,
  commandLevel,
  type ArenaClientMessage,
  type ArenaServerMessage,
  type CommandAck,
  type ControlLease,
  type ControlLevel,
  type ErrorBody,
  type FactionObservation,
  type MapInfo,
  type MatchMode,
  type PrincipalKind,
  type RtsCommand,
  type RtsEvent,
  type ToolResponse,
  type ToolSchema,
} from "./protocol.js";
import { TerrainGrid } from "./terrain.js";
import type { ArenaTransport } from "./transport.js";

/** 带稳定错误码的客户端错误 */
export class ArenaError extends Error {
  readonly code: number;
  readonly codeName: string;

  constructor(body: ErrorBody) {
    super(`[0x${body.code.toString(16).padStart(4, "0")}] ${body.message}`);
    this.name = "ArenaError";
    this.code = body.code;
    this.codeName = body.name;
  }
}

export interface ArenaIdentity {
  readonly principalId: string;
  readonly kind: PrincipalKind;
  readonly faction: number;
}

export interface WelcomeInfo {
  readonly faction: number;
  readonly mode: MatchMode;
  readonly tick: number;
  readonly map: MapInfo;
  readonly tools: readonly ToolSchema[];
}

export type ArenaClientEvent =
  | { readonly type: "observation"; readonly observation: FactionObservation; readonly events: readonly RtsEvent[] }
  | { readonly type: "lease"; readonly lease: ControlLease | null }
  | { readonly type: "lease_revoked"; readonly by: string }
  | { readonly type: "error"; readonly error: ArenaError };

interface Pending {
  resolve(value: unknown): void;
  reject(error: ArenaError): void;
}

const local = (code: number, name: string, message: string) => new ArenaError({ code, name, message });

export class ArenaClient {
  private seq = 0;
  private readonly pending = new Map<number, Pending>();
  private readonly observationWaiters: ((o: FactionObservation) => void)[] = [];
  private readonly listeners = new Set<(e: ArenaClientEvent) => void>();
  private welcomeWaiter: { resolve(w: WelcomeInfo): void; reject(e: ArenaError): void } | null = null;
  private readonly unsubscribe: () => void;

  readonly apm = new ApmBudget();
  welcome: WelcomeInfo | null = null;
  terrain: TerrainGrid | null = null;
  lease: ControlLease | null = null;
  observation: FactionObservation | null = null;
  /** 收到的全部事件（按到达顺序） */
  readonly events: RtsEvent[] = [];

  constructor(
    private readonly transport: ArenaTransport,
    readonly identity: ArenaIdentity,
  ) {
    this.unsubscribe = transport.onMessage((m) => this.receive(m));
  }

  /** 当前权威 Tick（以最新观测为准） */
  get tick(): number {
    return this.observation?.tick ?? this.welcome?.tick ?? 0;
  }

  on(listener: (e: ArenaClientEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(e: ArenaClientEvent): void {
    for (const l of [...this.listeners]) l(e);
  }

  private send(message: ArenaClientMessage): void {
    this.transport.send(message);
  }

  private request<T>(build: (seq: number) => ArenaClientMessage): Promise<T> {
    const seq = ++this.seq;
    const p = new Promise<T>((resolve, reject) => {
      this.pending.set(seq, { resolve: resolve as (v: unknown) => void, reject });
    });
    this.send(build(seq));
    return p;
  }

  private settle(seq: number | null, value: unknown, error: ErrorBody | undefined): void {
    if (seq === null) return;
    const p = this.pending.get(seq);
    if (!p) return;
    this.pending.delete(seq);
    if (error) p.reject(new ArenaError(error));
    else p.resolve(value);
  }

  private receive(m: ArenaServerMessage): void {
    switch (m.type) {
      case "welcome": {
        const info: WelcomeInfo = { faction: m.faction, mode: m.mode, tick: m.tick, map: m.map, tools: m.tools };
        this.welcome = info;
        this.terrain = TerrainGrid.decode(m.map);
        this.welcomeWaiter?.resolve(info);
        this.welcomeWaiter = null;
        break;
      }
      case "lease":
        // 成功的申请带回新租约；成功的交还不带租约；失败时本地租约保持不变
        if (!m.error) this.lease = m.lease ?? null;
        this.settle(m.seq, m.lease ?? null, m.error);
        if (!m.error) this.emit({ type: "lease", lease: this.lease });
        break;
      case "lease_revoked":
        this.lease = null;
        this.emit({ type: "lease_revoked", by: m.by });
        break;
      case "ack":
        if (m.ack) this.apm.record(m.tick);
        this.settle(m.seq, m.ack, m.error);
        break;
      case "tool_result":
        this.settle(m.seq, m.response, undefined);
        break;
      case "observation": {
        this.observation = m.observation;
        this.events.push(...m.events);
        const waiters = this.observationWaiters.splice(0);
        for (const w of waiters) w(m.observation);
        this.emit({ type: "observation", observation: m.observation, events: m.events });
        break;
      }
      case "error": {
        const e = new ArenaError(m.error);
        if (this.welcomeWaiter) {
          this.welcomeWaiter.reject(e);
          this.welcomeWaiter = null;
        }
        this.emit({ type: "error", error: e });
        break;
      }
    }
  }

  /** 入座并等待 `welcome` */
  connect(): Promise<WelcomeInfo> {
    const p = new Promise<WelcomeInfo>((resolve, reject) => {
      this.welcomeWaiter = { resolve, reject };
    });
    this.send({
      type: "hello",
      protocol: ARENA_PROTOCOL,
      principal_id: this.identity.principalId,
      kind: this.identity.kind,
      faction: this.identity.faction,
    });
    return p;
  }

  acquireLease(level: ControlLevel): Promise<ControlLease> {
    return this.request<ControlLease>((seq) => ({ type: "acquire_lease", seq, level }));
  }

  /** 交还租约（动作 27 `rts.release_human_lease`） */
  async releaseLease(): Promise<void> {
    const lease = this.lease;
    if (!lease) throw local(RynvekErrorCode.UnauthorizedCapabilityToken, "UnauthorizedCapabilityToken", "未持有租约");
    await this.request<null>((seq) => ({ type: "release_lease", seq, fencing_token: lease.fencing_token }));
  }

  /** 本地预检（与服务端同码） */
  precheck(cmd: RtsCommand): ArenaError | null {
    const lease = this.lease;
    if (!lease) return local(RynvekErrorCode.UnauthorizedCapabilityToken, "UnauthorizedCapabilityToken", "未持有控制租约");
    const need = commandLevel(cmd);
    if (CONTROL_LEVEL_ORDER[lease.level] < CONTROL_LEVEL_ORDER[need]) {
      return local(RynvekErrorCode.PluginCapabilityDenied, "PluginCapabilityDenied", `该命令需要控制等级 ${need}`);
    }
    if (this.apm.remaining(this.tick) <= 0) {
      return local(RynvekErrorCode.CommandRateLimitExceeded, "CommandRateLimitExceeded", "本地 APM 预算已用尽");
    }
    return null;
  }

  command(cmd: RtsCommand): Promise<CommandAck> {
    const err = this.precheck(cmd);
    if (err) return Promise.reject(err);
    const token = this.lease!.fencing_token;
    return this.request<CommandAck>((seq) => ({ type: "command", seq, fencing_token: token, command: cmd }));
  }

  /** LLM 工具调用；工具层拒绝体现在 `ToolResponse.error`，Promise 本身不 reject */
  callTool(tool: string, args: unknown): Promise<ToolResponse> {
    const token = this.lease?.fencing_token ?? 0;
    return this.request<ToolResponse>((seq) => ({ type: "tool_call", seq, fencing_token: token, tool, arguments: args }));
  }

  /** 等待下一份观测（不发送任何消息） */
  nextObservation(): Promise<FactionObservation> {
    return new Promise((resolve) => this.observationWaiters.push(resolve));
  }

  /** 推进 `ticks` 并等待观测（本地练习 / PvE / 测试） */
  advance(ticks: number): Promise<FactionObservation> {
    const p = this.nextObservation();
    this.send({ type: "advance", ticks });
    return p;
  }

  dispose(): void {
    this.unsubscribe();
  }
}
