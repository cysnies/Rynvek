// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 多席位会话集线器：
 * - `ReplayHub` 逐条比对各客户端发出的消息与夹具中的 `send`，再把夹具中的 `recv` 投递给对应收件人；
 *   Rust 侧 `rynvek-engine/tests/arena_loop.rs` 用同一夹具驱动真实 `ArenaSession` 并逐字节比对 `recv`，
 *   从而把 TypeScript 前端与 Rust 权威层闭合成一个可无头复现的回路。
 * - `LiveHub` 启动 `arena_server` 无头会话进程（JSON Lines），实时转发并录制夹具。
 */

import { spawn, type ChildProcessByStdio } from "node:child_process";
import { createInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";

import { MemoryTransport, type ArenaClientMessage, type Envelope } from "@rynvek/client";
import { expect } from "vitest";

export interface TranscriptStep {
  readonly from: string;
  readonly send: ArenaClientMessage;
  readonly recv: readonly Envelope[];
}

export interface SessionHub {
  transportFor(principalId: string): MemoryTransport;
  /** 已完成的步骤（录制结果或已回放的前缀） */
  readonly steps: readonly TranscriptStep[];
  close(): void;
}

abstract class BaseHub implements SessionHub {
  protected readonly transports = new Map<string, MemoryTransport>();
  readonly steps: TranscriptStep[] = [];

  transportFor(principalId: string): MemoryTransport {
    let t = this.transports.get(principalId);
    if (!t) {
      t = new MemoryTransport((m) => this.onSend(principalId, m));
      this.transports.set(principalId, t);
    }
    return t;
  }

  protected deliver(envelopes: readonly Envelope[]): void {
    for (const e of envelopes) this.transports.get(e.to)?.deliver(e.message);
  }

  protected abstract onSend(from: string, message: ArenaClientMessage): void;

  close(): void {}
}

export class ReplayHub extends BaseHub {
  private cursor = 0;

  constructor(private readonly transcript: readonly TranscriptStep[]) {
    super();
  }

  get remaining(): number {
    return this.transcript.length - this.cursor;
  }

  protected onSend(from: string, message: ArenaClientMessage): void {
    const step = this.transcript[this.cursor++];
    expect(step, `夹具已耗尽，却收到 ${from} 的 ${message.type}`).toBeDefined();
    expect({ from, send: message }).toEqual({ from: step!.from, send: step!.send });
    this.steps.push(step!);
    queueMicrotask(() => this.deliver(step!.recv));
  }
}

export class LiveHub extends BaseHub {
  private readonly child: ChildProcessByStdio<Writable, Readable, null>;
  private readonly queue: { from: string; send: ArenaClientMessage }[] = [];

  constructor(command: string, args: readonly string[] = []) {
    super();
    this.child = spawn(command, args, { stdio: ["pipe", "pipe", "inherit"] });
    createInterface({ input: this.child.stdout }).on("line", (line) => {
      const head = this.queue.shift();
      if (!head) throw new Error(`会话进程多余输出：${line}`);
      const recv = JSON.parse(line) as Envelope[];
      this.steps.push({ ...head, recv });
      this.deliver(recv);
    });
  }

  protected onSend(from: string, message: ArenaClientMessage): void {
    this.queue.push({ from, send: message });
    this.child.stdin.write(`${JSON.stringify({ from, message })}\n`);
  }

  override close(): void {
    this.child.stdin.end();
    this.child.kill();
  }
}

/** 夹具序列化：每步一行，便于审阅差异 */
export function serializeTranscript(steps: readonly TranscriptStep[]): string {
  return `[\n${steps.map((s) => JSON.stringify(s)).join(",\n")}\n]\n`;
}
