// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/web-platform`：Rynvek Web 平台旗舰应用路由与工作区控制器（规范 07 / 审计报告 §12.3）。
 *
 * 统一挂载四大核心工作台：
 * - `/arena`：Rynvek Arena 神经 RTS 对战与回放审查器；
 * - `/studio`：Rynvek Studio 实时协作世界编辑器（HLC + CRDT `WorldPatch`）；
 * - `/lab`：Rynvek Lab 确定性演化参数扫描与谱系分析台；
 * - `/hub`：Rynvek Hub 世界包（`.rworld`）、插件包（`.rpack`）与黄金回放库。
 */

export const WEB_PLATFORM_SURFACES = ["arena", "studio", "lab", "hub"] as const;
export type WebPlatformSurface = (typeof WEB_PLATFORM_SURFACES)[number];

export interface WebPlatformRoute {
  readonly surface: WebPlatformSurface;
  readonly path: string;
  readonly titleZh: string;
  readonly requiresWebGpuCanvas: boolean;
}

export const WEB_PLATFORM_ROUTES: Readonly<Record<WebPlatformSurface, WebPlatformRoute>> = {
  arena: {
    surface: "arena",
    path: "/arena",
    titleZh: "神经竞技场 (Arena)",
    requiresWebGpuCanvas: true,
  },
  studio: {
    surface: "studio",
    path: "/studio",
    titleZh: "世界创作工坊 (Studio)",
    requiresWebGpuCanvas: true,
  },
  lab: {
    surface: "lab",
    path: "/lab",
    titleZh: "演化实验台 (Lab)",
    requiresWebGpuCanvas: false,
  },
  hub: {
    surface: "hub",
    path: "/hub",
    titleZh: "社区与资产中心 (Hub)",
    requiresWebGpuCanvas: false,
  },
};

/** 解析 URL 路径对应的平台工作台（缺省回退至 `arena`） */
export function resolvePlatformSurface(pathname: string): WebPlatformRoute {
  const seg = pathname.replace(/^\/+/, "").split("/")[0] ?? "";
  if ((WEB_PLATFORM_SURFACES as readonly string[]).includes(seg)) {
    return WEB_PLATFORM_ROUTES[seg as WebPlatformSurface];
  }
  return WEB_PLATFORM_ROUTES.arena;
}
