// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// L4 突触检视的时间序列：膜电位、权重 w_ji 与资格迹 e_ji 的定长环形缓冲与按像素降采样。
//
// 降采样采用逐桶 min/max（而非平均或 LTTB）：单个尖峰在任意缩放下都不会被抹掉，
// 且结果只依赖输入与桶数，确定可复现（ERRATA E-020）。

/** 定长环形时间序列（按 Tick 追加，满后覆盖最旧样本） */
export class RingSeries {
  private readonly data: Float64Array;
  private start = 0;
  private count = 0;
  /** 最新样本对应的 Tick；空序列为 null */
  private lastTick: number | null = null;

  public constructor(public readonly capacity: number) {
    if (!Number.isInteger(capacity) || capacity <= 0) throw new RangeError("容量必须是正整数");
    this.data = new Float64Array(capacity);
  }

  public get length(): number {
    return this.count;
  }

  /** 最旧样本的 Tick */
  public get firstTick(): number | null {
    return this.lastTick === null ? null : this.lastTick - this.count + 1;
  }

  /** 追加一个 Tick 的样本；Tick 必须连续递增（跳号时以 NaN 补洞，表示无数据） */
  public push(tick: number, value: number): void {
    if (this.lastTick !== null) {
      if (tick <= this.lastTick) throw new RangeError(`Tick 必须递增：${tick} ≤ ${this.lastTick}`);
      const gap = Math.min(tick - this.lastTick - 1, this.capacity);
      for (let i = 0; i < gap; i++) this.append(Number.NaN);
    }
    this.append(value);
    this.lastTick = tick;
  }

  /** 第 i 个样本（0 为最旧） */
  public at(i: number): number {
    if (i < 0 || i >= this.count) return Number.NaN;
    return this.data[(this.start + i) % this.capacity] ?? Number.NaN;
  }

  public toArray(): Float64Array {
    const out = new Float64Array(this.count);
    for (let i = 0; i < this.count; i++) out[i] = this.at(i);
    return out;
  }

  public clear(): void {
    this.start = 0;
    this.count = 0;
    this.lastTick = null;
  }

  private append(v: number): void {
    if (this.count < this.capacity) {
      this.data[(this.start + this.count) % this.capacity] = v;
      this.count++;
    } else {
      this.data[this.start] = v;
      this.start = (this.start + 1) % this.capacity;
    }
  }
}

export interface MinMaxBuckets {
  readonly min: Float64Array;
  readonly max: Float64Array;
}

/**
 * 把序列切成 `buckets` 个等宽桶并取每桶极值；
 * 桶 i 覆盖下标 [floor(i·n/b), floor((i+1)·n/b))；NaN 被跳过，全为 NaN 的桶输出 NaN。
 * 样本数不足桶数时每个样本独占一桶（桶数收缩为样本数）。
 */
export function downsampleMinMax(samples: ArrayLike<number>, buckets: number): MinMaxBuckets {
  const n = samples.length;
  const b = Math.max(0, Math.min(Math.floor(buckets), n));
  const min = new Float64Array(b);
  const max = new Float64Array(b);
  for (let i = 0; i < b; i++) {
    const lo = Math.floor((i * n) / b);
    const hi = Math.floor(((i + 1) * n) / b);
    let mn = Infinity;
    let mx = -Infinity;
    for (let j = lo; j < hi; j++) {
      const v = samples[j] ?? Number.NaN;
      if (Number.isNaN(v)) continue;
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
    min[i] = mn === Infinity ? Number.NaN : mn;
    max[i] = mx === -Infinity ? Number.NaN : mx;
  }
  return { min, max };
}

/**
 * 把 min/max 桶转为 SVG 路径（每桶一条竖线，x 取像素中心），y 轴向下。
 * NaN 桶断开路径；坐标保留两位小数，保证快照测试稳定。
 */
export function minMaxToSvgPath(b: MinMaxBuckets, width: number, height: number, yMin: number, yMax: number): string {
  const n = b.min.length;
  if (n === 0 || yMax <= yMin) return "";
  const fx = (i: number) => (((i + 0.5) * width) / n).toFixed(2);
  const fy = (v: number) => ((1 - (Math.min(yMax, Math.max(yMin, v)) - yMin) / (yMax - yMin)) * height).toFixed(2);
  const parts: string[] = [];
  for (let i = 0; i < n; i++) {
    const lo = b.min[i] ?? Number.NaN;
    const hi = b.max[i] ?? Number.NaN;
    if (Number.isNaN(lo) || Number.isNaN(hi)) continue;
    parts.push(`M${fx(i)} ${fy(hi)}V${fy(lo)}`);
  }
  return parts.join("");
}
