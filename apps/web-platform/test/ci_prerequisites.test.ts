// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const workflow = readFileSync(new URL("../../../.github/workflows/ci.yml", import.meta.url), "utf8");
const gate = readFileSync(new URL("../../../crates/xtask/src/gate.rs", import.meta.url), "utf8");

describe("完整CI门禁的真实工具前置条件", () => {
  it("提供经官方SHA256校验的固定版cargo-deny及锁定缓存，不等到全测试结束才发现缺工具", () => {
    expect(workflow).toContain("cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz");
    expect(workflow).toContain("9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f");
    expect(workflow).toContain("sha256sum --check");
    expect(workflow).toContain("cargo fetch --locked");
    expect(workflow).toContain("npm install --global pnpm@10.21.0");
    expect(workflow).toContain("pnpm fetch --frozen-lockfile");
    expect(workflow).toContain("install uv==0.13.8");
    expect(workflow.indexOf("Prepare full gate prerequisites")).toBeLessThan(workflow.indexOf("Run Rynvek CI Full Gate"));
  });
  it("只补依赖，不关闭离线许可证检查或跳过原完整门禁", () => {
    expect(workflow).toContain("cargo xtask ci-full-gate --soak-duration 90s");
    expect(workflow).not.toMatch(/ci-full-gate[^\n]*--skip/);
    expect(gate).toContain('["deny", "--offline", "check", "licenses", "bans"]');
  });
});
