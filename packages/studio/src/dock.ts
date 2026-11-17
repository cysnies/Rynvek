// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 可停靠工作区（Dockable Workspace，D593–D616）的纯函数布局模型。
//
// 布局是一棵树：分栏节点（split，水平 row / 垂直 column，子节点按比例分配）与标签组（tabs）。
// 所有操作返回新树，并经过 `normalizeLayout` 规范化：删除空标签组、折叠单子分栏、
// 合并同向嵌套分栏、比例重新归一。布局可序列化为 JSON 持久化。

/** Studio 内置面板（项目文档 3.1） */
export const STUDIO_PANELS = [
  "world_editor",
  "neural_inspector",
  "rule_editor",
  "timeline",
  "rview_preview",
] as const;
export type StudioPanelId = (typeof STUDIO_PANELS)[number];

export const STUDIO_PANEL_TITLES: Readonly<Record<StudioPanelId, string>> = {
  world_editor: "世界地形与场编辑器",
  neural_inspector: "神经拓扑与脉冲检视器",
  rule_editor: "RuleGraph 规则编辑器",
  timeline: "时间线单步调试器",
  rview_preview: ".rview 视觉包预览",
};

export type SplitDirection = "row" | "column";

export interface DockTabs {
  readonly type: "tabs";
  readonly id: string;
  readonly panels: readonly string[];
  readonly active: string;
}

export interface DockSplit {
  readonly type: "split";
  readonly id: string;
  readonly direction: SplitDirection;
  readonly children: readonly DockNode[];
  /** 与 children 一一对应的比例，和为 1 */
  readonly sizes: readonly number[];
}

export type DockNode = DockTabs | DockSplit;

export interface DockLayout {
  readonly version: 1;
  readonly root: DockNode | null;
  /** 节点 ID 计数器，保证新节点 ID 确定且不重复 */
  readonly nextId: number;
}

/** 停靠位置：中心并入标签组，四边则拆分 */
export type DockPosition = "center" | "left" | "right" | "top" | "bottom";

/** 单个分栏子节点的最小比例 */
export const MIN_PANE_FRACTION = 0.1;

export class DockLayoutError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "DockLayoutError";
  }
}

export function emptyLayout(): DockLayout {
  return { version: 1, root: null, nextId: 1 };
}

/** Studio 缺省布局：左侧世界编辑器（上）+ 时间线（下），右侧检视器 / 规则 / 视觉包标签组 */
export function defaultStudioLayout(): DockLayout {
  return {
    version: 1,
    nextId: 6,
    root: {
      type: "split",
      id: "n1",
      direction: "row",
      sizes: [0.62, 0.38],
      children: [
        {
          type: "split",
          id: "n2",
          direction: "column",
          sizes: [0.72, 0.28],
          children: [
            { type: "tabs", id: "n3", panels: ["world_editor"], active: "world_editor" },
            { type: "tabs", id: "n4", panels: ["timeline"], active: "timeline" },
          ],
        },
        {
          type: "tabs",
          id: "n5",
          panels: ["neural_inspector", "rule_editor", "rview_preview"],
          active: "neural_inspector",
        },
      ],
    },
  };
}

// ---- 查询 ----

export function allPanels(layout: DockLayout): string[] {
  const out: string[] = [];
  walk(layout.root, (n) => {
    if (n.type === "tabs") out.push(...n.panels);
  });
  return out;
}

export function findTabsOf(layout: DockLayout, panel: string): DockTabs | null {
  let hit: DockTabs | null = null;
  walk(layout.root, (n) => {
    if (n.type === "tabs" && n.panels.includes(panel)) hit = n;
  });
  return hit;
}

export function findNode(layout: DockLayout, id: string): DockNode | null {
  let hit: DockNode | null = null;
  walk(layout.root, (n) => {
    if (n.id === id) hit = n;
  });
  return hit;
}

function walk(node: DockNode | null, visit: (n: DockNode) => void): void {
  if (node === null) return;
  visit(node);
  if (node.type === "split") for (const c of node.children) walk(c, visit);
}

// ---- 变换 ----

function mapNode(node: DockNode | null, fn: (n: DockNode) => DockNode | null): DockNode | null {
  if (node === null) return null;
  if (node.type === "split") {
    const kids = node.children.map((c) => mapNode(c, fn));
    const children: DockNode[] = [];
    const sizes: number[] = [];
    kids.forEach((k, i) => {
      if (k !== null) {
        children.push(k);
        sizes.push(node.sizes[i] ?? 0);
      }
    });
    return fn({ ...node, children, sizes });
  }
  return fn(node);
}

