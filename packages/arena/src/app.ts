// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `createArenaApp`：组装一个玩家席位的完整前端闭环（连接 → 输入路由 → 控制器 → HUD / 小地图）。
 * 渲染层只需订阅 `client.on("observation")` 并读取 `hud()` / `minimap()`。
 */

import { ArenaClient, InputActionRouter, type ArenaIdentity, type ArenaTransport } from "@rynvek/client";

import { ArenaController } from "./controller.js";
import { renderMinimap, type Minimap } from "./minimap.js";
import { ArenaUiState, hudView, type HudView } from "./state.js";

export interface ArenaApp {
  readonly client: ArenaClient;
  readonly router: InputActionRouter;
  readonly ui: ArenaUiState;
  readonly controller: ArenaController;
  hud(): HudView | null;
  minimap(scale?: number): Minimap | null;
  dispose(): void;
}

export function createArenaApp(transport: ArenaTransport, identity: ArenaIdentity): ArenaApp {
  const client = new ArenaClient(transport, identity);
  // 对局画面同时激活画布与 RTS 作用域
  const router = new InputActionRouter(["Canvas", "RTS_Gameplay"]);
  const ui = new ArenaUiState();
  const controller = new ArenaController(client, router, ui);
  return {
    client,
    router,
    ui,
    controller,
    hud: () => (client.observation ? hudView(client.observation) : null),
    minimap: (scale = 2) => (client.observation && client.terrain ? renderMinimap(client.terrain, client.observation, scale) : null),
    dispose: () => {
      controller.dispose();
      client.dispose();
    },
  };
}
