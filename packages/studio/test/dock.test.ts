// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import {
  DockLayoutError,
  MIN_PANE_FRACTION,
  STUDIO_PANELS,
  activatePanel,
  allPanels,
  closePanel,
  defaultStudioLayout,
  dockPanel,
  emptyLayout,
  findNode,
  findTabsOf,
  normalizeLayout,
  parseLayout,
  resizeSplit,
  serializeLayout,
  type DockSplit,
} from "../src/index.js";

describe("Dock 布局模型", () => {
  it("缺省布局包含项目文档 3.1 的五个面板", () => {
    const l = defaultStudioLayout();
    expect(allPanels(l).sort()).toEqual([...STUDIO_PANELS].sort());
    expect(normalizeLayout(l)).toEqual(l);
    expect(parseLayout(serializeLayout(l))).toEqual(l);
  });

  it("空布局停靠创建根标签组；无目标时并入第一个标签组", () => {
    let l = dockPanel(emptyLayout(), "a", null);
    expect(l.root).toEqual({ type: "tabs", id: "n1", panels: ["a"], active: "a" });
    expect(l.nextId).toBe(2);
    l = dockPanel(l, "b", null);
    expect(l.root).toMatchObject({ panels: ["a", "b"], active: "b" });
  });

  it("四边停靠拆分出新分栏，同向分栏被合并", () => {
    let l = dockPanel(emptyLayout(), "a", null);
    l = dockPanel(l, "b", "n1", "right");
    expect(l.root).toMatchObject({ type: "split", direction: "row", sizes: [0.5, 0.5] });
    const right = findTabsOf(l, "b")!;
    l = dockPanel(l, "c", right.id, "right");
    // 同向嵌套被提升为一层三栏
    const root = l.root as DockSplit;
    expect(root.children).toHaveLength(3);
    expect(root.sizes).toEqual([0.5, 0.25, 0.25]);
    l = dockPanel(l, "d", findTabsOf(l, "a")!.id, "top");
    expect(((l.root as DockSplit).children[0] as DockSplit).direction).toBe("column");
    expect(allPanels(l)).toEqual(["d", "a", "b", "c"]);
  });

  it("移动面板：从原组移除并在目标位置插入，空组与单子分栏被折叠", () => {
    let l = defaultStudioLayout();
    l = dockPanel(l, "timeline", "n5", "center", 0);
    expect(findTabsOf(l, "timeline")).toMatchObject({ id: "n5", panels: ["timeline", "neural_inspector", "rule_editor", "rview_preview"], active: "timeline" });
    expect(findNode(l, "n4")).toBeNull();
    expect(findNode(l, "n2")).toBeNull(); // 只剩世界编辑器，分栏被折叠
    expect((l.root as DockSplit).children.map((c) => c.id)).toEqual(["n3", "n5"]);
    // 组内重排
    l = dockPanel(l, "rview_preview", "n5", "center", 1);
    expect(findTabsOf(l, "rview_preview")!.panels).toEqual(["timeline", "rview_preview", "neural_inspector", "rule_editor"]);
    // 唯一面板停靠到自身：不变
    expect(dockPanel(l, "world_editor", "n3", "left")).toBe(l);
    expect(dockPanel(l, "world_editor", "n3", "center")).toBe(l);
    expect(() => dockPanel(l, "x", "n999")).toThrow(DockLayoutError);
  });

  it("关闭与激活", () => {
    let l = defaultStudioLayout();
    l = closePanel(l, "neural_inspector");
    expect(findTabsOf(l, "rule_editor")!.active).toBe("rule_editor");
    l = activatePanel(l, "rview_preview");
    expect(findTabsOf(l, "rview_preview")!.active).toBe("rview_preview");
    expect(closePanel(l, "nope")).toBe(l);
    for (const p of allPanels(l)) l = closePanel(l, p);
    expect(l.root).toBeNull();
    expect(() => activatePanel(l, "timeline")).toThrow(DockLayoutError);
  });

  it("分隔条调整受最小比例约束，比例和保持为 1", () => {
    const l = defaultStudioLayout();
    const r1 = resizeSplit(l, "n1", 0, 0.1);
    expect((r1.root as DockSplit).sizes).toEqual([0.72, 0.28]);
    const r2 = resizeSplit(l, "n1", 0, 5);
    expect((r2.root as DockSplit).sizes).toEqual([1 - MIN_PANE_FRACTION, MIN_PANE_FRACTION]);
    const r3 = resizeSplit(l, "n1", 0, -5);
    expect((r3.root as DockSplit).sizes).toEqual([MIN_PANE_FRACTION, 1 - MIN_PANE_FRACTION]);
    expect(() => resizeSplit(l, "n1", 1, 0.1)).toThrow(DockLayoutError);
    expect(() => resizeSplit(l, "n3", 0, 0.1)).toThrow(DockLayoutError);
  });

  it("解析持久化布局时拒绝非法结构", () => {
    const good = JSON.parse(serializeLayout(defaultStudioLayout()));
    const bad = (mut: (x: any) => void) => {
      const c = structuredClone(good);
      mut(c);
      return () => parseLayout(JSON.stringify(c));
    };
    expect(() => parseLayout("{")).toThrow(DockLayoutError);
    expect(bad((x) => (x.version = 2))).toThrow(/版本/);
    expect(bad((x) => (x.root.children[1].panels = ["world_editor"]))).toThrow(/重复/);
    expect(bad((x) => (x.root.children[1].id = "n1"))).toThrow(/ID 重复/);
    expect(bad((x) => (x.root.children[1].active = "ghost"))).toThrow(/active/);
    expect(bad((x) => (x.root.sizes = [1]))).toThrow(/sizes/);
    expect(bad((x) => (x.root.direction = "diagonal"))).toThrow(/direction/);
    expect(bad((x) => (x.root.children = [x.root.children[0]]))).toThrow(/至少两个/);
    // 合法但未规范化的输入被规范化
    const loose = structuredClone(good);
    loose.root.sizes = [3, 1];
    expect((parseLayout(JSON.stringify(loose)).root as DockSplit).sizes).toEqual([0.75, 0.25]);
  });
});
