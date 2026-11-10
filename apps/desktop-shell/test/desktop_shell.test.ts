// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import { DEFAULT_DESKTOP_SHELL_CONFIG, resolveDesktopStoragePaths } from "../src/index.js";

describe("@rynvek/desktop-shell", () => {
  it("resolves canonical local storage paths", () => {
    const paths = resolveDesktopStoragePaths(DEFAULT_DESKTOP_SHELL_CONFIG);
    expect(paths.sqliteDbPath).toBe(".rynvek-desktop/local.sqlite3");
    expect(paths.snapshotsDir).toBe(".rynvek-desktop/snapshots");
    expect(paths.journalsDir).toBe(".rynvek-desktop/journals");
  });
});
