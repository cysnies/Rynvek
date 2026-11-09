// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 扩展包信任等级（项目文档“TrustLevel 0–3”，数值与 Hub 表 `trust_level` 列一致：
// 0 Builtin、1 Official、2 Verified、3 Community / Untrusted Ephemeral）。

export const TrustLevel = {
  /** 随官方二进制编译发布的内核模块 */
  CoreBuiltin: 0,
  /** Rynvek 官方 Ed25519 签名、`rynvek.*` 命名空间 */
  OfficialSigned: 1,
  /** Hub 实名开发者签名并通过自动化审查 */
  VerifiedCreator: 2,
  /** 未签名或本地导入、自定义房间临时下载的包 */
  UntrustedEphemeral: 3,
} as const;

export type TrustLevel = (typeof TrustLevel)[keyof typeof TrustLevel];

export function isTrustLevel(v: unknown): v is TrustLevel {
  return v === 0 || v === 1 || v === 2 || v === 3;
}
