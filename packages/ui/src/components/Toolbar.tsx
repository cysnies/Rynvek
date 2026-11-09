// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import type { ReactNode } from "react";
import { type ActionBinding, formatChord } from "../actions.js";

export interface ToolbarProps {
  readonly label: string;
  readonly children?: ReactNode;
}

/** 工具栏容器 */
export function Toolbar({ label, children }: ToolbarProps) {
  return (
    <div role="toolbar" aria-label={label} className="rv-toolbar">
      {children}
    </div>
  );
}

export interface StatusBarProps {
  readonly children?: ReactNode;
}

/** 底部状态栏（等宽字体显示 Tick、后端、帧率等） */
export function StatusBar({ children }: StatusBarProps) {
  return (
    <footer role="status" className="rv-statusbar">
      {children}
    </footer>
  );
}

export interface KbdProps {
  /** 显示某个动作的首选键盘绑定 */
  readonly binding: ActionBinding;
}

/** 快捷键提示：显示动作的第一个键盘组合键；纯指针动作显示规范中的桌面绑定文本 */
export function Kbd({ binding }: KbdProps) {
  const first = binding.keyboard[0];
  return <kbd className="rv-kbd">{first ? formatChord(first) : binding.desktop}</kbd>;
}
