// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { type KeyboardEvent, type ReactNode, useId, useRef, useState } from "react";

export interface TabItem {
  readonly id: string;
  readonly label: string;
  readonly content: ReactNode;
}

export interface TabsProps {
  readonly items: readonly TabItem[];
  /** 受控选中项 */
  readonly value?: string;
  readonly defaultValue?: string;
  readonly onValueChange?: (id: string) => void;
  readonly label: string;
}

/**
 * 标签页（WAI-ARIA Tabs 模式）：方向键 / Home / End 在标签间移动焦点并激活，
 * 只有选中的标签处于 Tab 序列中（roving tabindex）。
 */
export function Tabs({ items, value, defaultValue, onValueChange, label }: TabsProps) {
  const base = useId();
  const [inner, setInner] = useState(defaultValue ?? items[0]?.id ?? "");
  const selected = value ?? inner;
  const refs = useRef(new Map<string, HTMLButtonElement>());

  const select = (id: string) => {
    if (value === undefined) setInner(id);
    onValueChange?.(id);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const idx = items.findIndex((t) => t.id === selected);
    if (idx < 0 || items.length === 0) return;
    let next = -1;
    if (e.key === "ArrowRight") next = (idx + 1) % items.length;
    else if (e.key === "ArrowLeft") next = (idx - 1 + items.length) % items.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = items.length - 1;
    if (next < 0) return;
    e.preventDefault();
    const id = items[next]!.id;
    select(id);
    refs.current.get(id)?.focus();
  };

  const current = items.find((t) => t.id === selected);
  return (
    <div>
      <div role="tablist" aria-label={label} className="rv-tablist" onKeyDown={onKeyDown}>
        {items.map((t) => (
          <button
            key={t.id}
            ref={(el) => {
              if (el) refs.current.set(t.id, el);
              else refs.current.delete(t.id);
            }}
            type="button"
            role="tab"
            id={`${base}-tab-${t.id}`}
            aria-selected={t.id === selected}
            aria-controls={`${base}-panel-${t.id}`}
            tabIndex={t.id === selected ? 0 : -1}
            className="rv-tab"
            onClick={() => select(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>
      {current ? (
        <div role="tabpanel" id={`${base}-panel-${current.id}`} aria-labelledby={`${base}-tab-${current.id}`}>
          {current.content}
        </div>
      ) : null}
    </div>
  );
}
