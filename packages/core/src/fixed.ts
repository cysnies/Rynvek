// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 确定性定点数算术（规范 02 §2.1 & Audit v1.1 §5 边界修复），与 Rust `rynvek_core::fixed` 位级一致。
 *
 * - `Q32_32` 以 `bigint` 保存 64 位原始值，先在无限精度 bigint（对应 Rust i128）完成移位与乘除，
 *   再提供 `checked*`（越界返回 `null`）与 `saturating*`（钳制至 `[I64_MIN, I64_MAX]`）语义；
 * - `Q8_8` 以 `number` 保存 16 位原始值（int32 运算足以容纳全部中间结果）。
 */

const FRACTION_BITS = 32n;
const I64_MIN = -(1n << 63n);
const I64_MAX = (1n << 63n) - 1n;
/** Newton-Raphson 平方根的固定迭代次数（与 Rust 实现一致） */
const SQRT_ITERATIONS = 8;

/** 饱和钳制到 i64 范围 */
function saturateI64(value: bigint): bigint {
  return value < I64_MIN ? I64_MIN : value > I64_MAX ? I64_MAX : value;
}

/** 检查 bigint 是否处于 i64 可表示范围内 */
function isWithinI64(value: bigint): boolean {
  return value >= I64_MIN && value <= I64_MAX;
}

/** 无符号整数的二进制位数（等价于 `128 - leading_zeros`） */
function bitLength(value: bigint): bigint {
  return BigInt(value.toString(2).length);
}

/** 64 位有符号定点数 Q32.32（整数 32 位，小数 32 位） */
export class Q32_32 {
  public static readonly ZERO = new Q32_32(0n);
  public static readonly ONE = new Q32_32(1n << 32n);
  public static readonly NEG_ONE = new Q32_32(-(1n << 32n));
  public static readonly HALF = new Q32_32(1n << 31n);
  public static readonly MIN = new Q32_32(I64_MIN);
  public static readonly MAX = new Q32_32(I64_MAX);
  /** floor(2^32 / 30) */
  public static readonly DT_30HZ = new Q32_32(143_165_576n);

  /** 原始 i64 值 */
  public readonly raw: bigint;

  private constructor(raw: bigint) {
    this.raw = raw;
  }

  /** 由原始 i64 值构造；超出 i64 范围时抛出 RangeError */
  public static fromRaw(raw: bigint): Q32_32 {
    if (!isWithinI64(raw)) {
      throw new RangeError(`Q32_32 原始值超出 i64 范围：${raw}`);
    }
    return new Q32_32(raw);
  }

  /** 由 32 位整数构造 */
  public static fromInt(value: number): Q32_32 {
    if (!Number.isInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff) {
      throw new RangeError(`fromInt 需要 i32 整数：${value}`);
    }
    return new Q32_32(BigInt(value) << FRACTION_BITS);
  }

  /** 由千分之一单位（milli-unit）受检构造，超出 Q32.32 范围时返回 null */
  public static checkedFromMilli(milli: bigint): Q32_32 | null {
    if (!isWithinI64(milli)) {
      return null;
    }
    const scaled = (milli << FRACTION_BITS) / 1000n;
    return isWithinI64(scaled) ? new Q32_32(scaled) : null;
  }

  /** 由千分之一单位（milli-unit）饱和构造，向零截断并在越界时钳制到 i64 边界 */
  public static saturatingFromMilli(milli: bigint): Q32_32 {
    const clampedMilli = saturateI64(milli);
    const scaled = (clampedMilli << FRACTION_BITS) / 1000n;
    return new Q32_32(saturateI64(scaled));
  }

  /** 由千分之一单位（milli-unit）构造（默认采用饱和语义，先在宽整数域左移再除以 1000，消除移位溢出） */
  public static fromMilli(milli: bigint): Q32_32 {
    return Q32_32.saturatingFromMilli(milli);
  }

  /** 向负无穷取整得到整数部分（i32） */
  public toIntFloor(): number {
    return Number(BigInt.asIntN(32, this.raw >> FRACTION_BITS));
  }

  /** 受检加法：越界返回 null */
  public checkedAdd(rhs: Q32_32): Q32_32 | null {
    const sum = this.raw + rhs.raw;
    return isWithinI64(sum) ? new Q32_32(sum) : null;
  }

  /** 受检减法：越界返回 null */
  public checkedSub(rhs: Q32_32): Q32_32 | null {
    const diff = this.raw - rhs.raw;
    return isWithinI64(diff) ? new Q32_32(diff) : null;
  }

  /** 饱和加法 */
  public saturatingAdd(rhs: Q32_32): Q32_32 {
    return new Q32_32(saturateI64(this.raw + rhs.raw));
  }

