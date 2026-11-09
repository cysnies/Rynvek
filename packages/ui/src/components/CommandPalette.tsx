// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { type KeyboardEvent, type ReactNode, useEffect, useId, useMemo, useRef, useState } from "react";
import { fuzzyFilter } from "../fuzzy.js";

export interface PaletteCommand {
  readonly id: string;
  readonly title: string;
  /** 右侧提示（通常是快捷键） */
  readonly hint?: ReactNode;
  readonly disabled?: boolean;
  run(): void;
}

export interface CommandPaletteProps {
  readonly open: boolean;
  readonly commands: readonly PaletteCommand[];
  readonly onClose: () => void;
  readonly placeholder?: string;
}

function highlight(text: string, positions: readonly number[]): ReactNode {
  if (positions.length === 0) return text;
  const set = new Set(positions);
  return [...text].map((ch, i) => (set.has(i) ? <mark key={i}>{ch}</mark> : ch));
}

/**
 * 命令面板（`global.command_palette`）：组合框 + 列表框模式。
 * 输入即模糊过滤；↑/↓ 移动高亮，Enter 执行，Escape 关闭；执行后自动关闭。
 */
export function CommandPalette({ open, commands, onClose, placeholder = "输入命令…" }: CommandPaletteProps) {
  const listId = useId();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const results = useMemo(() => fuzzyFilter(query, commands.filter((c) => !c.disabled), (c) => c.title), [query, commands]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      input.current?.focus();
    }
  }, [open]);

  if (!open) return null;

  const run = (index: number) => {
    const r = results[index];
    if (!r) return;
    onClose();
    r.item.run();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => (results.length === 0 ? 0 : (a + 1) % results.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => (results.length === 0 ? 0 : (a - 1 + results.length) % results.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(active);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  const activeId = results[active] ? `${listId}-${results[active].item.id}` : undefined;
  return (
    <div className="rv-palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="命令面板" className="rv-palette">
        <input
          ref={input}
          className="rv-input"
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-activedescendant={activeId}
          aria-autocomplete="list"
          placeholder={placeholder}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
        />
        <ul role="listbox" id={listId} className="rv-palette-list">
          {results.map((r, i) => (
            <li
              key={r.item.id}
              id={`${listId}-${r.item.id}`}
              role="option"
              aria-selected={i === active}
              className="rv-palette-item"
              onMouseEnter={() => setActive(i)}
              onClick={() => run(i)}
            >
              <span>{highlight(r.item.title, r.positions)}</span>
              {r.item.hint ? <span>{r.item.hint}</span> : null}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
