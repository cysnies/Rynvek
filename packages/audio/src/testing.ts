// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 可录制的 Web Audio 测试替身（`@rynvek/audio/testing`）。
//
// 无头环境（Node / jsdom）没有 AudioContext。这里实现合成器与混音器实际用到的最小子集，
// 把每个节点的创建、连线、启停以及 AudioParam 的自动化事件全部录下，
// 并按 Web Audio 规范的插值公式求任意时刻的参数值，供测试做确定性断言。

/** AudioParam 自动化事件 */
export type ParamEvent =
  | { readonly kind: "set"; readonly value: number; readonly time: number }
  | { readonly kind: "linear"; readonly value: number; readonly time: number }
  | { readonly kind: "exponential"; readonly value: number; readonly time: number }
  | { readonly kind: "target"; readonly value: number; readonly time: number; readonly timeConstant: number }
  | { readonly kind: "cancel"; readonly time: number; readonly value?: undefined };

/** 录制型 AudioParam */
export class FakeAudioParam {
  public readonly events: ParamEvent[] = [];
  private current: number;

  public constructor(public readonly defaultValue: number) {
    this.current = defaultValue;
  }

  /** 直接赋值语义与浏览器一致：等价于在当前时刻 set（这里仅记录最新静态值） */
  public get value(): number {
    return this.current;
  }

  public set value(v: number) {
    this.current = v;
  }

  public setValueAtTime(value: number, time: number): this {
    this.events.push({ kind: "set", value, time });
    return this;
  }

  public linearRampToValueAtTime(value: number, time: number): this {
    this.events.push({ kind: "linear", value, time });
    return this;
  }

  public exponentialRampToValueAtTime(value: number, time: number): this {
    // 浏览器对非正目标值抛 RangeError，替身保持同样的约束以暴露错误用法
    if (!(value > 0)) throw new RangeError(`exponentialRampToValueAtTime 目标值必须为正：${value}`);
    this.events.push({ kind: "exponential", value, time });
    return this;
  }

  public setTargetAtTime(value: number, time: number, timeConstant: number): this {
    this.events.push({ kind: "target", value, time, timeConstant });
    return this;
  }

  public cancelScheduledValues(time: number): this {
    // 删除 time 之后的所有事件，并记录一次取消便于断言
    for (let i = this.events.length - 1; i >= 0; i--) {
      const e = this.events[i];
      if (e !== undefined && e.kind !== "cancel" && e.time >= time) this.events.splice(i, 1);
    }
    this.events.push({ kind: "cancel", time });
    return this;
  }

  /**
   * 按 Web Audio 规范求 t 时刻的值：
   * 线性 / 指数斜坡从上一事件的（时间, 值）插值到本事件；setTarget 按指数逼近；首事件之前为静态值。
   */
  public valueAt(t: number): number {
    let value = this.current;
    let prevTime = 0;
    let target: { value: number; time: number; tc: number } | null = null;
    const evs = this.events.filter((e): e is Exclude<ParamEvent, { kind: "cancel" }> => e.kind !== "cancel");
    for (const e of evs) {
      if (target !== null) {
        // 进入下一个事件前，setTarget 的逼近值在该事件时间处被“冻结”为起点
        const end = Math.min(e.time, t);
        value = target.value + (value - target.value) * Math.exp(-(end - target.time) / target.tc);
        prevTime = end;
        target = null;
        if (t < e.time) return value;
      }
      if (e.kind === "set") {
        if (t < e.time) return value;
        value = e.value;
        prevTime = e.time;
      } else if (e.kind === "target") {
        if (t < e.time) return value;
        target = { value: e.value, time: e.time, tc: e.timeConstant };
        prevTime = e.time;
      } else {
        if (t < e.time) {
          const span = e.time - prevTime;
          if (span <= 0 || t < prevTime) return value;
          const k = (t - prevTime) / span;
          return e.kind === "linear"
            ? value + (e.value - value) * k
            : value * Math.pow(e.value / value, k);
        }
        value = e.value;
        prevTime = e.time;
      }
    }
    if (target !== null) {
      return target.value + (value - target.value) * Math.exp(-(t - target.time) / target.tc);
    }
    return value;
  }
}

/** 录制型节点基类 */
export class FakeAudioNode {
  public readonly outputs: FakeAudioNode[] = [];
  public disconnected = false;

  public constructor(
    public readonly context: FakeAudioContext,
    public readonly nodeType: string,
  ) {
    context.nodes.push(this);
  }

