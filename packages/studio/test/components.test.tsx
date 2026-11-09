// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// @vitest-environment jsdom
import { TrustLevel, loadRview, writeRview } from "@rynvek/assets";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AuthoringDocument,
  DockWorkspace,
  HybridLogicalClock,
  RviewPreview,
  STUDIO_PANEL_TITLES,
  StudioCommands,
  StudioWorkspace,
  UndoManager,
  allPanels,
  defaultStudioLayout,
  findTabsOf,
  type DockLayout,
  type DockSplit,
} from "../src/index.js";
import { baseline, idGen } from "./helpers.js";

afterEach(cleanup);

function Harness({ initial, onChange }: { initial: DockLayout; onChange?: (l: DockLayout) => void }) {
  const [layout, setLayout] = useState(initial);
  return (
    <DockWorkspace
      layout={layout}
      onLayoutChange={(l) => {
        setLayout(l);
        onChange?.(l);
      }}
      panelTitle={(id) => STUDIO_PANEL_TITLES[id as keyof typeof STUDIO_PANEL_TITLES] ?? id}
      renderPanel={(id) => <p>内容:{id}</p>}
    />
  );
}

describe("DockWorkspace", () => {
  it("渲染标签组、切换与关闭标签", () => {
    let latest = defaultStudioLayout();
    render(<Harness initial={latest} onChange={(l) => (latest = l)} />);
    expect(screen.getByText("内容:world_editor")).toBeTruthy();
    expect(screen.getByText("内容:neural_inspector")).toBeTruthy();
    const ruleTab = screen.getByRole("tab", { name: "TypeScript 规则编辑器" });
    expect(ruleTab.getAttribute("aria-selected")).toBe("false");
    fireEvent.click(ruleTab);
    expect(screen.getByText("内容:rule_editor")).toBeTruthy();
    expect(screen.getByRole("tab", { name: "TypeScript 规则编辑器" }).getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "关闭 时间线单步调试器" }));
    expect(allPanels(latest)).not.toContain("timeline");
    expect(screen.queryByText("内容:timeline")).toBeNull();
  });

  it("标签键盘导航（方向键 / Delete）", () => {
    let latest = defaultStudioLayout();
    render(<Harness initial={latest} onChange={(l) => (latest = l)} />);
    const tab = screen.getByRole("tab", { name: "神经拓扑与脉冲检视器" });
    fireEvent.keyDown(tab, { key: "ArrowRight" });
    expect(findTabsOf(latest, "rule_editor")!.active).toBe("rule_editor");
    expect(document.activeElement?.textContent).toBe("TypeScript 规则编辑器");
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(findTabsOf(latest, "rview_preview")!.active).toBe("rview_preview");
    fireEvent.keyDown(document.activeElement!, { key: "Delete" });
    expect(allPanels(latest)).not.toContain("rview_preview");
  });

  it("分隔条键盘调整（WAI-ARIA Window Splitter）", () => {
    let latest = defaultStudioLayout();
    render(<Harness initial={latest} onChange={(l) => (latest = l)} />);
    // 外层左右分栏的分隔条是纵向的；内层上下分栏的是横向的
    const outerOf = () => screen.getAllByRole("separator").find((s) => s.getAttribute("aria-orientation") === "vertical")!;
    const outer = outerOf();
    expect(outer.getAttribute("aria-valuenow")).toBe("62");
    const inner = screen.getAllByRole("separator").find((s) => s.getAttribute("aria-orientation") === "horizontal")!;
    expect(inner.getAttribute("aria-valuenow")).toBe("72");
    fireEvent.keyDown(outer!, { key: "ArrowRight" });
    expect((latest.root as DockSplit).sizes[0]).toBeCloseTo(0.64, 6);
    fireEvent.keyDown(outer!, { key: "Home" });
    expect((latest.root as DockSplit).sizes[0]).toBeCloseTo(0.1, 6);
    expect(outerOf().getAttribute("aria-valuenow")).toBe("10");
  });

  it("拖放标签到另一组的右侧停靠", () => {
    let latest = defaultStudioLayout();
    render(<Harness initial={latest} onChange={(l) => (latest = l)} />);
    expect(screen.queryByTestId("drop-n3-right")).toBeNull();
    fireEvent.dragStart(screen.getByRole("tab", { name: "时间线单步调试器" }));
    fireEvent.drop(screen.getByTestId("drop-n3-right"));
    const world = findTabsOf(latest, "world_editor")!;
    const timeline = findTabsOf(latest, "timeline")!;
    expect(world.id).not.toBe(timeline.id);
    expect(findTabsOf(latest, "timeline")!.panels).toEqual(["timeline"]);
    expect(screen.queryByTestId("drop-n3-right")).toBeNull(); // 拖放结束后隐藏落点
    expect(screen.getByText("内容:timeline")).toBeTruthy();
  });
});

