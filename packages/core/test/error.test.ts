// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { ErrorRecoveryAction, RynvekError, RynvekErrorCode, errorNameOf, formatErrorCode, recoveryActionOf } from "../src/index.js";

const RUST_ERROR_SOURCE = fileURLToPath(new URL("../../../crates/rynvek-core/src/error.rs", import.meta.url));

/** 从 Rust 源码中解析 `RynvekErrorCode` 枚举的 `名称 = 0x数值` 对 */
function parseRustErrorCodes(): Map<string, number> {
  const source = readFileSync(RUST_ERROR_SOURCE, "utf8");
  const body = source.slice(source.indexOf("pub enum RynvekErrorCode"), source.indexOf("pub enum ErrorRecoveryAction"));
  const entries = [...body.matchAll(/^\s+(\w+) = 0x([0-9A-Fa-f]{4}),/gm)].map(
    (m): [string, number] => [m[1] ?? "", Number.parseInt(m[2] ?? "", 16)],
  );
  return new Map(entries);
}

describe("RynvekErrorCode", () => {
  it("与 Rust 注册表逐项一致", () => {
    const rust = parseRustErrorCodes();
    expect(rust.size).toBe(42);
    expect(new Map(Object.entries(RynvekErrorCode))).toEqual(rust);
  });

  it("反查、格式化与恢复动作", () => {
    expect(errorNameOf(0x1003)).toBe("CoordinateOutOfCanonicalRange");
    expect(errorNameOf(0x1000)).toBeUndefined();
    expect(formatErrorCode(RynvekErrorCode.CoordinateOutOfCanonicalRange)).toBe("RV-0x1003 CoordinateOutOfCanonicalRange");
    expect(recoveryActionOf(RynvekErrorCode.GpuDeviceLost)).toBe(ErrorRecoveryAction.RecoverOrDegradeBackend);
    expect(recoveryActionOf(RynvekErrorCode.RuleUndeclaredWriteAccess)).toBeUndefined();
    const err = new RynvekError(RynvekErrorCode.FixedPointOverflow, "detail");
    expect(err.message).toBe("RV-0x1002 FixedPointOverflow: detail");
    expect(err.code).toBe(0x1002);
  });
});
