// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 可停靠工作区 React 视图：递归渲染分栏与标签组，支持标签拖放停靠、关闭、
// 分隔条指针拖动与键盘调整（WAI-ARIA Window Splitter 模式）。
// 拖放状态保存在组件内而非 DataTransfer，便于无头测试，也避免跨窗口拖入伪造面板。

import { type KeyboardEvent, type PointerEvent, type ReactNode, useCallback, useRef, useState } from "react";
import {
  activatePanel,
  closePanel,
  dockPanel,
  resizeSplit,
  type DockLayout,
  type DockNode,
  type DockPosition,
  type DockSplit,
  type DockTabs,
} from "../dock.js";
import { DOCK_STYLESHEET } from "./styles.js";

/** 键盘调整分隔条的步长（比例） */
export const SEPARATOR_KEY_STEP = 0.02;

const POSITIONS: readonly DockPosition[] = ["center", "left", "right", "top", "bottom"];
const POSITION_LABELS: Readonly<Record<DockPosition, string>> = {
  center: "并入标签组",
  left: "停靠到左侧",
  right: "停靠到右侧",
  top: "停靠到上方",
  bottom: "停靠到下方",
};

export interface DockWorkspaceProps {
  readonly layout: DockLayout;
  readonly onLayoutChange: (layout: DockLayout) => void;
  readonly renderPanel: (panelId: string) => ReactNode;
  readonly panelTitle?: (panelId: string) => string;
  /** 面板是否允许关闭（缺省允许） */
  readonly closable?: (panelId: string) => boolean;
}

export function DockWorkspace({ layout, onLayoutChange, renderPanel, panelTitle, closable }: DockWorkspaceProps) {
  const [dragging, setDragging] = useState<string | null>(null);
  const title = panelTitle ?? ((id: string) => id);

  const drop = useCallback(
    (target: string, position: DockPosition) => {
      if (dragging === null) return;
      onLayoutChange(dockPanel(layout, dragging, target, position));
      setDragging(null);
    },
    [dragging, layout, onLayoutChange],
  );

  const ctx: RenderCtx = {
    layout,
    onLayoutChange,
    renderPanel,
    title,
    closable: closable ?? (() => true),
    dragging,
    setDragging,
    drop,
  };

  return (
    <div className="rv-dock" data-testid="rv-dock">
      <style data-rv-dock-stylesheet="">{DOCK_STYLESHEET}</style>
      {layout.root === null ? <div className="rv-dock-empty">工作区为空</div> : <DockNodeView node={layout.root} ctx={ctx} />}
    </div>
  );
}

interface RenderCtx {
  readonly layout: DockLayout;
  readonly onLayoutChange: (layout: DockLayout) => void;
  readonly renderPanel: (panelId: string) => ReactNode;
  readonly title: (panelId: string) => string;
  readonly closable: (panelId: string) => boolean;
  readonly dragging: string | null;
  readonly setDragging: (panel: string | null) => void;
  readonly drop: (target: string, position: DockPosition) => void;
}

function DockNodeView({ node, ctx }: { node: DockNode; ctx: RenderCtx }) {
  return node.type === "split" ? <SplitView node={node} ctx={ctx} /> : <TabsView node={node} ctx={ctx} />;
}

