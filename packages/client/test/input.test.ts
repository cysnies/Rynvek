// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { InputActionRouter, type ActionInvocation } from "../src/index.js";

const key = (k: string, mods: Partial<{ ctrl: boolean; shift: boolean; alt: boolean; meta: boolean }> = {}, isTextInput = false) => ({
  key: k,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
  metaKey: mods.meta ?? false,
  isTextInput,
});

function recorder(router: InputActionRouter) {
  const got: ActionInvocation[] = [];
  for (const id of [
    "global.command_palette",
    "global.cancel_or_menu",
    "camera.pan_left",
    "camera.zoom_in",
    "rts.select_primary",
    "rts.box_select",
    "rts.select_add_toggle",
    "rts.select_same_archetype",
    "rts.context_order",
    "rts.order_attack_move",
    "rts.order_patrol",
    "rts.order_hold_position",
    "rts.release_human_lease",
    "rts.assign_control_group_1_9",
    "rts.recall_control_group_1_9",
    "studio.sim_play_pause",
  ] as const)
    router.on(id, (inv) => got.push(inv));
  return got;
}

describe("InputActionRouter（规范 12.1）", () => {
  it("键盘映射到逻辑动作并遵守作用域", () => {
    const r = new InputActionRouter(["Canvas", "RTS_Gameplay"]);
    const got = recorder(r);
    expect(r.handleKey(key("k", { ctrl: true }))).toBe(true);
    expect(r.handleKey(key("a"))).toBe(true);
    expect(r.handleKey(key("a", { alt: true }))).toBe(true);
    expect(r.handleKey(key("h"))).toBe(true);
    expect(r.handleKey(key("3", { ctrl: true }))).toBe(true);
    expect(r.handleKey(key("3"))).toBe(true);
    expect(r.handleKey(key(" "))).toBe(false); // Studio_IDE 未激活
    expect(got.map((g) => [g.id, g.group])).toEqual([
      ["global.command_palette", undefined],
      ["camera.pan_left", undefined],
      ["rts.release_human_lease", undefined],
      ["rts.order_hold_position", undefined],
      ["rts.assign_control_group_1_9", 3],
      ["rts.recall_control_group_1_9", 3],
    ]);
    r.pushContext("Studio_IDE");
    expect(r.handleKey(key(" "))).toBe(true);
    r.popContext("Studio_IDE");
    expect(r.contexts).toEqual(["Global", "Canvas", "RTS_Gameplay"]);
  });

  it("文本输入框内不触发单字符快捷键，但组合键照常", () => {
    const r = new InputActionRouter(["Canvas", "RTS_Gameplay"]);
    const got = recorder(r);
    expect(r.handleKey(key("a", {}, true))).toBe(false);
    expect(r.handleKey(key("k", { ctrl: true }, true))).toBe(true);
    expect(got).toHaveLength(1);
  });

  it("指针手势与 Q/P + 左键组合", () => {
    const r = new InputActionRouter(["Canvas", "RTS_Gameplay"]);
    const got = recorder(r);
    r.handlePointer({ kind: "click", button: "left", x: 1, y: 2 });
    r.handlePointer({ kind: "click", button: "left", x: 1, y: 2, shift: true });
    r.handlePointer({ kind: "click", button: "left", x: 1, y: 2, double: true });
    r.handlePointer({ kind: "click", button: "left", x: 1, y: 2, ctrl: true });
    r.handlePointer({ kind: "drag", button: "left", x: 1, y: 2, x2: 9, y2: 8 });
    r.handlePointer({ kind: "click", button: "right", x: 5, y: 5 });
    r.handlePointer({ kind: "wheel", direction: "up", x: 0, y: 0 });
    expect(r.handleKey(key("q"))).toBe(true);
    expect(r.armed).toBe("q");
    r.handlePointer({ kind: "click", button: "left", x: 7, y: 7 });
    r.handleKey(key("p"));
    r.handleKey(key("Escape"));
    expect(r.armed).toBeNull();
    r.handleKey(key("p"));
    r.handlePointer({ kind: "click", button: "left", x: 3, y: 4 });
    expect(got.map((g) => g.id)).toEqual([
      "rts.select_primary",
      "rts.select_add_toggle",
      "rts.select_same_archetype",
      "rts.select_same_archetype",
      "rts.box_select",
      "rts.context_order",
      "camera.zoom_in",
      "rts.order_attack_move",
      "global.cancel_or_menu",
      "rts.order_patrol",
    ]);
    expect(got[4]).toMatchObject({ point: [1, 2], point2: [9, 8] });
    expect(got[7]).toMatchObject({ point: [7, 7] });
  });
});
