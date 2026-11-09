// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { type ReactNode, useId } from "react";

export interface PanelProps {
  readonly title: string;
  /** 标题栏右侧操作区 */
  readonly actions?: ReactNode;
  readonly children?: ReactNode;
  readonly className?: string;
}

/** 面板：带标题栏的区域，标题作为可访问名称 */
export function Panel({ title, actions, children, className }: PanelProps) {
  const id = useId();
  return (
    <section className={className ? `rv-panel ${className}` : "rv-panel"} aria-labelledby={id}>
      <header className="rv-panel-header">
        <span id={id}>{title}</span>
        {actions ? <span>{actions}</span> : null}
      </header>
      <div className="rv-panel-body">{children}</div>
    </section>
  );
}