/** 从布局中移除面板（其余结构不变，空标签组随后被规范化删除） */
function removePanel(root: DockNode | null, panel: string): DockNode | null {
  return mapNode(root, (n) => {
    if (n.type !== "tabs" || !n.panels.includes(panel)) return n;
    const panels = n.panels.filter((p) => p !== panel);
    if (panels.length === 0) return null;
    const idx = n.panels.indexOf(panel);
    const active = n.active === panel ? (panels[Math.min(idx, panels.length - 1)] as string) : n.active;
    return { ...n, panels, active };
  });
}

/**
 * 把面板停靠到目标标签组（不在布局中则新增，已在布局中则移动）。
 * `target` 为 null 且布局为空时创建根标签组。
 */
export function dockPanel(
  layout: DockLayout,
  panel: string,
  target: string | null,
  position: DockPosition = "center",
  index?: number,
): DockLayout {
  if (panel.length === 0) throw new DockLayoutError("面板 ID 不能为空");
  let nextId = layout.nextId;
  const fresh = (): string => `n${nextId++}`;

  if (target === null) {
    if (layout.root !== null && findTabsOf(layout, panel) === null) {
      // 无目标：停靠到第一个标签组
      let first: DockTabs | null = null;
      walk(layout.root, (n) => {
        if (first === null && n.type === "tabs") first = n;
      });
      return dockPanel(layout, panel, (first as DockTabs | null)?.id ?? null, position, index);
    }
    if (layout.root === null) {
      const id = fresh();
      return { version: 1, nextId, root: { type: "tabs", id, panels: [panel], active: panel } };
    }
    return layout;
  }

  const targetNode = findNode(layout, target);
  if (targetNode === null || targetNode.type !== "tabs") throw new DockLayoutError(`目标标签组不存在：${target}`);
  // 自身所在组只有它一个面板时，向自身四边停靠没有意义
  if (position !== "center" && targetNode.panels.length === 1 && targetNode.panels[0] === panel) return layout;

  const root = removePanel(layout.root, panel);
  let placed = false;
  const newRoot = mapNode(root, (n) => {
    if (n.id !== target || n.type !== "tabs") return n;
    placed = true;
    if (position === "center") {
      const panels = [...n.panels];
      const at = index === undefined ? panels.length : Math.max(0, Math.min(index, panels.length));
      panels.splice(at, 0, panel);
      return { ...n, panels, active: panel };
    }
    const tabs: DockTabs = { type: "tabs", id: fresh(), panels: [panel], active: panel };
    const direction: SplitDirection = position === "left" || position === "right" ? "row" : "column";
    const first = position === "left" || position === "top";
    return {
      type: "split",
      id: fresh(),
      direction,
      children: first ? [tabs, n] : [n, tabs],
      sizes: [0.5, 0.5],
    };
  });
  // 目标组恰好因移除面板而消失（把唯一面板停靠到自身中心）
  if (!placed) return layout;
  return normalizeLayout({ version: 1, root: newRoot, nextId });
}

export function closePanel(layout: DockLayout, panel: string): DockLayout {
  if (findTabsOf(layout, panel) === null) return layout;
  return normalizeLayout({ ...layout, root: removePanel(layout.root, panel) });
}

export function activatePanel(layout: DockLayout, panel: string): DockLayout {
  if (findTabsOf(layout, panel) === null) throw new DockLayoutError(`面板不在布局中：${panel}`);
  return { ...layout, root: mapNode(layout.root, (n) => (n.type === "tabs" && n.panels.includes(panel) ? { ...n, active: panel } : n)) };
}

/**
 * 拖动分栏中第 `handle` 与 `handle + 1` 个子节点之间的分隔条，`delta` 为比例增量。
 * 两侧都不小于 `MIN_PANE_FRACTION`。
 */
export function resizeSplit(layout: DockLayout, splitId: string, handle: number, delta: number): DockLayout {
  const node = findNode(layout, splitId);
  if (node === null || node.type !== "split") throw new DockLayoutError(`分栏不存在：${splitId}`);
  if (handle < 0 || handle >= node.children.length - 1) throw new DockLayoutError(`分隔条下标越界：${handle}`);
  const a = node.sizes[handle] as number;
  const b = node.sizes[handle + 1] as number;
  const d = Math.max(MIN_PANE_FRACTION - a, Math.min(b - MIN_PANE_FRACTION, delta));
  const sizes = [...node.sizes];
  sizes[handle] = round6(a + d);
  sizes[handle + 1] = round6(a + b - (a + d));
  return { ...layout, root: mapNode(layout.root, (n) => (n.id === splitId ? { ...n, sizes } as DockSplit : n)) };
}