  /** 饱和减法 */
  public saturatingSub(rhs: Q32_32): Q32_32 {
    return new Q32_32(saturateI64(this.raw - rhs.raw));
  }

  /** 受检定点乘法：越界时返回 null */
  public checkedMul(rhs: Q32_32): Q32_32 | null {
    const prod = (this.raw * rhs.raw) >> FRACTION_BITS;
    return isWithinI64(prod) ? new Q32_32(prod) : null;
  }

  /** 饱和定点乘法：全精度乘积算术右移 32 位后饱和钳制为 i64 */
  public saturatingMul(rhs: Q32_32): Q32_32 {
    const prod = (this.raw * rhs.raw) >> FRACTION_BITS;
    return new Q32_32(saturateI64(prod));
  }

  /** 定点乘法（默认等价于 saturatingMul，与 Rust `Q32_32::mul` 完全一致） */
  public mul(rhs: Q32_32): Q32_32 {
    return this.saturatingMul(rhs);
  }

  /** 受检定点除法：除数为零或商超出 i64 范围时返回 null */
  public checkedDiv(rhs: Q32_32): Q32_32 | null {
    if (rhs.raw === 0n) {
      return null;
    }
    const quot = (this.raw << FRACTION_BITS) / rhs.raw;
    return isWithinI64(quot) ? new Q32_32(quot) : null;
  }

  /** 饱和定点除法：除数为零时返回 `fallback`，否则向零截断并饱和钳制到 i64 范围 */
  public saturatingDiv(rhs: Q32_32, fallback: Q32_32): Q32_32 {
    if (rhs.raw === 0n) {
      return fallback;
    }
    const quot = (this.raw << FRACTION_BITS) / rhs.raw;
    return new Q32_32(saturateI64(quot));
  }

  /** 安全除法：除数为零时返回 `fallback`，否则向零截断并饱和钳制 */
  public divSafe(rhs: Q32_32, fallback: Q32_32): Q32_32 {
    return this.saturatingDiv(rhs, fallback);
  }

  /** 确定性整数平方根（与 Rust `sqrt_deterministic` 逐步一致的 Newton-Raphson 迭代） */
  public sqrtDeterministic(): Q32_32 {
    if (this.raw <= 0n) {
      return Q32_32.ZERO;
    }
    const val = this.raw << FRACTION_BITS;
    let x = 1n << ((bitLength(val) + 1n) >> 1n);
    for (let i = 0; i < SQRT_ITERATIONS; i++) {
      x = (x + val / x) >> 1n;
    }
    const res = x * x > val ? x - 1n : x;
    return new Q32_32(saturateI64(res));
  }

  public equals(other: Q32_32): boolean {
    return this.raw === other.raw;
  }

  public compare(other: Q32_32): -1 | 0 | 1 {
    return this.raw < other.raw ? -1 : this.raw > other.raw ? 1 : 0;
  }

  public toString(): string {
    return `Q32_32(${this.raw})`;
  }
}

const I16_MIN = -0x8000;
const I16_MAX = 0x7fff;

function clamp(value: number, min: number, max: number): number {
  return value < min ? min : value > max ? max : value;
}

/** 16 位有符号定点数 Q8.8（整数 8 位，小数 8 位精度 1/256） */
export class Q8_8 {
  public static readonly ZERO = new Q8_8(0);
  public static readonly ONE = new Q8_8(256);
  public static readonly NEG_ONE = new Q8_8(-256);

  /** 原始 i16 值 */
  public readonly raw: number;

  private constructor(raw: number) {
    this.raw = raw;
  }

  /** 由原始 i16 值构造；不是 i16 整数时抛出 RangeError */
  public static fromRaw(raw: number): Q8_8 {
    if (!Number.isInteger(raw) || raw < I16_MIN || raw > I16_MAX) {
      throw new RangeError(`Q8_8 原始值超出 i16 范围：${raw}`);
    }
    return new Q8_8(raw);
  }

  /** 定点乘法：乘积右移 8 位后饱和钳制到 i16 */
  public mul(rhs: Q8_8): Q8_8 {
    // 两个 i16 的乘积不超过 2^30，JS 的 `>>` 在 int32 上执行算术右移，与 Rust 一致
    return new Q8_8(clamp((this.raw * rhs.raw) >> 8, I16_MIN, I16_MAX));
  }

  /** 硬双曲正切激活：钳制到 [-1.0, +1.0] */
  public hardTanh(): Q8_8 {
    return new Q8_8(clamp(this.raw, -256, 256));
  }

  public equals(other: Q8_8): boolean {
    return this.raw === other.raw;
  }

  public toString(): string {
    return `Q8_8(${this.raw})`;
  }
}
