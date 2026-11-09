// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 36 项标准逻辑动作与多上下文默认绑定全表（规范 12.1，GAP-27）。
 *
 * 表格字段逐字照抄规范（去掉 Markdown 反引号）：`desktop` / `touch` / `gamepad` 为展示文本（快捷键提示与命令面板用），
 * `keyboard` / `pointer` 是由 `desktop` 列解析出的结构化绑定，供输入路由匹配。
 * 业务组件不得直接监听 `keydown` 硬编码键值；运行期路由器 `InputActionRouter` 归属 `@rynvek/client`（M6），
 * 本模块只提供表与纯函数匹配器，Studio 在 M5 先以它驱动 `studio.*` 动作（ERRATA E-017）。
 */

/** 交互作用域 */
export type InteractionContext = "Global" | "Canvas" | "RTS_Gameplay" | "Studio_IDE" | "Code_Editor";

/** 作用域特异性（越大越具体，匹配时优先） */
export const CONTEXT_SPECIFICITY: Readonly<Record<InteractionContext, number>> = {
  Global: 0,
  Canvas: 1,
  RTS_Gameplay: 2,
  Studio_IDE: 2,
  Code_Editor: 3,
};

/** 键盘组合键（`key` 为规范化后的 `KeyboardEvent.key`：字母小写、空格为 `" "`） */
export interface KeyChord {
  readonly key: string;
  readonly ctrl: boolean;
  readonly meta: boolean;
  readonly shift: boolean;
  readonly alt: boolean;
}

/** 指针 / 滚轮手势 */
export type PointerGesture =
  | { readonly kind: "wheel"; readonly direction: "up" | "down" }
  | { readonly kind: "click"; readonly button: "left" | "right"; readonly modifier?: "shift" | "ctrl"; readonly double?: boolean }
  | { readonly kind: "drag"; readonly button: "left" }
  /** 先按下某键再左键点击（如 `Q` + `LeftClick`） */
  | { readonly kind: "key_then_click"; readonly key: string };

export interface ActionBinding {
  /** 规范编号 `01`–`36` */
  readonly no: string;
  readonly id: LogicalActionId;
  readonly context: InteractionContext;
  readonly desktop: string;
  readonly touch: string;
  /** 规范中为 `—` 的项记为 null */
  readonly gamepad: string | null;
  readonly keyboard: readonly KeyChord[];
  readonly pointer: readonly PointerGesture[];
  /** `1..9` 类绑定：数字键同时携带编队编号 */
  readonly digitGroup: boolean;
}

function chord(spec: string): KeyChord {
  const parts = spec.split("+");
  const raw = parts.pop()!;
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  return {
    key: normalizeKey(raw),
    ctrl: mods.has("ctrl"),
    meta: mods.has("cmd"),
    shift: mods.has("shift"),
    alt: mods.has("alt"),
  };
}

/** 规范键名 / `KeyboardEvent.key` → 规范化键 */
export function normalizeKey(key: string): string {
  switch (key) {
    case "Space":
    case "Spacebar":
      return " ";
    case "Period (.)":
    case "Period":
      return ".";
    default:
      return key.length === 1 ? key.toLowerCase() : key;
  }
}

/** 36 个逻辑动作 ID（规范编号顺序） */
export const LOGICAL_ACTION_IDS = [
  "global.command_palette",
  "global.cancel_or_menu",
  "global.toggle_inspector",
  "global.toggle_minimap",
  "global.toggle_neural_overlay",
  "global.toggle_field_overlay",
  "global.cycle_audio_mute",
  "camera.pan_up",
  "camera.pan_down",
  "camera.pan_left",
  "camera.pan_right",
  "camera.zoom_in",
  "camera.zoom_out",
  "camera.jump_to_nexus",
  "rts.select_primary",
  "rts.box_select",
  "rts.select_add_toggle",
  "rts.select_same_archetype",
  "rts.context_order",
  "rts.order_attack_move",
  "rts.order_hold_position",
  "rts.order_patrol",
  "rts.open_build_menu",
  "rts.rotate_building_cw",
  "rts.demolish_selected",
  "rts.beacon_modulator_panel",
  "rts.release_human_lease",
  "rts.assign_control_group_1_9",
  "rts.recall_control_group_1_9",
  "studio.sim_play_pause",
  "studio.sim_step_single_tick",
  "studio.tool_brush_field",
  "studio.tool_spawn_organism",
  "studio.undo_patch",
  "studio.redo_patch",
  "studio.compile_and_hot_reload",
] as const;
export type LogicalActionId = (typeof LOGICAL_ACTION_IDS)[number];

const L = { kind: "click", button: "left" } as const;

type Row = [no: string, id: LogicalActionId, ctx: InteractionContext, desktop: string, touch: string, gamepad: string | null, keyboard: string[], pointer: PointerGesture[], digitGroup?: boolean];

