// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { AudioDirector, AudioMixer, NeuralStateSonifier, parseSoundBank, type SoundBank, type NeuralSoundLevels } from "@rynvek/audio";
import type { ArenaClient, FactionObservation, RtsEvent } from "@rynvek/client";

export type ArenaAudioState = "off" | "enabling" | "running" | "muted" | "suspended" | "error" | "ended";
export const AUDIO_STALE_MS = 3000;
export interface ArenaAudioStatus { readonly state: ArenaAudioState; readonly bound: boolean; readonly ready: boolean; readonly neural: boolean; readonly message: string }
export interface AudioFramePlan { readonly events: readonly string[]; readonly neural: NeuralSoundLevels }

/** 只接原已授权阵营投影。即使误送敌方事件也再次拒绝；不读ghost/全世界遥测，不以消失推断死亡。 */
export function planArenaAudio(obs: FactionObservation, events: readonly RtsEvent[], previous: FactionObservation | null): AudioFramePlan {
  const n = obs.neural;
  if (!n || n.sample_tick !== obs.tick || n.spike_window_substeps !== 64 ||
    ![obs.tick, obs.faction, n.spiking_neurons, n.continuous_neurons, n.spikes_in_window, n.continuous_abs_mean_q8].every(v => Number.isSafeInteger(v) && v >= 0) ||
    obs.faction > 7 || n.continuous_abs_mean_q8 > 32768 || n.spiking_neurons > Math.floor(Number.MAX_SAFE_INTEGER / 64) || n.spikes_in_window > n.spiking_neurons * 64 ||
    (n.continuous_neurons === 0 && n.continuous_abs_mean_q8 !== 0)) throw new Error("当前阵营神经投影非法；不猜测缺失指标");
  const cues: string[] = [];
  const nexus = obs.buildings.find(b => b.faction === obs.faction && b.kind === "bld.nexus_core");
  const oldNexus = previous?.buildings.find(b => b.id === nexus?.id && b.faction === obs.faction);
  // 生命下降告警，不声称已识别炮火/伤害原因。
  if (nexus && oldNexus && nexus.hp_milli < oldNexus.hp_milli) cues.push("alert.nexus_under_attack");
  for (const event of events.slice(0, 256)) {
    if (!("faction" in event) || event.faction !== obs.faction) continue;
    switch (event.type) {
      case "building_placed": cues.push("building.placed"); break;
      case "building_completed": cues.push("building.completed"); break;
      case "building_destroyed": cues.push("building.destroyed"); break;
      case "unit_spawned": cues.push("unit.spawned"); break;
      // 复用死亡提示音色，不伪造一条权威爆炸或武器发射事件。
      case "unit_died": cues.push("combat.explosion_large"); break;
      case "research_completed": cues.push("ui.confirm"); break;
      case "genome_bred": cues.push("neural.mutation_epoch"); break;
    }
  }
  return { events: [...new Set(cues)].slice(0, 16), neural: {
    ringDensity: n.spiking_neurons ? n.spikes_in_window / (n.spiking_neurons * 64) : 0,
    continuousLevel: Math.min(1, n.continuous_abs_mean_q8 / 256),
  }};
}

