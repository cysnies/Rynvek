// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 无状态计数器式确定性随机数 `Philox4x32-10`（规范 02 §2.2）。
 *
 * 与 Rust `rynvek_core::rng` 及 WGSL `philox_rng.wgsl` 位级对齐：
 * 32 位乘法拆成 16 位半字计算（与 WGSL 实现逐式相同），避免超出双精度的精确整数范围。
 */

export type U32x4 = readonly [number, number, number, number];
export type U32x2 = readonly [number, number];

export const PHILOX_M0 = 0xd2511f53;
export const PHILOX_M1 = 0xcd9e8d57;
export const PHILOX_W0 = 0x9e3779b9;
export const PHILOX_W1 = 0xbb67ae85;
export const PHILOX_ROUNDS = 10;

const LOW16 = 0xffff;

/** 32 位无符号乘法，返回 `[lo, hi]` */
export function mulHiLoU32(a: number, b: number): [number, number] {
  const aLo = a & LOW16;
  const aHi = a >>> 16;
  const bLo = b & LOW16;
  const bHi = b >>> 16;

  const p0 = aLo * bLo;
  const p1 = aLo * bHi;
  const p2 = aHi * bLo;
  const p3 = aHi * bHi;

  const mid = (p0 >>> 16) + (p1 & LOW16) + (p2 & LOW16);
  const lo = ((p0 & LOW16) | ((mid & LOW16) << 16)) >>> 0;
  const hi = (p3 + (p1 >>> 16) + (p2 >>> 16) + (mid >>> 16)) >>> 0;
  return [lo, hi];
}

/** Philox4x32-10 分组函数 */
export function philox4x32_10(ctrIn: U32x4, keyIn: U32x2): U32x4 {
  let [c0, c1, c2, c3] = ctrIn;
  let [k0, k1] = keyIn;
  for (let round = 0; round < PHILOX_ROUNDS; round++) {
    const [lo0, hi0] = mulHiLoU32(PHILOX_M0, c0);
    const [lo1, hi1] = mulHiLoU32(PHILOX_M1, c2);
    [c0, c1, c2, c3] = [(hi1 ^ c1 ^ k0) >>> 0, lo1, (hi0 ^ c3 ^ k1) >>> 0, lo0];
    k0 = (k0 + PHILOX_W0) >>> 0;
    k1 = (k1 + PHILOX_W1) >>> 0;
  }
  return [c0, c1, c2, c3];
}

/** 随机数完整寻址键（字段语义与 Rust `RandomKey` 相同） */
export interface RandomKey {
  readonly worldSeed: bigint;
  readonly tick: bigint;
  readonly entitySerial: bigint;
  /** 高 16 位为随机流 ID，低 16 位为阶段 ID */
  readonly streamAndPhase: number;
  readonly subCounter: number;
}

/** 将随机流 ID 与阶段 ID 打包为 `streamAndPhase` */
export function packStreamAndPhase(streamId: number, phaseId: number): number {
  return (((streamId & LOW16) << 16) | (phaseId & LOW16)) >>> 0;
}

function lo32(value: bigint): number {
  return Number(BigInt.asUintN(32, value));
}

function hi32(value: bigint): number {
  return Number(BigInt.asUintN(32, BigInt.asUintN(64, value) >> 32n));
}

/** Philox 计数器字：`[sub_counter, stream_and_phase, lo32(entity_serial), hi32(entity_serial)]` */
export function randomKeyCounter(rk: RandomKey): U32x4 {
  return [rk.subCounter >>> 0, rk.streamAndPhase >>> 0, lo32(rk.entitySerial), hi32(rk.entitySerial)];
}

/** Philox 密钥字：`[lo32(world_seed ^ tick), hi32(world_seed ^ tick)]` */
export function randomKeyKey(rk: RandomKey): U32x2 {
  const mixed = BigInt.asUintN(64, rk.worldSeed) ^ BigInt.asUintN(64, rk.tick);
  return [lo32(mixed), hi32(mixed)];
}

/** 生成 4 个独立均匀分布的 32 位随机数 */
export function generateU32x4(rk: RandomKey): U32x4 {
  return philox4x32_10(randomKeyCounter(rk), randomKeyKey(rk));
}
