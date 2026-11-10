// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  DEFAULT_SAMPLE_EVERY,
  MAX_RUNS,
  MAX_TICKS,
  SpecError,
  TABLE_COLUMNS,
  effectiveSampleEvery,
  planRuns,
  seedRange,
  toRequestBody,
  validateSpec,
} from "../src/index.js";

const rust = (rel: string) => readFileSync(fileURLToPath(new URL(`../../../${rel}`, import.meta.url)), "utf8");

describe("与 Rust rynvek_rts::lab 对齐", () => {
  it("常量一致", () => {
    const src = rust("crates/rynvek-rts/src/lab/mod.rs");
    const constant = (name: string) => Number(new RegExp(`pub const ${name}: \\w+ = ([\\d_]+);`).exec(src)?.[1]?.replaceAll("_", ""));
    expect(constant("MAX_RUNS")).toBe(MAX_RUNS);
    expect(constant("MAX_TICKS")).toBe(MAX_TICKS);
    expect(constant("DEFAULT_SAMPLE_EVERY")).toBe(DEFAULT_SAMPLE_EVERY);
  });

  it("结果表列名与 arrow_io.rs 一致", () => {
    const src = rust("crates/rynvek-rts/src/lab/arrow_io.rs");
    const schema = (fn: string) => {
      const body = new RegExp(`pub fn ${fn}\\(\\) -> SchemaRef \\{([\\s\\S]*?)\\n\\}`).exec(src)?.[1] ?? "";
      return [...body.matchAll(/field\("(\w+)"/g)].map((m) => m[1]);
    };
    expect(schema("runs_schema")).toEqual(TABLE_COLUMNS.runs);
    expect(schema("metrics_schema")).toEqual(TABLE_COLUMNS.metrics);
    expect(schema("lineage_schema")).toEqual(TABLE_COLUMNS.lineage);
  });
});

describe("validateSpec", () => {
  const ok = { name: "exp", seeds: [1, 2], ticks: 300 };

  it("接受合法规格并补齐缺省采样间隔", () => {
    expect(() => validateSpec(ok)).not.toThrow();
    expect(effectiveSampleEvery(ok)).toBe(30);
    expect(effectiveSampleEvery({ ...ok, ticks: 10 })).toBe(10);
  });

  it.each([
    [{ ...ok, name: "  " }, "name"],
    [{ ...ok, name: "名".repeat(129) }, "name"],
    [{ ...ok, seeds: [] }, "seeds"],
    [{ ...ok, seeds: [-1] }, "种子"],
    [{ ...ok, seeds: [2 ** 60] }, "种子"],
    [{ ...ok, ticks: 0 }, "ticks"],
    [{ ...ok, ticks: MAX_TICKS + 1 }, "ticks"],
    [{ ...ok, sample_every: 301 }, "sample_every"],
    [{ ...ok, seeds: seedRange(0, 129), variants: [{ label: "a" }, { label: "b" }] }, "超过上限"],
    [{ ...ok, variants: [{ label: "a" }, { label: "a" }] }, "重复"],
    [{ ...ok, variants: [{ label: "" }] }, "label"],
    [{ ...ok, variants: [{ label: "d", mode: "duel" as const, players: 1 }] }, "players"],
    [{ ...ok, variants: [{ label: "p", players: 3 }] }, "players"],
  ])("拒绝 %#", (spec, msg) => {
    expect(() => validateSpec(spec)).toThrow(SpecError);
    expect(() => validateSpec(spec)).toThrow(msg);
  });

  it("名字按字符计数（与 Rust chars().count() 一致）", () => {
    expect(() => validateSpec({ ...ok, name: "🧬".repeat(128) })).not.toThrow();
  });
});

describe("planRuns / toRequestBody", () => {
  it("变体优先、种子次之", () => {
    const runs = planRuns({ name: "x", seeds: [7, 8], ticks: 30, variants: [{ label: "a" }, { label: "b" }] });
    expect(runs.map((r) => [r.run, r.variant.label, r.seed])).toEqual([
      [0, "a", 7],
      [1, "a", 8],
      [2, "b", 7],
      [3, "b", 8],
    ]);
    expect(planRuns({ name: "x", seeds: [1], ticks: 30 })[0]?.variant.label).toBe("baseline");
  });

  it("请求体只含已设置字段", () => {
    expect(toRequestBody({ name: "x", seeds: [1], ticks: 60, variants: [{ label: "d", mode: "duel" }] })).toEqual({
      name: "x",
      seeds: [1],
      ticks: 60,
      variants: [{ label: "d", mode: "duel" }],
    });
    expect(() => toRequestBody({ name: "", seeds: [1], ticks: 1 })).toThrow(SpecError);
  });
});