interface Graph { readonly ctx: AudioContext; readonly mixer: AudioMixer; readonly director: AudioDirector; sonifier: NeuralStateSonifier | null }
/** AudioContext只在enableFromGesture的同步段创建/请求resume。无自动启用、缓存补播、外部音源或凭据。 */
export class ArenaAudioSession {
  private source: ArenaClient | null = null;
  private bank: SoundBank | null = null;
  private bankLabel = "内置24配方";
  private unsubscribe: (() => void) | null = null;
  private graph: Graph | null = null;
  private previous: FactionObservation | null = null;
  private epoch = 0;
  private receivedAt = -Infinity;
  private stale: ReturnType<typeof setTimeout> | null = null;
  private mode: ArenaAudioState = "off";
  private message = "默认关闭；连接后需点击启用音频。";
  private neural = false;
  private volume = .5;
  constructor(private readonly changed: (status: ArenaAudioStatus) => void,
    private readonly createContext: () => AudioContext = () => new AudioContext({latencyHint:"interactive"})) {}
  get status(): ArenaAudioStatus { return {state:this.mode,bound:this.source !== null,ready:!!this.source && !!this.previous && !this.previous.outcome && Date.now() - this.receivedAt < AUDIO_STALE_MS,neural:this.neural,message:this.message}; }
  private notify(): void { this.changed(this.status); }
  bind(source: ArenaClient): void {
    this.detach(); this.source = source;
    this.unsubscribe = source.on(event => {
      if (source !== this.source) return;
      if (event.type === "error") { this.detach("服务端拒绝，音频已停止；重连后重新启用。"); return; }
      if (event.type !== "observation") return;
      try {
        const obs = event.observation;
        if (obs.faction !== source.identity.faction) throw new Error("阵营不符");
        if (this.previous && obs.tick < this.previous.tick) throw new Error("实时Tick回退");
        const plan = planArenaAudio(obs, event.events, this.previous);
        // 投降等可在同Tick产生真实终局：先停止，再对普通重复观察去重。
        if (obs.outcome) { this.previous = obs; this.stopGraph(); this.mode = "ended"; this.message = "已终局，音频停止；不重放历史提示。"; this.notify(); return; }
        if (this.previous && obs.tick === this.previous.tick) return; // 观察性音频，重复Tick不重放。
        this.previous = obs; this.receivedAt = Date.now();
        const g = this.graph;
        if (!g || this.mode !== "running") { this.notify(); return; }
        for (const cue of plan.events) g.director.play(cue, {intensity:.65});
        if (this.neural) {
          g.sonifier ??= new NeuralStateSonifier(g.ctx, g.mixer.bus("neural_sonify"));
          g.sonifier.update(plan.neural);
        }
        this.armStale(); this.notify();
      } catch { this.stopGraph(); this.mode = "error"; this.message = "音频投影拒绝，已静音；不影响权威仿真。"; this.notify(); }
    });
    this.message = "已绑定原实时阵营流；点击启用，历史事件不补播。"; this.notify();
  }
  async enableFromGesture(): Promise<void> {
    if (!this.status.ready || this.mode === "enabling" || this.mode === "running") { this.notify(); return; }
    this.stopGraph(); const own = this.epoch; this.mode = "enabling"; this.notify();
    let ctx: AudioContext | null = null;
    try {
      ctx = this.createContext(); const mixer = new AudioMixer(ctx, {masterGain:this.volume});
      const graph: Graph = {ctx,mixer,director:new AudioDirector(ctx,{mixer,...(this.bank?{manifest:this.bank.manifest,recipes:this.bank.recipes}:{})}),sonifier:null}; this.graph = graph;
      // 在实际按钮手势栈中调用resume，await之前无网络/计时器；浏览器策略仍最终决定。
      const resumed = ctx.state === "running" ? Promise.resolve() : ctx.resume();
      await resumed;
      if (own !== this.epoch || this.graph !== graph) { if (ctx.state !== "closed") void ctx.close().catch(() => {}); return; }
      if (ctx.state !== "running") throw new Error("未解锁");
      ctx.onstatechange = () => { if (this.graph === graph && ctx?.state !== "running") this.suspend("音频上下文已挂起；需再次手势启用。"); };
      this.mode = "running"; this.message = `音频已启用 / ${this.bankLabel}；仅新本人阵营事件/真实脑态。设备未听验。`;
      this.armStale(); this.notify();
    } catch {
      if (own !== this.epoch) return;
      this.stopGraph(); if (ctx && ctx.state !== "closed") void ctx.close().catch(() => {});
      this.mode = "error"; this.message = "音频不可用或手势解锁未获准；可再次点击，不绕策略。"; this.notify();
    }
  }
  setSoundBank(manifest:unknown,recipes:unknown,label:string):void {
    if(!this.source)throw new Error("先连接实际Arena阵营流");
    const bank=parseSoundBank(manifest,recipes);this.stopGraph();this.bank=bank;this.bankLabel=label.slice(0,180);this.mode="off";this.message=`已验证并选择 ${this.bankLabel}；需再次手势启用，不补播。`;this.notify();
  }
  clearSoundBank():void {this.stopGraph();this.bank=null;this.bankLabel="内置24配方";this.mode="off";this.message="已恢复内置配方；需手势重新启用。";this.notify();}
  mute(): void { this.stopGraph(); this.mode = "muted"; this.message = "已全部静音并切断旧图；重新启用需手势，不补播。"; this.notify(); }
  suspend(message = "页面隐藏/失焦，已停止音频；返回后需再次点击启用。"): void { this.stopGraph(); this.mode = "suspended"; this.message = message; this.notify(); }
  setVolume(value: number): void { this.volume = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0; this.graph?.mixer.setMasterGain(this.volume); }
  setNeural(enabled: boolean): void {
    this.neural = enabled;
    this.graph?.sonifier?.stop(); if (this.graph) this.graph.sonifier = null;
    this.notify(); // 仅后续真实观测更新声化，不用旧态造一次spike。
  }
  detach(message = "未绑定实时席位；音频关闭。换世界/身份需重新启用。"): void {
    this.stopGraph(); this.unsubscribe?.(); this.unsubscribe = null; this.source = null; this.previous = null; this.receivedAt = -Infinity;
    this.bank=null;this.bankLabel="内置24配方";
    this.mode = "off"; this.message = message; this.notify();
  }
  private armStale(): void {
    if (this.stale) clearTimeout(this.stale);
    this.stale = setTimeout(() => this.suspend("超过3秒没有新权威Tick，音频已停止；待新观测后手势启用。"), AUDIO_STALE_MS);
  }
  private stopGraph(): void {
    this.epoch++; if (this.stale) clearTimeout(this.stale); this.stale = null;
    const graph = this.graph; this.graph = null;
    if (!graph) return;
    graph.ctx.onstatechange = null; graph.mixer.disconnect(); graph.sonifier?.stop();
    if (graph.ctx.state !== "closed") void graph.ctx.close().catch(() => {});
  }
}
