// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `InputActionRouter`：规范 12.1 的运行期全局输入路由（GAP-27，D642–D644）。
 *
 * 业务组件只订阅逻辑动作（`LogicalActionId`），不直接监听 `keydown`。键盘匹配复用 `@rynvek/ui::matchKeyboard`
 * （同一张 36 项表，ERRATA E-017）；本模块补上：
 *
 * - 作用域栈：`Global` 恒定激活，其余作用域由界面按焦点压栈 / 出栈；
 * - 指针手势：左键单击 / Shift / Ctrl / 双击、左键拖框、右键、滚轮；
 * - "按键 + 左键" 组合（`Q` + 左键进攻移动、`P` + 左键巡逻）：按下 `Q` 后进入待定状态，下一次左键点击触发，
 *   `Escape` 或其他动作取消；
 * - 文本输入框内不触发不带修饰键的单字符快捷键（与 Studio 一致，ERRATA E-021）。
 */

import {
  ACTION_BINDINGS,
  matchKeyboard,
  normalizeKey,
  type InteractionContext,
  type KeyEventLike,
  type LogicalActionId,
  type PointerGesture,
} from "@rynvek/ui";

export interface RouterKeyEvent extends KeyEventLike {
  /** 事件目标是否为文本输入（input / textarea / contenteditable） */
  readonly isTextInput?: boolean;
}

export type RouterPointerEvent =
  | {
      readonly kind: "click";
      readonly button: "left" | "right";
      readonly x: number;
      readonly y: number;
      readonly shift?: boolean;
      readonly ctrl?: boolean;
      readonly double?: boolean;
    }
  | { readonly kind: "drag"; readonly button: "left"; readonly x: number; readonly y: number; readonly x2: number; readonly y2: number }
  | { readonly kind: "wheel"; readonly direction: "up" | "down"; readonly x: number; readonly y: number };

/** 派发给处理器的动作调用 */
export interface ActionInvocation {
  readonly id: LogicalActionId;
  readonly source: "keyboard" | "pointer";
  /** 指针坐标（世界 / 屏幕坐标由调用方约定） */
  readonly point?: readonly [number, number];
  /** 拖框的另一角 */
  readonly point2?: readonly [number, number];
  /** 编队编号 1..9 */
  readonly group?: number;
}

export type ActionHandler = (invocation: ActionInvocation) => void;

function gestureMatches(g: PointerGesture, e: RouterPointerEvent): boolean {
  switch (g.kind) {
    case "wheel":
      return e.kind === "wheel" && e.direction === g.direction;
    case "drag":
      return e.kind === "drag";
    case "click": {
      if (e.kind !== "click" || e.button !== g.button) return false;
      const modifier = e.shift ? "shift" : e.ctrl ? "ctrl" : undefined;
      return (g.modifier ?? undefined) === modifier && (g.double ?? false) === (e.double ?? false);
    }
    case "key_then_click":
      return false; // 由待定按键状态机处理
  }
}

export class InputActionRouter {
  private readonly stack: InteractionContext[] = [];
  private readonly handlers = new Map<LogicalActionId, Set<ActionHandler>>();
  private armedKey: string | null = null;

  constructor(initial: readonly InteractionContext[] = []) {
    for (const c of initial) this.pushContext(c);
  }

  /** 当前激活的作用域（含恒定的 `Global`） */
  get contexts(): readonly InteractionContext[] {
    return ["Global", ...this.stack];
  }

  /** 已按下、等待左键的组合键（`q` / `p`） */
  get armed(): string | null {
    return this.armedKey;
  }

  pushContext(context: InteractionContext): void {
    if (context !== "Global" && !this.stack.includes(context)) this.stack.push(context);
  }

  popContext(context: InteractionContext): void {
    const i = this.stack.lastIndexOf(context);
    if (i >= 0) this.stack.splice(i, 1);
  }

  on(id: LogicalActionId, handler: ActionHandler): () => void {
    let set = this.handlers.get(id);
    if (!set) this.handlers.set(id, (set = new Set()));
    set.add(handler);
    return () => set.delete(handler);
  }

  private dispatch(inv: ActionInvocation): boolean {
    const set = this.handlers.get(inv.id);
    if (!set || set.size === 0) return false;
    for (const h of [...set]) h(inv);
    return true;
  }

  private keyThenClickBinding(key: string): LogicalActionId | null {
    for (const b of ACTION_BINDINGS) {
      if (!this.contexts.includes(b.context)) continue;
      if (b.pointer.some((g) => g.kind === "key_then_click" && g.key === key)) return b.id;
    }
    return null;
  }

  /** 处理键盘事件；返回是否被某个逻辑动作消费（调用方据此 `preventDefault`） */
  handleKey(e: RouterKeyEvent): boolean {
    const plain = !e.ctrlKey && !e.metaKey && !e.altKey;
    if (e.isTextInput && plain && normalizeKey(e.key).length === 1) return false;
    const key = normalizeKey(e.key);
    if (plain && !e.shiftKey && this.keyThenClickBinding(key)) {
      this.armedKey = key;
      return true;
    }
    const m = matchKeyboard(e, this.contexts);
    if (!m) return false;
    if (m.binding.id === "global.cancel_or_menu") this.armedKey = null;
    return this.dispatch(m.group === undefined ? { id: m.binding.id, source: "keyboard" } : { id: m.binding.id, source: "keyboard", group: m.group });
  }

  /** 处理指针事件 */
  handlePointer(e: RouterPointerEvent): boolean {
    const point = [e.x, e.y] as const;
    if (this.armedKey !== null && e.kind === "click" && e.button === "left") {
      const id = this.keyThenClickBinding(this.armedKey);
      this.armedKey = null;
      if (id) return this.dispatch({ id, source: "pointer", point });
    }
    if (e.kind === "click" && e.button === "right") this.armedKey = null;
    let best: { id: LogicalActionId; rank: number } | null = null;
    for (const b of ACTION_BINDINGS) {
      if (!this.contexts.includes(b.context)) continue;
      if (!b.pointer.some((g) => gestureMatches(g, e))) continue;
      const rank = this.stack.indexOf(b.context);
      if (!best || rank > best.rank) best = { id: b.id, rank };
    }
    if (!best) return false;
    return e.kind === "drag"
      ? this.dispatch({ id: best.id, source: "pointer", point, point2: [e.x2, e.y2] })
      : this.dispatch({ id: best.id, source: "pointer", point });
  }
}