const round6 = (x: number): number => Math.round(x * 1e6) / 1e6;

/** 规范化：删空组、折叠单子分栏、合并同向嵌套分栏、比例归一、修正失效的活动标签 */
export function normalizeLayout(layout: DockLayout): DockLayout {
  const norm = (node: DockNode | null): DockNode | null => {
    if (node === null) return null;
    if (node.type === "tabs") {
      const panels = [...new Set(node.panels)];
      if (panels.length === 0) return null;
      const active = panels.includes(node.active) ? node.active : (panels[0] as string);
      return { ...node, panels, active };
    }
    const children: DockNode[] = [];
    const sizes: number[] = [];
    node.children.forEach((c, i) => {
      const n = norm(c);
      const s = node.sizes[i] ?? 0;
      if (n === null) return;
      if (n.type === "split" && n.direction === node.direction) {
        // 同向嵌套：把孙节点按比例提升一层
        n.children.forEach((g, j) => {
          children.push(g);
          sizes.push(s * (n.sizes[j] ?? 0));
        });
      } else {
        children.push(n);
        sizes.push(s);
      }
    });
    if (children.length === 0) return null;
    if (children.length === 1) return children[0] as DockNode;
    const total = sizes.reduce((x, y) => x + y, 0);
    const normalized = total > 0 ? sizes.map((s) => s / total) : sizes.map(() => 1 / sizes.length);
    return { ...node, children, sizes: normalized.map(round6) };
  };
  return { version: 1, root: norm(layout.root), nextId: layout.nextId };
}

// ---- 序列化 ----

export function serializeLayout(layout: DockLayout): string {
  return JSON.stringify(layout);
}

/** 解析持久化布局；结构非法、ID 或面板重复时抛错 */
export function parseLayout(json: string): DockLayout {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    throw new DockLayoutError("布局不是合法 JSON");
  }
  const obj = raw as Partial<DockLayout> | null;
  if (obj === null || typeof obj !== "object" || obj.version !== 1) throw new DockLayoutError("布局版本必须为 1");
  if (typeof obj.nextId !== "number" || !Number.isInteger(obj.nextId) || obj.nextId < 1) throw new DockLayoutError("nextId 非法");
  const ids = new Set<string>();
  const panels = new Set<string>();
  const check = (n: unknown, depth: number): DockNode => {
    if (depth > 32) throw new DockLayoutError("布局嵌套过深");
    const node = n as Record<string, unknown>;
    if (node === null || typeof node !== "object" || typeof node["id"] !== "string") throw new DockLayoutError("节点缺少 id");
    const id = node["id"];
    if (ids.has(id)) throw new DockLayoutError(`节点 ID 重复：${id}`);
    ids.add(id);
    if (node["type"] === "tabs") {
      const ps = node["panels"];
      if (!Array.isArray(ps) || ps.length === 0 || !ps.every((p) => typeof p === "string")) throw new DockLayoutError(`${id}.panels 非法`);
      for (const p of ps as string[]) {
        if (panels.has(p)) throw new DockLayoutError(`面板重复：${p}`);
        panels.add(p);
      }
      if (typeof node["active"] !== "string" || !(ps as string[]).includes(node["active"])) throw new DockLayoutError(`${id}.active 非法`);
      return { type: "tabs", id, panels: [...(ps as string[])], active: node["active"] };
    }
    if (node["type"] === "split") {
      const dir = node["direction"];
      const kids = node["children"];
      const sizes = node["sizes"];
      if (dir !== "row" && dir !== "column") throw new DockLayoutError(`${id}.direction 非法`);
      if (!Array.isArray(kids) || kids.length < 2) throw new DockLayoutError(`${id}.children 至少两个`);
      if (!Array.isArray(sizes) || sizes.length !== kids.length || !sizes.every((s) => typeof s === "number" && s > 0)) {
        throw new DockLayoutError(`${id}.sizes 非法`);
      }
      return { type: "split", id, direction: dir, children: kids.map((k) => check(k, depth + 1)), sizes: [...(sizes as number[])] };
    }
    throw new DockLayoutError(`${id}.type 非法`);
  };
  const root = obj.root === null || obj.root === undefined ? null : check(obj.root, 0);
  return normalizeLayout({ version: 1, root, nextId: obj.nextId });
}
