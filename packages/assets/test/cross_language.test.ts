// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 跨语言容器夹具：`tests/packages/sample.rview` 由本包写出，Rust `rynvek-world::package` 必须逐字节复现。
// 更新夹具：RYNVEK_UPDATE_FIXTURES=1 pnpm --filter @rynvek/assets test

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { readRview } from "../src/index.js";
import { samplePack } from "./fixtures.js";

const FIXTURE = fileURLToPath(new URL("../../../tests/packages/sample.rview", import.meta.url));

describe("跨语言 .rview 夹具", () => {
  it("已提交夹具与 TypeScript 写出器逐字节一致", () => {
    const bytes = samplePack();
    if (process.env["RYNVEK_UPDATE_FIXTURES"] === "1" || !existsSync(FIXTURE)) writeFileSync(FIXTURE, bytes);
    const committed = new Uint8Array(readFileSync(FIXTURE));
    expect(committed).toEqual(bytes);
    expect(readRview(committed).manifest.entries.map((e) => e.path)).toEqual([
      "palette/factions.json",
      "shaders/glow.wgsl",
      "sprites/atlas.json",
      "sprites/atlas.png",
      "theme/biome.json",
    ]);
  });
});
