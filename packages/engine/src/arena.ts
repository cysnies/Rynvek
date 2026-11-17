// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/** 现有 ArenaClient → 现有 Dedicated Worker。只转发观察，不拥有仿真或命令 API。 */
import type { ArenaClient } from "@rynvek/client";
import { EngineHost, type TransferableCanvasLike, type WorkerLike } from "./host.js";
import type { EngineInitOptions } from "./protocol.js";

export interface ArenaRenderOptions extends EngineInitOptions {
  worldId: string;
  arenaFaction: number;
  onError(error: Error): void;
}

/** 画布一次移交；组件卸载先退订，再关闭 Worker。实际游戏输入仍走 ArenaController/SDK。 */
export function createArenaRenderHost(
  worker: WorkerLike,
  canvas: TransferableCanvasLike,
  client: Pick<ArenaClient, "on" | "observation"> & Partial<Pick<ArenaClient, "terrain" | "welcome">>,
  options: ArenaRenderOptions,
): { host: EngineHost; dispose(): void } {
  if (!/^[0-9a-f]{32}$/i.test(options.worldId) || !Number.isInteger(options.arenaFaction)
      || options.arenaFaction < 0 || options.arenaFaction > 255 || options.gpuMode !== "required") {
    throw new Error("竞技场渲染必须绑定合法世界/阵营并要求 GPU；不把空白 CPU 模式冒充显示成功");
  }
  const host = new EngineHost(worker);
  const offError = host.on("ENGINE_ERROR", (event) => options.onError(new Error(event.message)));
  const offDegraded = host.on("BACKEND_DEGRADED", (event) => options.onError(new Error(event.reason)));
  try { host.attach(canvas, options); }
  catch (error) { host.shutdown(); throw error; }
  let mapSent = false;
  const present = (observation: NonNullable<ArenaClient["observation"]>): void => {
    if (observation.faction !== options.arenaFaction) {
      options.onError(new Error("拒绝向渲染 Worker 转发其他阵营观察"));
      return;
    }
    try {
      // 使用 SDK 已经核验的 welcome 地形，不重新生图；先地图后观察，Worker 再校验。
      if (!mapSent && client.terrain && client.welcome) {
        const {width, height, tiles} = client.terrain;
        host.send({ type: "ARENA_MAP", worldId: options.worldId,
          map: {width, height, tiles: Array.from(tiles), tilesHash: client.welcome.map.tiles_hash} });
        mapSent = true;
      }
      host.send({ type: "ARENA_OBSERVATION", worldId: options.worldId, observation });
    }
    catch (error) { options.onError(error instanceof Error ? error : new Error(String(error))); }
  };
  const offObservation = client.on((event) => {
    if (event.type === "observation") present(event.observation);
  });
  if (client.observation) present(client.observation);
  let disposed = false;
  return { host, dispose: () => {
    if (disposed) return;
    disposed = true;
    offObservation(); offError(); offDegraded(); host.shutdown();
  } };
}
