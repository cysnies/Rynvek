// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 96 字节二进制能力令牌，与 Rust `rynvek_protocol::capability` 逐字节一致（Little-Endian）。
 *
 * 字节 `0..32` 为声明区，字节 `32..96` 为对声明区的 Ed25519 签名（本模块不做签名验证）。
 */

export const CAPABILITY_TOKEN_LEN = 96;
export const CAPABILITY_CLAIMS_LEN = 32;
export const ED25519_SIGNATURE_LEN = 64;
export const CAPABILITY_TOKEN_VERSION = 1;

/** 权限范围的点分名称（按位序号排列，下标即位序号） */
export const CAPABILITY_SCOPES = [
  "world.list",
  "world.create",
  "world.read",
  "world.fork",
  "world.edit",
  "world.connect",
  "match.play",
  "hub.publish",
  "lab.submit",
  "lab.read",
  "account.owner",
] as const;

export type CapabilityScope = (typeof CAPABILITY_SCOPES)[number];

/** 全部已定义权限位的掩码 */
export const CAPABILITY_SCOPES_KNOWN_MASK = (1n << BigInt(CAPABILITY_SCOPES.length)) - 1n;

/** 权限对应的位掩码 */
export function scopeBit(scope: CapabilityScope): bigint {
  return 1n << BigInt(CAPABILITY_SCOPES.indexOf(scope));
}

/** 由权限列表构造位集 */
export function scopeSetOf(scopes: readonly CapabilityScope[]): bigint {
  return scopes.reduce((bits, scope) => bits | scopeBit(scope), 0n);
}

/** 按位序号升序列出位集中的权限 */
export function scopesIn(bits: bigint): CapabilityScope[] {
  return CAPABILITY_SCOPES.filter((scope) => (bits & scopeBit(scope)) !== 0n);
}

/** 解码后的能力令牌 */
export interface CapabilityToken {
  readonly version: number;
  readonly signerKeyId: number;
  readonly nonce: number;
  readonly principalId: bigint;
  readonly scopes: bigint;
  readonly expiresAtUnixS: bigint;
  readonly signature: Uint8Array;
}

/** 令牌解码错误 */
export class CapabilityTokenError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "CapabilityTokenError";
  }
}

/** 被签名的 32 字节声明区 */
export function capabilityClaimsBytes(token: CapabilityToken): Uint8Array {
  const claims = new Uint8Array(CAPABILITY_CLAIMS_LEN);
  const view = new DataView(claims.buffer);
  view.setUint8(0, token.version);
  view.setUint8(1, 0); // 保留字节
  view.setUint16(2, token.signerKeyId, true);
  view.setUint32(4, token.nonce, true);
  view.setBigUint64(8, token.principalId, true);
  view.setBigUint64(16, token.scopes, true);
  view.setBigUint64(24, token.expiresAtUnixS, true);
  return claims;
}

/** 编码为 96 字节 */
export function encodeCapabilityToken(token: CapabilityToken): Uint8Array {
  if (token.signature.length !== ED25519_SIGNATURE_LEN) {
    throw new CapabilityTokenError(`签名必须为 64 字节，实际 ${token.signature.length}`);
  }
  const out = new Uint8Array(CAPABILITY_TOKEN_LEN);
  out.set(capabilityClaimsBytes(token), 0);
  out.set(token.signature, CAPABILITY_CLAIMS_LEN);
  return out;
}

/** 从恰好 96 字节解码并校验结构（不验证签名） */
export function decodeCapabilityToken(bytes: Uint8Array): CapabilityToken {
  if (bytes.length !== CAPABILITY_TOKEN_LEN) {
    throw new CapabilityTokenError(`令牌必须为 96 字节，实际 ${bytes.length}`);
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, CAPABILITY_TOKEN_LEN);
  const version = view.getUint8(0);
  if (version !== CAPABILITY_TOKEN_VERSION) {
    throw new CapabilityTokenError(`不支持的令牌格式版本 ${version}`);
  }
  const reserved = view.getUint8(1);
  if (reserved !== 0) {
    throw new CapabilityTokenError(`保留字节必须为 0，实际 ${reserved}`);
  }
  const scopes = view.getBigUint64(16, true);
  if ((scopes & ~CAPABILITY_SCOPES_KNOWN_MASK) !== 0n) {
    throw new CapabilityTokenError(`权限位集包含未定义位 0x${scopes.toString(16)}`);
  }
  return {
    version,
    signerKeyId: view.getUint16(2, true),
    nonce: view.getUint32(4, true),
    principalId: view.getBigUint64(8, true),
    scopes,
    expiresAtUnixS: view.getBigUint64(24, true),
    signature: bytes.slice(CAPABILITY_CLAIMS_LEN),
  };
}

/** 在 `nowUnixS` 时刻是否授予 `scope`（不含签名验证） */
export function capabilityGrants(token: CapabilityToken, scope: CapabilityScope, nowUnixS: bigint): boolean {
  return nowUnixS < token.expiresAtUnixS && (token.scopes & scopeBit(scope)) !== 0n;
}
