// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  ACTION_BINDINGS,
  LOGICAL_ACTION_IDS,
  actionBinding,
  findBindingConflicts,
  formatChord,
  matchKeyboard,
} from "../src/index.js";

/** 直接解析开发规范 12.1 的 Markdown 表格作为真值 */
function specRows(): string[][] {
  const spec = readFileSync(new URL("../../../docs/Rynvek_Development_Specification_v1.0.md", import.meta.url), "utf8");
  const section = spec.slice(spec.indexOf("### 12.1"), spec.indexOf("### 12.2"));
  return section
    .split("\n")
    .filter((l) => /^\| `\d\d` \|/.test(l))
    .map((l) =>
      l
        .split("|")
        .slice(1, -1)
        .map((c) => c.trim().replaceAll("`", "")),
    );
}

const key = (k: string, mods: Partial<{ ctrl: boolean; meta: boolean; shift: boolean; alt: boolean }> = {}) => ({
  key: k,
  ctrlKey: mods.ctrl ?? false,
  metaKey: mods.meta ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
});

describe("36 项动作全表与规范逐字一致", () => {
  it("编号、ID、作用域与三列默认绑定逐项相同", () => {
    const rows = specRows();
    expect(rows).toHaveLength(36);
    expect(ACTION_BINDINGS).toHaveLength(36);
    rows.forEach(([no, id, ctx, desktop, touch, gamepad], i) => {
      const b = ACTION_BINDINGS[i]!;
      expect([b.no, b.id, b.context, b.desktop, b.touch, b.gamepad ?? "—"]).toEqual([no, id, ctx, desktop, touch, gamepad]);
    });
    expect([...LOGICAL_ACTION_IDS]).toEqual(rows.map((r) => r[1]));
  });

  it("每个动作都至少有一种可匹配的键盘或指针绑定", () => {
    for (const b of ACTION_BINDINGS) expect(b.keyboard.length + b.pointer.length).toBeGreaterThan(0);
  });

  it("可同时激活的作用域之间没有组合键冲突", () => {
    expect(findBindingConflicts()).toEqual([]);
  });
});

describe("键盘匹配", () => {
  const studio = ["Global", "Canvas", "Studio_IDE"] as const;

  it("修饰键必须完全一致：Ctrl+Z 撤销、Ctrl+Shift+Z 重做、Cmd+Z 也撤销", () => {
    expect(matchKeyboard(key("z", { ctrl: true }), studio)?.binding.id).toBe("studio.undo_patch");
    expect(matchKeyboard(key("Z", { ctrl: true, shift: true }), studio)?.binding.id).toBe("studio.redo_patch");
    expect(matchKeyboard(key("z", { meta: true }), studio)?.binding.id).toBe("studio.undo_patch");
    expect(matchKeyboard(key("z"), studio)).toBeNull();
  });

  it("Space / Period 驱动 Studio 仿真控制，M 与 Ctrl+M 区分", () => {
    expect(matchKeyboard(key(" "), studio)?.binding.id).toBe("studio.sim_play_pause");
    expect(matchKeyboard(key("."), studio)?.binding.id).toBe("studio.sim_step_single_tick");
    expect(matchKeyboard(key("m"), studio)?.binding.id).toBe("global.toggle_minimap");
    expect(matchKeyboard(key("m", { ctrl: true }), studio)?.binding.id).toBe("global.cycle_audio_mute");
  });

  it("只在活动作用域内匹配", () => {
    expect(matchKeyboard(key(" "), ["Global"])).toBeNull();
    expect(matchKeyboard(key("s", { ctrl: true }), studio)).toBeNull();
    expect(matchKeyboard(key("s", { ctrl: true }), [...studio, "Code_Editor"])?.binding.id).toBe("studio.compile_and_hot_reload");
    expect(matchKeyboard(key("ArrowUp"), studio)?.binding.id).toBe("camera.pan_up");
  });

  it("编队动作携带数字编号", () => {
    const rts = ["Global", "Canvas", "RTS_Gameplay"] as const;
    expect(matchKeyboard(key("7", { ctrl: true }), rts)).toMatchObject({ binding: { id: "rts.assign_control_group_1_9" }, group: 7 });
    expect(matchKeyboard(key("3"), rts)).toMatchObject({ binding: { id: "rts.recall_control_group_1_9" }, group: 3 });
    expect(matchKeyboard(key("0"), rts)).toBeNull();
  });

  it("组合键展示文本", () => {
    expect(formatChord(actionBinding("studio.redo_patch")!.keyboard[0]!)).toBe("Ctrl+Shift+Z");
    expect(formatChord(actionBinding("studio.sim_play_pause")!.keyboard[0]!)).toBe("Space");
    expect(actionBinding("rts.order_attack_move")!.pointer).toEqual([{ kind: "key_then_click", key: "q" }]);
  });
});