function SplitView({ node, ctx }: { node: DockSplit; ctx: RenderCtx }) {
  const ref = useRef<HTMLDivElement>(null);
  const drag = useRef<{ handle: number; origin: number } | null>(null);
  const horizontal = node.direction === "row";

  const onKeyDown = (handle: number) => (e: KeyboardEvent<HTMLDivElement>) => {
    const dec = horizontal ? "ArrowLeft" : "ArrowUp";
    const inc = horizontal ? "ArrowRight" : "ArrowDown";
    let delta = 0;
    if (e.key === dec) delta = -SEPARATOR_KEY_STEP;
    else if (e.key === inc) delta = SEPARATOR_KEY_STEP;
    else if (e.key === "Home") delta = -1;
    else if (e.key === "End") delta = 1;
    else return;
    e.preventDefault();
    ctx.onLayoutChange(resizeSplit(ctx.layout, node.id, handle, delta));
  };

  const onPointerDown = (handle: number) => (e: PointerEvent<HTMLDivElement>) => {
    drag.current = { handle, origin: horizontal ? e.clientX : e.clientY };
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    const rect = ref.current?.getBoundingClientRect();
    const extent = rect ? (horizontal ? rect.width : rect.height) : 0;
    if (d === null || extent <= 0) return;
    const pos = horizontal ? e.clientX : e.clientY;
    ctx.onLayoutChange(resizeSplit(ctx.layout, node.id, d.handle, (pos - d.origin) / extent));
    d.origin = pos;
  };
  const onPointerUp = () => {
    drag.current = null;
  };

  const items: ReactNode[] = [];
  node.children.forEach((child, i) => {
    if (i > 0) {
      const before = node.sizes[i - 1] ?? 0;
      items.push(
        <div
          key={`sep-${child.id}`}
          className="rv-dock-separator"
          role="separator"
          tabIndex={0}
          aria-orientation={horizontal ? "vertical" : "horizontal"}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(before * 100)}
          aria-label="调整面板大小"
          onKeyDown={onKeyDown(i - 1)}
          onPointerDown={onPointerDown(i - 1)}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        />,
      );
    }
    items.push(
      <div key={child.id} className="rv-dock-pane" style={{ flexGrow: node.sizes[i] ?? 0 }}>
        <DockNodeView node={child} ctx={ctx} />
      </div>,
    );
  });

  return (
    <div ref={ref} className="rv-dock-split" data-direction={node.direction} data-node-id={node.id}>
      {items}
    </div>
  );
}

function TabsView({ node, ctx }: { node: DockTabs; ctx: RenderCtx }) {
  const tabRefs = useRef(new Map<string, HTMLButtonElement>());
  const select = (panel: string) => ctx.onLayoutChange(activatePanel(ctx.layout, panel));

  // 标签列表的左右方向键切换（roving tabindex）
  const onTabKey = (panel: string) => (e: KeyboardEvent<HTMLButtonElement>) => {
    const idx = node.panels.indexOf(panel);
    let next = -1;
    if (e.key === "ArrowRight") next = (idx + 1) % node.panels.length;
    else if (e.key === "ArrowLeft") next = (idx - 1 + node.panels.length) % node.panels.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = node.panels.length - 1;
    else if (e.key === "Delete" && ctx.closable(panel)) {
      e.preventDefault();
      ctx.onLayoutChange(closePanel(ctx.layout, panel));
      return;
    } else return;
    e.preventDefault();
    const target = node.panels[next] as string;
    select(target);
    tabRefs.current.get(target)?.focus();
  };

  const active = node.active;
  const panelDomId = `rv-dock-panel-${node.id}`;
  return (
    <div className="rv-dock-tabs" data-node-id={node.id}>
      <div role="tablist" className="rv-tablist" aria-label="面板标签">
        {node.panels.map((panel) => {
          const selected = panel === active;
          return (
            <span key={panel} className="rv-dock-tab">
              <button
                ref={(el) => {
                  if (el) tabRefs.current.set(panel, el);
                  else tabRefs.current.delete(panel);
                }}
                type="button"
                role="tab"
                className="rv-tab"
                id={`rv-dock-tab-${node.id}-${panel}`}
                aria-selected={selected}
                aria-controls={panelDomId}
                tabIndex={selected ? 0 : -1}
                draggable
                data-panel-id={panel}
                onClick={() => select(panel)}
                onKeyDown={onTabKey(panel)}
                onDragStart={() => ctx.setDragging(panel)}
                onDragEnd={() => ctx.setDragging(null)}
              >
                {ctx.title(panel)}
              </button>
              {ctx.closable(panel) ? (
                <button
                  type="button"
                  className="rv-dock-close"
                  aria-label={`关闭 ${ctx.title(panel)}`}
                  onClick={() => ctx.onLayoutChange(closePanel(ctx.layout, panel))}
                >
                  ×
                </button>
              ) : null}
            </span>
          );
        })}
      </div>
      <div
        role="tabpanel"
        id={panelDomId}
        className="rv-dock-body"
        aria-labelledby={`rv-dock-tab-${node.id}-${active}`}
        data-panel-id={active}
      >
        {ctx.renderPanel(active)}
      </div>
      {ctx.dragging !== null
        ? POSITIONS.map((pos) => (
            <div
              key={pos}
              className="rv-dock-drop"
              data-dock-position={pos}
              data-testid={`drop-${node.id}-${pos}`}
              aria-label={POSITION_LABELS[pos]}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                ctx.drop(node.id, pos);
              }}
            />
          ))
        : null}
    </div>
  );
}
