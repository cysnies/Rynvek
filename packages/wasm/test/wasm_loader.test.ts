// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";
import { RYNVEK_WASM_EXPORTS, encodeWasmInitCommand, verifyWasmExports } from "../src/index.js";

describe("@rynvek/wasm", () => {
  it("encodes ENGINE_INIT and verifies ABI export symbols", () => {
    const json = encodeWasmInitCommand({
      type: "ENGINE_INIT",
      width: 1280,
      height: 720,
      devicePixelRatio: 2,
      gpuMode: "auto",
    });
    expect(JSON.parse(json).width).toBe(1280);
    const mockExports = Object.fromEntries(RYNVEK_WASM_EXPORTS.map((k) => [k, () => {}]));
    expect(verifyWasmExports(mockExports)).toBe(true);
  });
});
