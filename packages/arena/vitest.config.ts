// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const src = (pkg: string) => fileURLToPath(new URL(`../${pkg}/src/index.ts`, import.meta.url));

export default defineConfig({
  resolve: {
    // 直接解析工作区依赖的源码，使 `pnpm --filter @rynvek/arena test` 无需预先构建依赖包
    alias: {
      "@rynvek/core": src("core"),
      "@rynvek/ui": src("ui"),
      "@rynvek/client": src("client"),
    },
  },
  test: {
    include: ["test/**/*.test.ts"],
    // 实况闭环会启动 Rust 无头会话进程
    testTimeout: 120_000,
  },
});
