// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  Button,
  CommandPalette,
  Kbd,
  Panel,
  type PaletteCommand,
  Tabs,
  ThemeProvider,
  actionBinding,
  fuzzyFilter,
  useTheme,
} from "../src/index.js";

afterEach(cleanup);

function ThemeSwitch() {
  const { theme, tokens, setTheme } = useTheme();
  return (
    <Button onClick={() => setTheme("scientific-light")} data-accent={tokens["--rv-color-accent"]}>
      {theme}
    </Button>
  );
}

describe("ThemeProvider", () => {
  it("输出 data-rv-theme、注入样式表并可切换主题", () => {
    const { container } = render(
      <ThemeProvider>
        <ThemeSwitch />
      </ThemeProvider>,
    );
    const root = container.querySelector(".rv-root")!;
    expect(root.getAttribute("data-rv-theme")).toBe("neutral-dark");
    expect(container.querySelector("style[data-rv-stylesheet]")!.textContent).toContain('[data-rv-theme="okabe-ito"]');
    const btn = screen.getByRole("button", { name: "neutral-dark" });
    expect(btn.getAttribute("data-accent")).toBe("#A3D65C");
    fireEvent.click(btn);
    expect(root.getAttribute("data-rv-theme")).toBe("scientific-light");
    expect(screen.getByRole("button").getAttribute("data-accent")).toBe("#4C7A1C");
  });

  it("覆盖层只把改动的令牌写成内联变量", () => {
    const { container } = render(
      <ThemeProvider layers={[{ kind: "faction", source: "t", tokens: { "--rv-faction-1": "#123456" } }]}>x</ThemeProvider>,
    );
    const style = (container.querySelector(".rv-root") as HTMLElement).style;
    expect(style.getPropertyValue("--rv-faction-1")).toBe("#123456");
    expect(style.getPropertyValue("--rv-color-bg")).toBe("");
  });

  it("useTheme 在 Provider 之外报错", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<ThemeSwitch />)).toThrow(/ThemeProvider/);
  });
});

describe("基础组件", () => {
  it("Button 缺省 type=button 并带变体属性", () => {
    render(<Button variant="primary">运行</Button>);
    const b = screen.getByRole("button", { name: "运行" });
    expect(b.getAttribute("type")).toBe("button");
    expect(b.getAttribute("data-variant")).toBe("primary");
  });

  it("Panel 以标题作为区域的可访问名称", () => {
    render(<Panel title="Inspector">内容</Panel>);
    expect(screen.getByRole("region", { name: "Inspector" }).textContent).toContain("内容");
  });

  it("Kbd 显示动作的首选键盘绑定", () => {
    render(<Kbd binding={actionBinding("studio.redo_patch")!} />);
    expect(screen.getByText("Ctrl+Shift+Z")).toBeTruthy();
  });

  it("Tabs：点击与方向键切换，只有选中标签可 Tab 聚焦", () => {
    const onChange = vi.fn();
    render(
      <Tabs
        label="面板"
        onValueChange={onChange}
        items={[
          { id: "a", label: "世界", content: "A 内容" },
          { id: "b", label: "规则", content: "B 内容" },
          { id: "c", label: "神经", content: "C 内容" },
        ]}
      />,
    );
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.tabIndex)).toEqual([0, -1, -1]);
    expect(screen.getByRole("tabpanel").textContent).toBe("A 内容");
    fireEvent.keyDown(tabs[0]!, { key: "ArrowLeft" });
    expect(screen.getByRole("tabpanel").textContent).toBe("C 内容");
    expect(document.activeElement).toBe(tabs[2]);
    fireEvent.keyDown(tabs[2]!, { key: "Home" });
    fireEvent.click(tabs[1]!);
    expect(screen.getByRole("tabpanel").textContent).toBe("B 内容");
    expect(onChange.mock.calls.map((c) => c[0])).toEqual(["c", "a", "b"]);
  });
});

describe("CommandPalette", () => {
  function Harness({ commands }: { commands: PaletteCommand[] }) {
    const [open, setOpen] = useState(true);
    return (
      <>
        <span data-testid="state">{open ? "open" : "closed"}</span>
        <CommandPalette open={open} commands={commands} onClose={() => setOpen(false)} />
      </>
    );
  }

  it("模糊过滤、方向键高亮、Enter 执行并关闭", () => {
    const ran: string[] = [];
    const cmds: PaletteCommand[] = ["播放 / 暂停", "单步 Tick", "撤销补丁", "重做补丁"].map((title, i) => ({
      id: `c${i}`,
      title,
      run: () => ran.push(title),
    }));
    render(<Harness commands={cmds} />);
    const input = screen.getByRole("combobox");
    expect(document.activeElement).toBe(input);
    expect(screen.getAllByRole("option")).toHaveLength(4);
    act(() => {
      fireEvent.change(input, { target: { value: "补丁" } });
    });
    const opts = screen.getAllByRole("option");
    expect(opts.map((o) => o.textContent)).toEqual(["撤销补丁", "重做补丁"]);
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[1]!.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(ran).toEqual(["重做补丁"]);
    expect(screen.getByTestId("state").textContent).toBe("closed");
  });

  it("Escape 关闭，禁用命令不出现", () => {
    render(<Harness commands={[{ id: "x", title: "隐藏", disabled: true, run: () => {} }, { id: "y", title: "可见", run: () => {} }]} />);
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["可见"]);
    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("模糊匹配：词首与连续命中优先，同分保持原顺序", () => {
    const r = fuzzyFilter("sp", ["studio.sim_play_pause", "global.toggle_field_overlay", "camera.pan_up", "spawn"], (s) => s);
    expect(r.map((x) => x.item)).toEqual(["spawn", "studio.sim_play_pause"]);
    expect(fuzzyFilter("", ["a", "b"], (s) => s).map((x) => x.item)).toEqual(["a", "b"]);
    expect(fuzzyFilter("zz", ["a"], (s) => s)).toEqual([]);
  });
});
