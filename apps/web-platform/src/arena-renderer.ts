// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import type { ArenaClient } from "@rynvek/client";
import { createArenaRenderHost, type WorkerLike } from "@rynvek/engine";
import { viewScale, type Viewport } from "./arena-view.js";

export interface ArenaRendererOptions {
  worldId: string;
  faction: number;
  view: Viewport;
  dpr: number;
  createWorker(): WorkerLike;
  onStatus(status: string): void;
  onError(error: Error): void;
  readyTimeoutMs?: number;
}

/** 只管理画布/相机/生命周期；不发游戏命令、不产生世界状态、不自动降为假GPU。 */
export function mountArenaRenderer(canvas: HTMLCanvasElement,
  client: Pick<ArenaClient, "on" | "observation"> & Partial<Pick<ArenaClient, "terrain" | "welcome">>, options: ArenaRendererOptions) {
  if (typeof canvas.transferControlToOffscreen !== "function") throw new Error("当前环境不支持 OffscreenCanvas；请手动选择临时 Canvas2D");
  const pixels = (size: number, dpr: number) => Math.max(1, Math.round(size * dpr));
  let width = pixels(options.view.width, options.dpr), height = pixels(options.view.height, options.dpr);
  // 所有权移交之前仅设置一次；移交后只能给 Worker 发 RESIZE。
  canvas.width = width; canvas.height = height;
  const worker = options.createWorker();
  let renderer: ReturnType<typeof createArenaRenderHost> | undefined;
  let disposed = false, camera = "";
  let timer: ReturnType<typeof setTimeout> | undefined;
  let offReady = () => {};
  const dispose = () => {
    if (disposed) return;
    disposed = true; clearTimeout(timer); offReady();
    worker.removeEventListener("error", onWorkerError); worker.removeEventListener("messageerror", onWorkerError);
    if (renderer) renderer.dispose(); else worker.terminate?.();
  };
  const fail = (error: Error) => {
    if (disposed) return;
    dispose(); options.onStatus("WebGPU 显示失败"); options.onError(error);
  };
  const onWorkerError = () => fail(new Error("模块 Worker 脚本或消息反序列化失败"));
  worker.addEventListener("error", onWorkerError); worker.addEventListener("messageerror", onWorkerError);
  options.onStatus("加载真实 WASM / 等待 Worker GPU");
  try {
    renderer = createArenaRenderHost(worker, canvas, client, {
      width, height, devicePixelRatio: options.dpr, worldId: options.worldId,
      arenaFaction: options.faction, gpuMode: "required", onError: fail,
    });
    if (disposed) renderer.dispose();
  } catch (error) { dispose(); throw error; }
  const ready = () => { clearTimeout(timer); options.onStatus("WebGPU Worker 已就绪"); };
  if (!disposed) {
    offReady = renderer.host.on("ENGINE_READY", ready);
    if (renderer.host.state === "ready") ready();
    else timer = setTimeout(() => fail(new Error("WASM/GPU 就绪超时；未确认显示成功，请手动切换 Canvas2D")), options.readyTimeoutMs ?? 20000);
  }
  const sync = (view: Viewport, dpr: number) => {
    if (disposed) return;
    const w = pixels(view.width, dpr), h = pixels(view.height, dpr);
    if (width !== w || height !== h) { renderer!.host.resize(w, h, dpr); width = w; height = h; }
    const pxPerCell = viewScale({...view, pixelRatio: dpr}) * dpr;
    const key = `${view.x}/${view.y}/${pxPerCell}`;
    if (camera !== key) {
      renderer!.host.send({ type: "SET_CAMERA", centerX: view.x, centerY: view.y, pxPerCell }); camera = key;
    }
  };
  sync(options.view, options.dpr);
  let selection = "";
  const select = (units: readonly number[], building: number | null) => {
    if (disposed) return;
    const ids = [...new Set(units)].sort((a, b) => a - b);
    const key = JSON.stringify([ids, building]);
    if (key === selection) return;
    renderer!.host.send({type: "ARENA_SELECTION", worldId: options.worldId, units: ids, building});
    selection = key;
  };
  return { sync, select, dispose };
}
