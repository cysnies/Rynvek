// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 传输抽象：客户端只依赖 `send` + `onMessage`，可接 WebSocket、Worker、stdio 子进程或测试用回放器。
 */

import type { ArenaClientMessage, ArenaServerMessage } from "./protocol.js";

export interface ArenaTransport {
  send(message: ArenaClientMessage): void;
  /** 订阅服务端消息，返回取消函数 */
  onMessage(handler: (message: ArenaServerMessage) => void): () => void;
  close?(): void;
}

/** 浏览器 `WebSocket` 的最小形状（避免依赖 DOM lib） */
export interface WebSocketLike {
  send(data: string): void;
  close(): void;
  addEventListener(type: "message", listener: (ev: { data: unknown }) => void): void;
  removeEventListener(type: "message", listener: (ev: { data: unknown }) => void): void;
}

/** 基于 WebSocket 的 JSON 文本帧传输 */
export class WebSocketTransport implements ArenaTransport {
  constructor(private readonly socket: WebSocketLike) {}

  send(message: ArenaClientMessage): void {
    this.socket.send(JSON.stringify(message));
  }

  onMessage(handler: (message: ArenaServerMessage) => void): () => void {
    const listener = (ev: { data: unknown }) => {
      if (typeof ev.data === "string") handler(JSON.parse(ev.data) as ArenaServerMessage);
    };
    this.socket.addEventListener("message", listener);
    return () => this.socket.removeEventListener("message", listener);
  }

  close(): void {
    this.socket.close();
  }
}

/**
 * 内存直连传输：`deliver` 由测试或本地服务端调用，向订阅者同步分发消息；`sent` 记录所有发出的消息。
 */
export class MemoryTransport implements ArenaTransport {
  readonly sent: ArenaClientMessage[] = [];
  private readonly handlers = new Set<(message: ArenaServerMessage) => void>();

  constructor(private readonly onSend?: (message: ArenaClientMessage) => void) {}

  send(message: ArenaClientMessage): void {
    this.sent.push(message);
    this.onSend?.(message);
  }

  onMessage(handler: (message: ArenaServerMessage) => void): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  deliver(message: ArenaServerMessage): void {
    for (const h of [...this.handlers]) h(message);
  }
}
