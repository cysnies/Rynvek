// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { SpecError, ablation, experiment, seedRange, sweep } from "../src/index.js";

describe("实验设计", () => {
  it("流式构建器产出已校验的规格", () => {
    const spec = experiment("sweep")
      .seeds(...seedRange(10, 3))
      .ticks(600)
      .sampleEvery(60)
      .variants(sweep({ mode: ["pve", "duel"], policy: ["idle", "economy"] }))
      .build();
    expect(spec.seeds).toEqual([10, 11, 12]);
    expect(spec.sample_every).toBe(60);
    expect(spec.variants?.map((v) => v.label)).toEqual([
      "pve-idle-p1",
      "pve-economy-p1",
      "duel-idle-p2",
      "duel-economy-p2",
    ]);
    expect(() => experiment("bad").ticks(10).build()).toThrow(SpecError);
  });

  it("扫描剔除不合法组合", () => {
    const vs = sweep({ mode: ["pve", "duel"], players: [1, 2] });
    expect(vs.map((v) => v.label)).toEqual(["pve-economy-p1", "pve-economy-p2", "duel-economy-p2"]);
    expect(() => sweep({ mode: ["duel"], players: [1] })).toThrow(SpecError);
  });

  it("消融：基线 + 单因子覆盖", () => {
    const vs = ablation({ "no-policy": { policy: "idle" }, coop: { players: 2 } });
    expect(vs).toEqual([
      { label: "baseline", mode: "pve", policy: "economy" },
      { label: "no-policy", mode: "pve", policy: "idle" },
      { label: "coop", mode: "pve", policy: "economy", players: 2 },
    ]);
    expect(() => experiment("abl").seeds(1).ticks(30).variants(vs).build()).not.toThrow();
  });
});
