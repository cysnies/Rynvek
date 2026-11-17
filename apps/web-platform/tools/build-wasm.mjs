// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const app = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(app, "../..");

/** 不下载工具、不删除缓存；CLI 必须与锁定的 Rust ABI 完全同版。 */
export function buildWasm({ run = spawnSync, env = process.env } = {}) {
  const version = readFileSync(resolve(root, "Cargo.lock"), "utf8").match(/name = "wasm-bindgen"\r?\nversion = "([^"]+)"/)?.[1];
  if (!version) throw new Error("Cargo.lock 未找到 wasm-bindgen 版本");
  const cli = env.WASM_BINDGEN || "wasm-bindgen";
  const check = run(cli, ["--version"], { encoding: "utf8", env });
  if (check.status !== 0 || check.stdout?.trim() !== `wasm-bindgen ${version}`) {
    throw new Error(`需要 wasm-bindgen ${version}；请用 WASM_BINDGEN 指定官方同版 CLI，不接受旧胶水`);
  }
  const buildEnv = { ...env, CARGO_BUILD_JOBS: env.CARGO_BUILD_JOBS || "2",
    CARGO_INCREMENTAL: "0", CARGO_PROFILE_DEV_DEBUG: "0" };
  const target = resolve(root, env.CARGO_TARGET_DIR || "target");
  for (const [command, args] of [
    ["cargo", ["build", "--locked", "-p", "rynvek-wasm", "--target", "wasm32-unknown-unknown"]],
    [cli, [resolve(target, "wasm32-unknown-unknown/debug/rynvek_wasm.wasm"), "--target", "web",
      "--out-name", "rynvek_wasm", "--out-dir", resolve(app, "src/generated/wasm")]],
  ]) {
    const result = run(command, args, { cwd: root, env: buildEnv, stdio: "inherit" });
    if (result.status !== 0) throw new Error(`${command} 失败（${result.status ?? result.error?.message}），不使用旧产物冒充成功`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { buildWasm(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
