// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// Studio 动作接线：把规范 12.1 中 `Studio_IDE` / `Code_Editor` 上下文的逻辑动作（30–36）
// 映射到仿真控制、工具切换、撤销重做与热重载。
// 全局输入路由器 `@rynvek/client::InputActionRouter` 属于 M6，这里只消费 `@rynvek/ui` 的匹配结果。

import type { AudioDirector } from "@rynvek/audio";
import type { EngineHost } from "@rynvek/engine";
import { matchKeyboard, type InteractionContext, type KeyEventLike, type LogicalActionId } from "@rynvek/ui";
import type { LocalEdit, UndoManager } from "./undo.js";

/** Studio 负责的逻辑动作 */
export const STUDIO_ACTION_IDS = [
  "studio.sim_play_pause",
  "studio.sim_step_single_tick",
  "studio.tool_brush_field",
  "studio.tool_spawn_organism",
  "studio.undo_patch",
  "studio.redo_patch",
  "studio.compile_and_hot_reload",
] as const satisfies readonly LogicalActionId[];
export type StudioActionId = (typeof STUDIO_ACTION_IDS)[number];

export type StudioTool = "select" | "brush_field" | "spawn_organism";

/** 仿真控制所需的最小宿主接口（`EngineHost` 满足） */
export type SimulationControl = Pick<EngineHost, "start" | "pause" | "step">;

export interface StudioCommandOptions {
  readonly sim: SimulationControl;
  readonly undo: UndoManager;
  /** 编译当前规则编辑器内容并热重载；返回补丁或 null（编译失败） */
  readonly compileAndHotReload?: () => LocalEdit | null;
  /** 播放时的 Tick 频率（缺省 30 Hz，与 E-016 的 `START.tickRate` 一致） */
  readonly tickRate?: number;
  /** 本地编辑产出的补丁（用于广播给协作者） */
  readonly onEdit?: (edit: LocalEdit) => void;
  /** 可选的界面音效：动作成功播放 `ui.confirm`，不可用播放 `ui.error` */
  readonly audio?: Pick<AudioDirector, "play">;
}

export interface StudioCommandState {
  readonly playing: boolean;
  readonly tool: StudioTool;
}

/** Studio 命令中心：状态机 + 动作分派，供工具栏、键盘与命令面板共用 */
export class StudioCommands {
  private playing = false;
  private tool: StudioTool = "select";
  private readonly listeners = new Set<(s: StudioCommandState) => void>();

  public constructor(private readonly options: StudioCommandOptions) {}

  public get state(): StudioCommandState {
    return { playing: this.playing, tool: this.tool };
  }

  public subscribe(fn: (s: StudioCommandState) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  /** 执行一个 Studio 动作；返回是否已处理（不可用时为 false） */
  public run(action: StudioActionId): boolean {
    const handled = this.dispatch(action);
    this.options.audio?.play(handled ? "ui.confirm" : "ui.error");
    if (handled) this.emit();
    return handled;
  }

  private dispatch(action: StudioActionId): boolean {
    const { sim, undo } = this.options;
    switch (action) {
      case "studio.sim_play_pause":
        if (this.playing) sim.pause();
        else sim.start(this.options.tickRate ?? 30);
        this.playing = !this.playing;
        break;
      case "studio.sim_step_single_tick":
        // 单步只在暂停时有意义：播放中先暂停，保证步进确定
        if (this.playing) {
          sim.pause();
          this.playing = false;
        }
        sim.step(1);
        break;
      case "studio.tool_brush_field":
        this.tool = this.tool === "brush_field" ? "select" : "brush_field";
        break;
      case "studio.tool_spawn_organism":
        this.tool = this.tool === "spawn_organism" ? "select" : "spawn_organism";
        break;
      case "studio.undo_patch": {
        const edit = undo.undo();
        if (edit === null) return false;
        this.options.onEdit?.(edit);
        break;
      }
      case "studio.redo_patch": {
        const edit = undo.redo();
        if (edit === null) return false;
        this.options.onEdit?.(edit);
        break;
      }
      case "studio.compile_and_hot_reload": {
        const edit = this.options.compileAndHotReload?.() ?? null;
        if (edit === null) return false;
        this.options.onEdit?.(edit);
        break;
      }
    }
    return true;
  }

  /** 处理键盘事件；返回被触发的动作（未命中或不属于 Studio 时为 null） */
  public handleKey(e: KeyEventLike, contexts: readonly InteractionContext[]): StudioActionId | null {
    const hit = matchKeyboard(e, contexts);
    const id = hit?.binding.id;
    if (id === undefined || !(STUDIO_ACTION_IDS as readonly string[]).includes(id)) return null;
    return this.run(id as StudioActionId) ? (id as StudioActionId) : null;
  }

  private emit(): void {
    const s = this.state;
    for (const l of this.listeners) l(s);
  }
}