const ROWS: Row[] = [
  ["01", "global.command_palette", "Global", "Ctrl+K / Cmd+K", "三指轻点顶部栏", "View + Menu", ["Ctrl+K", "Cmd+K"], []],
  ["02", "global.cancel_or_menu", "Global", "Escape", "左上角返回键", "B (East)", ["Escape"], []],
  ["03", "global.toggle_inspector", "Global", "Tab", "右侧边缘左滑", "Y (North)", ["Tab"], []],
  ["04", "global.toggle_minimap", "Global", "M", "点击小地图缩略框", "D-Pad Up", ["M"], []],
  ["05", "global.toggle_neural_overlay", "Global", "N", "双指长按空白处", "D-Pad Right", ["N"], []],
  ["06", "global.toggle_field_overlay", "Global", "F", "工具栏 Field 按钮", "D-Pad Left", ["F"], []],
  ["07", "global.cycle_audio_mute", "Global", "Ctrl+M", "状态栏音频图标", null, ["Ctrl+M"], []],
  ["08", "camera.pan_up", "Canvas", "W / ArrowUp", "单指拖拽平移", "左摇杆 LS Up", ["W", "ArrowUp"], []],
  ["09", "camera.pan_down", "Canvas", "S / ArrowDown", "单指拖拽平移", "左摇杆 LS Down", ["S", "ArrowDown"], []],
  ["10", "camera.pan_left", "Canvas", "A / ArrowLeft", "单指拖拽平移", "左摇杆 LS Left", ["A", "ArrowLeft"], []],
  ["11", "camera.pan_right", "Canvas", "D / ArrowRight", "单指拖拽平移", "左摇杆 LS Right", ["D", "ArrowRight"], []],
  ["12", "camera.zoom_in", "Canvas", "WheelUp / =", "双指张开捏合", "RT (右扳机)", ["="], [{ kind: "wheel", direction: "up" }]],
  ["13", "camera.zoom_out", "Canvas", "WheelDown / -", "双指收缩捏合", "LT (左扳机)", ["-"], [{ kind: "wheel", direction: "down" }]],
  ["14", "camera.jump_to_nexus", "Canvas", "Backspace / Home", "双击主脑头像", "LS Click (L3)", ["Backspace", "Home"], []],
  ["15", "rts.select_primary", "RTS_Gameplay", "MouseLeftClick", "单指点击", "A (South)", [], [L]],
  ["16", "rts.box_select", "RTS_Gameplay", "MouseLeftDrag", "长按 150ms 后拖框", "按住 A + 右摇杆画框", [], [{ kind: "drag", button: "left" }]],
  ["17", "rts.select_add_toggle", "RTS_Gameplay", "Shift+MouseLeft", "多选模式开关 + 点击", "LB + A", [], [{ kind: "click", button: "left", modifier: "shift" }]],
  ["18", "rts.select_same_archetype", "RTS_Gameplay", "DoubleClick / Ctrl+Click", "双击目标单位", "双击 A", [], [{ kind: "click", button: "left", double: true }, { kind: "click", button: "left", modifier: "ctrl" }]],
  ["19", "rts.context_order", "RTS_Gameplay", "MouseRightClick", "双指点击目标点", "X (West)", [], [{ kind: "click", button: "right" }]],
  ["20", "rts.order_attack_move", "RTS_Gameplay", "Q + LeftClick", "战术轮盘 Attack", "RB + X", [], [{ kind: "key_then_click", key: "q" }]],
  ["21", "rts.order_hold_position", "RTS_Gameplay", "H", "战术轮盘 Hold", "RB + B", ["H"], []],
  ["22", "rts.order_patrol", "RTS_Gameplay", "P + LeftClick", "战术轮盘 Patrol", "RB + Y", [], [{ kind: "key_then_click", key: "p" }]],
  ["23", "rts.open_build_menu", "RTS_Gameplay", "B", "底部建造抽屉按钮", "LB", ["B"], []],
  ["24", "rts.rotate_building_cw", "RTS_Gameplay", "R", "建造虚影旋转图标", "RS Click (R3)", ["R"], []],
  ["25", "rts.demolish_selected", "RTS_Gameplay", "Delete / X", "长按删除按钮", "LB + RS Click", ["Delete", "X"], []],
  ["26", "rts.beacon_modulator_panel", "RTS_Gameplay", "E", "点击信标调制环", "RB + A", ["E"], []],
  ["27", "rts.release_human_lease", "RTS_Gameplay", "Alt+A", "顶栏交还 AI 控制按钮", "D-Pad Down", ["Alt+A"], []],
  ["28", "rts.assign_control_group_1_9", "RTS_Gameplay", "Ctrl+1..9", "长按底部编队槽 1..9", null, [1, 2, 3, 4, 5, 6, 7, 8, 9].map((d) => `Ctrl+${d}`), [], true],
  ["29", "rts.recall_control_group_1_9", "RTS_Gameplay", "1..9", "点击底部编队槽 1..9", null, [1, 2, 3, 4, 5, 6, 7, 8, 9].map(String), [], true],
  ["30", "studio.sim_play_pause", "Studio_IDE", "Space", "顶栏播放/暂停键", "Start", ["Space"], []],
  ["31", "studio.sim_step_single_tick", "Studio_IDE", "Period (.)", "顶栏单步前进键", "RB", ["Period (.)"], []],
  ["32", "studio.tool_brush_field", "Studio_IDE", "G", "工具栏画笔图标", null, ["G"], []],
  ["33", "studio.tool_spawn_organism", "Studio_IDE", "V", "工具栏孵化图标", null, ["V"], []],
  ["34", "studio.undo_patch", "Studio_IDE", "Ctrl+Z / Cmd+Z", "双指左滑撤销", null, ["Ctrl+Z", "Cmd+Z"], []],
  ["35", "studio.redo_patch", "Studio_IDE", "Ctrl+Shift+Z", "双指右滑重做", null, ["Ctrl+Shift+Z"], []],
  ["36", "studio.compile_and_hot_reload", "Code_Editor", "Ctrl+S / Cmd+S", "编辑器右上角 Apply", null, ["Ctrl+S", "Cmd+S"], []],
];