  public connect<T extends FakeAudioNode>(destination: T): T {
    this.outputs.push(destination);
    return destination;
  }

  public disconnect(): void {
    this.outputs.length = 0;
    this.disconnected = true;
  }
}

export class FakeGainNode extends FakeAudioNode {
  public readonly gain = new FakeAudioParam(1);
  public constructor(ctx: FakeAudioContext) {
    super(ctx, "gain");
  }
}

export class FakeStereoPannerNode extends FakeAudioNode {
  public readonly pan = new FakeAudioParam(0);
  public constructor(ctx: FakeAudioContext) {
    super(ctx, "stereoPanner");
  }
}

export class FakeBiquadFilterNode extends FakeAudioNode {
  public type: BiquadFilterType = "lowpass";
  public readonly frequency = new FakeAudioParam(350);
  public readonly Q = new FakeAudioParam(1);
  public readonly gain = new FakeAudioParam(0);
  public constructor(ctx: FakeAudioContext) {
    super(ctx, "biquadFilter");
  }
}

/** 可调度源节点：记录 start / stop 时刻 */
export class FakeScheduledSourceNode extends FakeAudioNode {
  public startTime: number | null = null;
  public stopTime: number | null = null;
  public onended: (() => void) | null = null;

  public start(when = 0): void {
    if (this.startTime !== null) throw new Error("InvalidStateError：源节点只能 start 一次");
    this.startTime = when;
  }

  public stop(when = 0): void {
    this.stopTime = when;
  }
}

export class FakeOscillatorNode extends FakeScheduledSourceNode {
  public type: OscillatorType = "sine";
  public readonly frequency = new FakeAudioParam(440);
  public readonly detune = new FakeAudioParam(0);
  public constructor(ctx: FakeAudioContext) {
    super(ctx, "oscillator");
  }
}

export class FakeAudioBufferSourceNode extends FakeScheduledSourceNode {
  public buffer: FakeAudioBuffer | null = null;
  public loop = false;
  public readonly playbackRate = new FakeAudioParam(1);
  public constructor(ctx: FakeAudioContext) {
    super(ctx, "bufferSource");
  }
}

export class FakeAudioBuffer {
  private readonly channels: Float32Array[];
  public constructor(
    public readonly numberOfChannels: number,
    public readonly length: number,
    public readonly sampleRate: number,
  ) {
    this.channels = Array.from({ length: numberOfChannels }, () => new Float32Array(length));
  }

  public get duration(): number {
    return this.length / this.sampleRate;
  }

  public getChannelData(channel: number): Float32Array {
    const data = this.channels[channel];
    if (data === undefined) throw new RangeError(`声道越界：${channel}`);
    return data;
  }
}

/** 录制型 AudioContext；`currentTime` 由测试手动推进 */
export class FakeAudioContext {
  public readonly nodes: FakeAudioNode[] = [];
  public readonly destination: FakeAudioNode;
  public currentTime = 0;
  public state: AudioContextState;

  public constructor(
    public readonly sampleRate = 48_000,
    initialState: AudioContextState = "running",
  ) {
    this.state = initialState;
    this.destination = new FakeAudioNode(this, "destination");
  }

  public createGain(): FakeGainNode {
    return new FakeGainNode(this);
  }

  public createStereoPanner(): FakeStereoPannerNode {
    return new FakeStereoPannerNode(this);
  }

  public createBiquadFilter(): FakeBiquadFilterNode {
    return new FakeBiquadFilterNode(this);
  }

  public createOscillator(): FakeOscillatorNode {
    return new FakeOscillatorNode(this);
  }

  public createBufferSource(): FakeAudioBufferSourceNode {
    return new FakeAudioBufferSourceNode(this);
  }

  public createBuffer(channels: number, length: number, sampleRate: number): FakeAudioBuffer {
    return new FakeAudioBuffer(channels, length, sampleRate);
  }

  public resume(): Promise<void> {
    this.state = "running";
    return Promise.resolve();
  }

  public suspend(): Promise<void> {
    this.state = "suspended";
    return Promise.resolve();
  }

  /** 以浏览器类型视图交给被测代码 */
  public asAudioContext(): AudioContext {
    return this as unknown as AudioContext;
  }

  /** 按节点类型筛选已创建节点 */
  public nodesOfType<T extends FakeAudioNode>(nodeType: string): T[] {
    return this.nodes.filter((n) => n.nodeType === nodeType) as T[];
  }
}