describe("StudioWorkspace", () => {
  function setup(compile?: () => null) {
    const sim = { start: vi.fn(), pause: vi.fn(), step: vi.fn() };
    const doc = new AuthoringDocument(baseline());
    const undo = new UndoManager(doc, new HybridLogicalClock(1, () => 1000), idGen(1));
    const audio = { play: vi.fn() };
    const compileAndHotReload = vi.fn(compile ?? (() => null));
    const commands = new StudioCommands({ sim, undo, audio, compileAndHotReload });
    function App() {
      const [layout, setLayout] = useState(defaultStudioLayout());
      return (
        <StudioWorkspace
          commands={commands}
          layout={layout}
          onLayoutChange={setLayout}
          renderPanel={(id) => (id === "rule_editor" ? <textarea aria-label="规则源码" /> : <button type="button">{id}</button>)}
          status={<span>Tick 0</span>}
        />
      );
    }
    render(<App />);
    return { sim, doc, undo, audio, commands, compileAndHotReload };
  }

  it("工具栏按钮驱动仿真与工具状态，并显示快捷键", () => {
    const { sim, audio } = setup();
    const toolbar = screen.getByRole("toolbar", { name: "Studio 工具栏" });
    const play = within(toolbar).getByRole("button", { name: /播放/ });
    expect(play.textContent).toContain("Space");
    fireEvent.click(play);
    expect(sim.start).toHaveBeenCalledWith(30);
    expect(within(toolbar).getByRole("button", { name: /暂停/ }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(within(toolbar).getByRole("button", { name: /单步/ }));
    expect(sim.pause).toHaveBeenCalledTimes(1);
    expect(sim.step).toHaveBeenCalledWith(1);
    fireEvent.click(within(toolbar).getByRole("button", { name: /场画笔/ }));
    expect(within(toolbar).getByRole("button", { name: /场画笔/ }).getAttribute("aria-pressed")).toBe("true");
    expect(audio.play).toHaveBeenLastCalledWith("ui.confirm");
    fireEvent.click(within(toolbar).getByRole("button", { name: /撤销/ }));
    expect(audio.play).toHaveBeenLastCalledWith("ui.error"); // 无可撤销内容
    expect(screen.getByRole("status").textContent).toBe("Tick 0");
  });

  it("键盘快捷键按焦点上下文分派；编辑器内打字不触发单键快捷键", () => {
    const { sim, undo, compileAndHotReload, doc } = setup();
    const worldButton = screen.getByRole("button", { name: "world_editor" });
    fireEvent.keyDown(worldButton, { key: " " });
    expect(sim.start).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(worldButton, { key: "." });
    expect(sim.step).toHaveBeenCalledWith(1);

    act(() => {
      undo.commit([{ SetWorldParam: { key_path: "field.a", new_value_json: "1" } }]);
    });
    fireEvent.keyDown(worldButton, { key: "z", ctrlKey: true });
    expect(doc.param("field.a")).toBeUndefined();
    fireEvent.keyDown(worldButton, { key: "Z", ctrlKey: true, shiftKey: true });
    expect(doc.param("field.a")).toBe("1");

    // Ctrl+S 只在 Code_Editor 上下文生效
    fireEvent.keyDown(worldButton, { key: "s", ctrlKey: true });
    expect(compileAndHotReload).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("tab", { name: "TypeScript 规则编辑器" }));
    const editor = screen.getByRole("textbox", { name: "规则源码" });
    fireEvent.keyDown(editor, { key: "g" });
    expect(screen.getByRole("button", { name: /场画笔/ }).getAttribute("aria-pressed")).toBe("false");
    fireEvent.keyDown(editor, { key: "s", ctrlKey: true });
    expect(compileAndHotReload).toHaveBeenCalledTimes(1);
  });
});

describe("RviewPreview", () => {
  it("展示清单、色板、着色器预算与被剔除的令牌", () => {
    const enc = new TextEncoder();
    const pkg = loadRview(
      writeRview({
        id: "rynvek.visual.industrial-default",
        version: "1.0.0",
        name: "Industrial",
        license: "CC0-1.0",
        engine_compat: ">=0.1.0 <0.2.0",
        entries: [
          { path: "palette.json", kind: "palette", media_type: "application/json", data: enc.encode('{"colors":{"faction-1":"#112233"}}') },
          { path: "theme.json", kind: "theme", media_type: "application/json", data: enc.encode('{"tokens":{"--rv-color-bg":"#000000"}}') },
          {
            path: "glow.wgsl",
            kind: "shader",
            media_type: "text/wgsl",
            data: enc.encode("@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }"),
          },
        ],
      }),
      { trustLevel: TrustLevel.UntrustedEphemeral },
    );
    render(<RviewPreview pkg={pkg} />);
    expect(screen.getByText("rynvek.visual.industrial-default@1.0.0")).toBeTruthy();
    expect(screen.getByText("#112233")).toBeTruthy();
    expect(screen.getByRole("region", { name: "着色器" }).textContent).toContain("fs：2 / 512 条指令");
    expect(screen.getByRole("list", { name: "被剔除的令牌" }).textContent).toContain("非官方包不能改写应用层");
    expect(screen.getByText("faction：1 个令牌")).toBeTruthy();
  });
});
