// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `@rynvek/desktop-shell`：Rynvek 桌面客户端宿主配置与 IPC 桥接策略（审计报告 §12.3）。
 */

export interface DesktopShellConfig {
  readonly dataDir: string;
  readonly localServerPort: number;
  readonly gpuBackendPreference: "vulkan" | "metal" | "dx12" | "auto";
  readonly maxMemoryBudgetMb: number;
}

export const DEFAULT_DESKTOP_SHELL_CONFIG: DesktopShellConfig = {
  dataDir: ".rynvek-desktop",
  localServerPort: 18080,
  gpuBackendPreference: "auto",
  maxMemoryBudgetMb: 2048,
};

export interface DesktopStoragePaths {
  readonly sqliteDbPath: string;
  readonly snapshotsDir: string;
  readonly journalsDir: string;
  readonly blobsDir: string;
}

/** 构造桌面端本地持久化目录布局 */
export function resolveDesktopStoragePaths(config: DesktopShellConfig = DEFAULT_DESKTOP_SHELL_CONFIG): DesktopStoragePaths {
  const base = config.dataDir.replace(/\/+$/, "");
  return {
    sqliteDbPath: `${base}/local.sqlite3`,
    snapshotsDir: `${base}/snapshots`,
    journalsDir: `${base}/journals`,
    blobsDir: `${base}/blobs`,
  };
}
