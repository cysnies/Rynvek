// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 把 `.rview` 视觉包转成五层主题隔离（GAP-14）中的主题层。
//
// - 主题条目的每个令牌按其所属命名空间落入 world / faction / visualization 层；
// - 色板条目中名为令牌后缀的颜色（如 `faction-1` → `--rv-faction-1`）同样落层，其余色名只留在色板里供渲染使用；
// - 应用层令牌只接受 TrustLevel 0/1（官方皮肤）；社区与临时包改写应用 UI 有仿冒风险，一律剔除并报告；
// - 所有值再经 `validateLayer` 的 CSS 安全校验（ERRATA E-020）。

import { TrustLevel, type RviewPackage } from "@rynvek/assets";
import {
  isRvToken,
  tokenLayer,
  validateLayer,
  type RvToken,
  type ThemeLayer,
  type ThemeLayerKind,
} from "@rynvek/ui";

export interface RejectedToken {
  readonly entry: string;
  readonly token: string;
  readonly reason: "unknown_token" | "application_layer_untrusted" | "unsafe_value";
}

export interface RviewThemeLayers {
  readonly layers: readonly ThemeLayer[];
  readonly rejected: readonly RejectedToken[];
}

/** 层的稳定输出顺序（低 → 高） */
const ORDER: readonly ThemeLayerKind[] = ["application", "world", "faction", "visualization"];

export function rviewThemeLayers(pkg: RviewPackage): RviewThemeLayers {
  const source = `rview:${pkg.manifest.id}@${pkg.manifest.version}`;
  const buckets = new Map<ThemeLayerKind, Partial<Record<RvToken, string>>>();
  const rejected: RejectedToken[] = [];
  const allowApplication = pkg.trustLevel <= TrustLevel.OfficialSigned;

  const put = (entry: string, token: string, value: string): void => {
    if (!isRvToken(token)) {
      rejected.push({ entry, token, reason: "unknown_token" });
      return;
    }
    const kind = tokenLayer(token);
    if (kind === "application" && !allowApplication) {
      rejected.push({ entry, token, reason: "application_layer_untrusted" });
      return;
    }
    // 单令牌校验，避免一个坏值连累整层
    try {
      validateLayer({ kind, source, tokens: { [token]: value } });
    } catch {
      rejected.push({ entry, token, reason: "unsafe_value" });
      return;
    }
    const bucket = buckets.get(kind) ?? {};
    bucket[token] = value;
    buckets.set(kind, bucket);
  };

  // 先色板后主题：同一令牌以显式主题条目为准；条目按路径排序，结果确定
  for (const [path, palette] of [...pkg.palettes].sort(([a], [b]) => (a < b ? -1 : 1))) {
    for (const [name, color] of Object.entries(palette.colors)) {
      if (isRvToken(`--rv-${name}`)) put(path, `--rv-${name}`, color);
    }
  }
  for (const [path, theme] of [...pkg.themes].sort(([a], [b]) => (a < b ? -1 : 1))) {
    for (const [token, value] of Object.entries(theme.tokens)) put(path, token, value);
  }

  const layers: ThemeLayer[] = [];
  for (const kind of ORDER) {
    const tokens = buckets.get(kind);
    if (tokens !== undefined) layers.push({ kind, source, tokens });
  }
  return { layers, rejected };
}
