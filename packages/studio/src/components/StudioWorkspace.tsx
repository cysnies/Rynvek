// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// Studio 外壳：顶栏（仿真控制、工具、撤销重做）+ 可停靠工作区 + 状态栏，
// 并把键盘事件按焦点所在面板的交互上下文交给 `StudioCommands`。

import { Button, Kbd, StatusBar, Toolbar, actionBinding, type InteractionContext, type LogicalActionId } from "@rynvek/ui";
import { type KeyboardEvent, type ReactNode, useEffect, useState } from "react";
import type { StudioCommandState, StudioCommands } from "../commands.js";
import { STUDIO_PANEL_TITLES, type DockLayout, type StudioPanelId } from "../dock.js";
import { DockWorkspace } from "./DockWorkspace.js";

export interface StudioWorkspaceProps {
  readonly commands: StudioCommands;
  readonly layout: DockLayout;
  readonly onLayoutChange: (layout: DockLayout) => void;
  readonly renderPanel: (panelId: string) => ReactNode;
  /** 状态栏内容（Tick、后端、修订号等） */
  readonly status?: ReactNode;
}

/** 面板的交互上下文：规则编辑器内启用 `Code_Editor`（如 Ctrl+S 热重载） */
export const PANEL_CONTEXTS: Readonly<Partial<Record<StudioPanelId, InteractionContext>>> = {
  rule_editor: "Code_Editor",
};

function contextsFor(target: EventTarget | null): InteractionContext[] {
  const ctx: InteractionContext[] = ["Global", "Studio_IDE"];
  const el = target instanceof Element ? target.closest<HTMLElement>("[data-rv-context]") : null;
  const extra = el?.dataset["rvContext"] as InteractionContext | undefined;
  if (extra !== undefined) ctx.push(extra);
  return ctx;
}

function useCommandState(commands: StudioCommands): StudioCommandState {
  const [state, setState] = useState(commands.state);
  useEffect(() => commands.subscribe(setState), [commands]);
  return state;
}

function ActionButton({
  commands,
  action,
  label,
  pressed,
}: {
  commands: StudioCommands;
  action: Parameters<StudioCommands["run"]>[0];
  label: string;
  pressed?: boolean;
}) {
  const binding = actionBinding(action as LogicalActionId);
  return (
    <Button
      variant={pressed ? "primary" : "ghost"}
      size="sm"
      aria-pressed={pressed}
      data-action={action}
      onClick={() => commands.run(action)}
    >
      {label}
      {binding ? <Kbd binding={binding} /> : null}
    </Button>
  );
}

export function StudioWorkspace({ commands, layout, onLayoutChange, renderPanel, status }: StudioWorkspaceProps) {
  const state = useCommandState(commands);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    // 文本输入框内的普通字符键不作为快捷键（否则无法在编辑器里输入 G、V 等字母）
    const target = e.target as HTMLElement;
    const typing = target.closest("input, textarea, [contenteditable='true']") !== null;
    if (typing && !e.ctrlKey && !e.metaKey && !e.altKey) return;
    if (commands.handleKey(e, contextsFor(e.target)) !== null) e.preventDefault();
  };

  return (
    <div className="rv-studio" onKeyDown={onKeyDown}>
      <Toolbar label="Studio 工具栏">
        <ActionButton commands={commands} action="studio.sim_play_pause" label={state.playing ? "暂停" : "播放"} pressed={state.playing} />
        <ActionButton commands={commands} action="studio.sim_step_single_tick" label="单步" />
        <ActionButton commands={commands} action="studio.tool_brush_field" label="场画笔" pressed={state.tool === "brush_field"} />
        <ActionButton commands={commands} action="studio.tool_spawn_organism" label="孵化" pressed={state.tool === "spawn_organism"} />
        <ActionButton commands={commands} action="studio.undo_patch" label="撤销" />
        <ActionButton commands={commands} action="studio.redo_patch" label="重做" />
      </Toolbar>
      <DockWorkspace
        layout={layout}
        onLayoutChange={onLayoutChange}
        panelTitle={(id) => STUDIO_PANEL_TITLES[id as StudioPanelId] ?? id}
        renderPanel={(id) => (
          <div data-rv-context={PANEL_CONTEXTS[id as StudioPanelId]} data-studio-panel={id} style={{ height: "100%" }}>
            {renderPanel(id)}
          </div>
        )}
      />
      <StatusBar>{status}</StatusBar>
    </div>
  );
}
