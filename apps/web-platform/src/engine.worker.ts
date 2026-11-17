// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { bootstrapEngineWorker, type WorkerScopeLike } from "@rynvek/engine";
import init, { EngineWorker } from "./generated/wasm/rynvek_wasm.js";
import wasmUrl from "./generated/wasm/rynvek_wasm_bg.wasm?url";

// 不是先 await 再监听：OffscreenCanvas/观察在二进制加载期间也必须按序保留。
bootstrapEngineWorker(globalThis as unknown as WorkerScopeLike, async () => {
  await init({ module_or_path: wasmUrl });
  return new EngineWorker();
});
