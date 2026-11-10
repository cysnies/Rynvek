// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import { WEB_PLATFORM_SURFACES, resolvePlatformSurface } from "../src/index.js";

describe("@rynvek/web-platform", () => {
  it("resolves all four flagship surfaces", () => {
    expect(WEB_PLATFORM_SURFACES).toEqual(["arena", "studio", "lab", "hub"]);
    expect(resolvePlatformSurface("/studio/project-1").surface).toBe("studio");
    expect(resolvePlatformSurface("/lab").requiresWebGpuCanvas).toBe(false);
    expect(resolvePlatformSurface("/").surface).toBe("arena");
  });
});