/** 36 项动作全表（按规范编号排序） */
export const ACTION_BINDINGS: readonly ActionBinding[] = ROWS.map(([no, id, context, desktop, touch, gamepad, keyboard, pointer, digitGroup]) => ({
  no,
  id,
  context,
  desktop,
  touch,
  gamepad,
  keyboard: keyboard.map(chord),
  pointer,
  digitGroup: digitGroup ?? false,
}));


const BY_ID = new Map<LogicalActionId, ActionBinding>(ACTION_BINDINGS.map((b) => [b.id, b]));

export function actionBinding(id: LogicalActionId): ActionBinding | undefined {
  return BY_ID.get(id);
}

/** `KeyboardEvent` 的最小形状 */
export interface KeyEventLike {
  readonly key: string;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
}

export interface ActionMatch {
  readonly binding: ActionBinding;
  /** `1..9` 编队类动作的编号 */
  readonly group?: number;
}

function chordEquals(c: KeyChord, e: KeyEventLike): boolean {
  return (
    c.key === normalizeKey(e.key) &&
    c.ctrl === e.ctrlKey &&
    c.meta === e.metaKey &&
    c.shift === e.shiftKey &&
    c.alt === e.altKey
  );
}

/**
 * 键盘事件 → 逻辑动作：只在活动作用域内匹配，修饰键必须完全一致；
 * 多个作用域同时命中时取最具体的作用域（例如代码编辑器中 `Ctrl+S` 优先于其他绑定）。
 */
export function matchKeyboard(e: KeyEventLike, active: readonly InteractionContext[]): ActionMatch | null {
  let best: ActionMatch | null = null;
  for (const b of ACTION_BINDINGS) {
    if (!active.includes(b.context)) continue;
    const hit = b.keyboard.find((c) => chordEquals(c, e));
    if (!hit) continue;
    if (best && CONTEXT_SPECIFICITY[best.binding.context] >= CONTEXT_SPECIFICITY[b.context]) continue;
    best = b.digitGroup ? { binding: b, group: Number(hit.key) } : { binding: b };
  }
  return best;
}

/** 组合键的展示文本（命令面板 / Tooltip 用），例如 `Ctrl+Shift+Z` */
export function formatChord(c: KeyChord): string {
  const parts: string[] = [];
  if (c.ctrl) parts.push("Ctrl");
  if (c.meta) parts.push("Cmd");
  if (c.alt) parts.push("Alt");
  if (c.shift) parts.push("Shift");
  parts.push(c.key === " " ? "Space" : c.key.length === 1 ? c.key.toUpperCase() : c.key);
  return parts.join("+");
}

/** 检查可同时激活的作用域之间是否存在完全相同的组合键冲突 */
export function findBindingConflicts(): [string, string, string][] {
  const out: [string, string, string][] = [];
  const coActive = (a: InteractionContext, b: InteractionContext) =>
    a === b || a === "Global" || b === "Global" || a === "Canvas" || b === "Canvas";
  for (let i = 0; i < ACTION_BINDINGS.length; i++) {
    for (let j = i + 1; j < ACTION_BINDINGS.length; j++) {
      const a = ACTION_BINDINGS[i]!;
      const b = ACTION_BINDINGS[j]!;
      if (!coActive(a.context, b.context)) continue;
      for (const ca of a.keyboard) {
        for (const cb of b.keyboard) {
          if (formatChord(ca) === formatChord(cb)) out.push([a.id, b.id, formatChord(ca)]);
        }
      }
    }
  }
  return out;
}
