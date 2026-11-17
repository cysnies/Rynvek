// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 创作态文档（AuthoringDocument）：多创作者 `WorldPatch` 的 CRDT 合并。
//
// 项目文档 GAP-25 指向的“规范 10.4 AuthoringDocument / 三方语义合并器”在开发规范中缺失，
// 这里按最佳实践补全（ERRATA E-021）：
//   - 文档 = 只读基线 + 已整合补丁集合（按 patch_id 去重的 G-Set）；
//   - 每个可寻址的“寄存器”（世界参数键、地形格、基因组 slug、规则模块 ID、实体模板 ID）是一个 LWW 寄存器，
//     写入戳 = (HLC, patch_id, 操作下标)，戳大者胜；寄存器保留按戳排序的写入历史，用于选择性撤销；
//   - 因而整合满足交换律、结合律与幂等性：任意副本只要收到相同的补丁集合，状态逐位相同（强最终一致）。
//   - `SetWorldParam.new_value_json == "null"` 表示删除该键（JSON null 即“未设置”）。

import { blake3Digest, toHex } from "@rynvek/assets";
import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import type { HybridLogicalClock } from "./hlc.js";
import {
  compareHlc,
  opKind,
  parseWorldPatch,
  patchIdHex,
  type HlcTimestamp,
  type WorldPatch,
  type WorldPatchOp,
} from "./patch.js";
import { isSafetyClassSufficient, requiredSafetyClass, safetyClassName } from "./safety.js";

export interface RuleModuleSource {
  readonly rule_graph_json: string;
  readonly compiled_rir_blake3: readonly number[];
}

/** 文档基线（通常来自已发布的 `.rworld`） */
export interface AuthoringBaseline {
  readonly width: number;
  readonly height: number;
  /** 行主序地形（`TerrainTileKind` 数值），长度 width × height */
  readonly terrain: Uint8Array;
  readonly params?: Readonly<Record<string, string>>;
  readonly genomes?: Readonly<Record<string, string>>;
  readonly rules?: Readonly<Record<string, RuleModuleSource>>;
  readonly templates?: readonly string[];
}

/** 写入戳 */
interface Stamp {
  readonly hlc: HlcTimestamp;
  readonly patch: string;
  readonly op: number;
}

interface Write<T> {
  readonly stamp: Stamp;
  readonly value: T;
}

export type RegisterRef =
  | { readonly kind: "param"; readonly key: string }
  | { readonly kind: "tile"; readonly x: number; readonly y: number }
  | { readonly kind: "genome"; readonly slug: string }
  | { readonly kind: "rule"; readonly moduleId: string }
  | { readonly kind: "template"; readonly templateId: string };

/** 同一寄存器上不同作者的写入相互覆盖（供 UI 提示冲突） */
export interface WriteConflict {
  readonly register: RegisterRef;
  readonly winner: string;
  readonly loser: string;
}

export interface IntegrateResult {
  /** false 表示该补丁已整合过（幂等） */
  readonly applied: boolean;
  readonly conflicts: readonly WriteConflict[];
}

export interface InverseResult {
  readonly operations: WorldPatchOp[];
  /** 规范操作集无法表达逆操作、或已被他人更新覆盖而跳过的寄存器 */
  readonly skipped: readonly { readonly register: RegisterRef; readonly reason: "superseded" | "not_invertible" }[];
}

function compareStamp(a: Stamp, b: Stamp): number {
  return compareHlc(a.hlc, b.hlc) || (a.patch < b.patch ? -1 : a.patch > b.patch ? 1 : 0) || a.op - b.op;
}

function rejectPatch(detail: string): never {
  throw new RynvekError(RynvekErrorCode.SimulationInvariantViolated, `WorldPatch: ${detail}`);
}

/** 按戳有序插入，返回插入位置 */
function insertWrite<T>(history: Write<T>[], w: Write<T>): number {
  let i = history.length;
  while (i > 0 && compareStamp((history[i - 1] as Write<T>).stamp, w.stamp) > 0) i--;
  history.splice(i, 0, w);
  return i;
}

export class AuthoringDocument {
  public readonly width: number;
  public readonly height: number;
  private readonly baseTerrain: Uint8Array;
  private readonly baseParams: ReadonlyMap<string, string>;
  private readonly baseGenomes: ReadonlyMap<string, string>;
  private readonly baseRules: ReadonlyMap<string, RuleModuleSource>;
  private readonly baseTemplates: ReadonlySet<string>;

  private revisionOffset = 0;
  private readonly catalogue = new Map<string, Write<string | undefined>[]>();
  private readonly patches = new Map<string, WorldPatch>();
  private readonly params = new Map<string, Write<string>[]>();
  private readonly tiles = new Map<number, Write<number>[]>();
  private readonly rules = new Map<string, Write<RuleModuleSource>[]>();
  private readonly removals = new Map<string, Write<true>[]>();

  public constructor(base: AuthoringBaseline) {
    if (!Number.isInteger(base.width) || !Number.isInteger(base.height) || base.width <= 0 || base.height <= 0) {
      throw new RangeError("地形尺寸必须为正整数");
    }
    if (base.terrain.length !== base.width * base.height) throw new RangeError("地形数组长度与尺寸不符");
    this.width = base.width;
    this.height = base.height;
    this.baseTerrain = base.terrain.slice();
    this.baseParams = new Map(Object.entries(base.params ?? {}));
    this.baseGenomes = new Map(Object.entries(base.genomes ?? {}));
    this.baseRules = new Map(Object.entries(base.rules ?? {}));
    this.baseTemplates = new Set(base.templates ?? []);
  }

  /** 已整合补丁数（单调递增的创作修订号，写入新补丁的 `base_authoring_revision`） */
  public get revision(): number {
    return this.revisionOffset + this.patches.size;
  }

  /** 仅由真实服务器提交回执校准；Tick提交也占权威修订，不能用本地补丁数冒充。 */
  public acknowledgeRevision(revision:number):void {
    if(!Number.isSafeInteger(revision)||revision<this.revision)throw new RangeError("权威修订必须安全且单调");
    this.revisionOffset=revision-this.patches.size;
  }

  public hasPatch(patchId: readonly number[] | string): boolean {
    return this.patches.has(typeof patchId === "string" ? patchId : patchIdHex(patchId));
  }

  /** 全部已整合补丁，按 HLC 全序 */
  public allPatches(): WorldPatch[] {
    return [...this.patches.values()].sort((a, b) =>
      compareStamp({ hlc: a.hlc, patch: patchIdHex(a.patch_id), op: 0 }, { hlc: b.hlc, patch: patchIdHex(b.patch_id), op: 0 }),
    );
  }

  /** 整合一个（本地或远端）补丁 */
  public integrate(input: WorldPatch): IntegrateResult {
    const patch = parseWorldPatch(input);
    const id = patchIdHex(patch.patch_id);
    if (this.patches.has(id)) return { applied: false, conflicts: [] };
    if (!isSafetyClassSufficient(patch)) {
      rejectPatch(`声明的 ${patch.safety_class} 低于所需的 ${safetyClassName(requiredSafetyClass(patch.operations))}`);
    }
    // 先整体校验，保证补丁原子整合
    for (const op of patch.operations) this.checkBounds(op);

    const conflicts: WriteConflict[] = [];
    const record = <T>(history: Write<T>[], write: Write<T>, register: RegisterRef): void => {
      const at = insertWrite(history, write);
      const author = write.stamp.hlc.author_node_id;
      const isLast = at === history.length - 1;
      const neighbour = isLast ? history[at - 1] : history[history.length - 1];
      if (neighbour !== undefined && neighbour.stamp.hlc.author_node_id !== author && neighbour.stamp.patch !== id) {
        conflicts.push(
          isLast
            ? { register, winner: id, loser: neighbour.stamp.patch }
            : { register, winner: neighbour.stamp.patch, loser: id },
        );
      }
    };

    patch.operations.forEach((op, index) => {
      const stamp: Stamp = { hlc: patch.hlc, patch: id, op: index };
      if ("SetWorldParam" in op) {
        const { key_path, new_value_json } = op.SetWorldParam;
        record(this.history(this.params, key_path), { stamp, value: new_value_json }, { kind: "param", key: key_path });
      } else if ("PaintTerrainRect" in op) {
        const { x0, y0, width, height, tile_kind } = op.PaintTerrainRect;
        let tileConflicts = 0;
        for (let y = y0; y < y0 + height; y++) {
          for (let x = x0; x < x0 + width; x++) {
            const before = conflicts.length;
            record(this.history(this.tiles, y * this.width + x), { stamp, value: tile_kind }, { kind: "tile", x, y });
            // 大面积涂刷只保留前 64 条格级冲突，避免冲突列表爆炸
            if (conflicts.length > before && ++tileConflicts > 64) conflicts.pop();
          }
        }
      } else if ("UpsertSeedGenome" in op) {
        const { genome_slug, genome_bytes_b64 } = op.UpsertSeedGenome;
        record(this.history(this.catalogue, genome_slug), { stamp, value: genome_bytes_b64 }, { kind: "genome", slug: genome_slug });
      } else if ("UpdateRuleModule" in op) {
        const { module_id, rule_graph_json, compiled_rir_blake3 } = op.UpdateRuleModule;
        record(this.history(this.rules, module_id), { stamp, value: { rule_graph_json, compiled_rir_blake3 } }, { kind: "rule", moduleId: module_id });
      } else {
        const { template_id } = op.RemoveEntityTemplate;
        if(template_id.startsWith("genome/"))record(this.history(this.catalogue,template_id.slice(7)),{stamp,value:undefined},{kind:"template",templateId:template_id});
        if(!template_id.startsWith("genome/"))record(this.history(this.removals, template_id), { stamp, value: true }, { kind: "template", templateId: template_id });
      }
    });
    this.patches.set(id, patch);
    return { applied: true, conflicts };
  }

  /** 整合一批补丁（顺序无关） */
  public integrateAll(patches: Iterable<WorldPatch>): WriteConflict[] {
    const out: WriteConflict[] = [];
    for (const p of patches) out.push(...this.integrate(p).conflicts);
    return out;
  }

  // ---- 物化视图 ----

  /** 世界参数的 JSON 值；未设置（或被设为 null）返回 undefined */
  public param(key: string): string | undefined {
    const v = this.params.get(key)?.at(-1)?.value ?? this.baseParams.get(key);
    return v === undefined || v === "null" ? undefined : v;
  }

  public tileAt(x: number, y: number): number {
    if (!this.inGrid(x, y)) throw new RangeError(`坐标越界：(${x}, ${y})`);
    const i = y * this.width + x;
    return this.tiles.get(i)?.at(-1)?.value ?? (this.baseTerrain[i] as number);
  }

  public genome(slug: string): string | undefined {
    const writes=this.catalogue.get(slug);
    return writes?.length ? writes.at(-1)?.value : this.baseGenomes.get(slug);
  }

  public ruleModule(id: string): RuleModuleSource | undefined {
    return this.rules.get(id)?.at(-1)?.value ?? this.baseRules.get(id);
  }

  /** 模板只能删除不能新建：存在于基线且没有删除记录 */
  public hasTemplate(id: string): boolean {
    return id.startsWith("genome/") ? this.genome(id.slice(7))!==undefined : this.baseTemplates.has(id) && !this.removals.has(id);
  }

  /** 物化地形 */
  public terrain(): Uint8Array {
    const out = this.baseTerrain.slice();
    for (const [i, h] of this.tiles) out[i] = (h.at(-1) as Write<number>).value;
    return out;
  }

  /** 物化状态的规范 JSON（键排序），用于收敛比较与持久化 */
  public canonicalState(): string {
    const sortObj = <T>(entries: Iterable<[string, T]>) =>
      Object.fromEntries([...entries].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
    const keys = (a: Iterable<string>, b: Iterable<string>) => [...new Set([...a, ...b])];
    const params = keys(this.baseParams.keys(), this.params.keys())
      .map((k): [string, string | undefined] => [k, this.param(k)])
      .filter((e): e is [string, string] => e[1] !== undefined);
    const genomes = keys(this.baseGenomes.keys(), this.catalogue.keys()).map((k): [string, string | undefined] => [k, this.genome(k)]).filter((e):e is [string,string]=>e[1]!==undefined);
    const rules = keys(this.baseRules.keys(), this.rules.keys()).map((k): [string, RuleModuleSource] => [k, this.ruleModule(k) as RuleModuleSource]);
    const templates = [...new Set([...this.baseTemplates,...keys(this.baseGenomes.keys(),this.catalogue.keys()).map(k=>`genome/${k}`)])].filter((t) => this.hasTemplate(t)).sort();
    return JSON.stringify({
      width: this.width,
      height: this.height,
      terrain_blake3: toHex(blake3Digest(this.terrain())),
      params: sortObj(params),
      genomes: sortObj(genomes),
      rules: sortObj(rules),
      templates,
    });
  }

  /** 物化状态的 BLAKE3（十六进制） */
  public stateHash(): string {
    return toHex(blake3Digest(new TextEncoder().encode(this.canonicalState())));
  }

  // ---- 选择性撤销 ----

  /**
   * 计算撤销某补丁所需的逆操作：只回退“当前胜出者仍是该补丁”的寄存器，
   * 已被后续写入覆盖的寄存器跳过（不覆盖他人的新修改）。
   */
  public inverseOf(patchId: readonly number[] | string): InverseResult {
    const id = typeof patchId === "string" ? patchId : patchIdHex(patchId);
    const patch = this.patches.get(id);
    if (patch === undefined) throw new RangeError(`未知补丁 ${id}`);
    const ops: WorldPatchOp[] = [];
    const skipped: { register: RegisterRef; reason: "superseded" | "not_invertible" }[] = [];

    /** 胜出者是本补丁时，返回其之前的写入值（不存在为 undefined）；否则返回 null */
    const previous = <T>(history: Write<T>[] | undefined): { prev: T | undefined; hadPrevious: boolean } | null => {
      if (history === undefined) return null;
      const last = history.at(-1);
      if (last === undefined || last.stamp.patch !== id) return null;
      // 同一补丁内多次写同一寄存器时，跳过本补丁的全部写入
      let j = history.length - 1;
      while (j >= 0 && (history[j] as Write<T>).stamp.patch === id) j--;
      return { prev: j >= 0 ? (history[j] as Write<T>).value : undefined, hadPrevious:j>=0 };
    };

    const seenCatalogue=new Set<string>();
    const invertCatalogue=(slug:string)=>{
      if(seenCatalogue.has(slug))return;seenCatalogue.add(slug);
      const r=previous(this.catalogue.get(slug));const prev=r?.hadPrevious?r.prev:this.baseGenomes.get(slug);
      if(r===null)skipped.push({register:{kind:"genome",slug},reason:"superseded"});
      else if(prev!==undefined)ops.push({UpsertSeedGenome:{genome_slug:slug,genome_bytes_b64:prev}});
      else if(this.genome(slug)!==undefined)ops.push({RemoveEntityTemplate:{template_id:`genome/${slug}`}});
    };
    const seenParams = new Set<string>();
    const tileRows = new Map<number, Map<number, number>>(); // y → (x → 先前地形)
    for (const op of patch.operations) {
      if ("SetWorldParam" in op) {
        const key = op.SetWorldParam.key_path;
        if (seenParams.has(key)) continue;
        seenParams.add(key);
        const r = previous(this.params.get(key));
        if (r === null) skipped.push({ register: { kind: "param", key }, reason: "superseded" });
        else ops.push({ SetWorldParam: { key_path: key, new_value_json: r.prev ?? this.baseParams.get(key) ?? "null" } });
      } else if ("PaintTerrainRect" in op) {
        const { x0, y0, width, height } = op.PaintTerrainRect;
        for (let y = y0; y < y0 + height; y++) {
          for (let x = x0; x < x0 + width; x++) {
            const i = y * this.width + x;
            const r = previous(this.tiles.get(i));
            if (r === null) {
              if (!tileRows.get(y)?.has(x)) skipped.push({ register: { kind: "tile", x, y }, reason: "superseded" });
              continue;
            }
            const row = tileRows.get(y) ?? new Map<number, number>();
            row.set(x, r.prev ?? (this.baseTerrain[i] as number));
            tileRows.set(y, row);
          }
        }
      } else if ("UpsertSeedGenome" in op) {
        invertCatalogue(op.UpsertSeedGenome.genome_slug);
      } else if ("UpdateRuleModule" in op) {
        const moduleId = op.UpdateRuleModule.module_id;
        const r = previous(this.rules.get(moduleId));
        const prev = r?.prev ?? this.baseRules.get(moduleId);
        if (r === null) skipped.push({ register: { kind: "rule", moduleId }, reason: "superseded" });
        else if (prev === undefined) skipped.push({ register: { kind: "rule", moduleId }, reason: "not_invertible" });
        else ops.push({ UpdateRuleModule: { module_id: moduleId, ...prev } });
      } else {
        if(op.RemoveEntityTemplate.template_id.startsWith("genome/")){invertCatalogue(op.RemoveEntityTemplate.template_id.slice(7));continue;}
        // 非基因目录模板没有恢复操作。
        skipped.push({ register: { kind: "template", templateId: op.RemoveEntityTemplate.template_id }, reason: "not_invertible" });
      }
    }
    ops.push(...tileRunsToRects(tileRows));
    return { operations: ops, skipped };
  }

  /** 由本地时钟生成补丁（尚未整合） */
  public createPatch(clock: HybridLogicalClock, operations: readonly WorldPatchOp[], patchId: readonly number[]): WorldPatch {
    return {
      patch_id: [...patchId],
      base_authoring_revision: this.revision,
      hlc: clock.now(),
      safety_class: safetyClassName(requiredSafetyClass(operations)),
      operations: [...operations],
    };
  }

  private history<K, T>(map: Map<K, Write<T>[]>, key: K): Write<T>[] {
    let h = map.get(key);
    if (h === undefined) {
      h = [];
      map.set(key, h);
    }
    return h;
  }

  private inGrid(x: number, y: number): boolean {
    return x >= 0 && y >= 0 && x < this.width && y < this.height;
  }

  private checkBounds(op: WorldPatchOp): void {
    if (opKind(op) !== "PaintTerrainRect") return;
    const { x0, y0, width, height } = (op as Extract<WorldPatchOp, { PaintTerrainRect: unknown }>).PaintTerrainRect;
    if (width === 0 || height === 0) rejectPatch("涂刷矩形面积为 0");
    if (!this.inGrid(x0, y0) || !this.inGrid(x0 + width - 1, y0 + height - 1)) {
      rejectPatch(`涂刷矩形 (${x0}, ${y0}, ${width}×${height}) 超出 ${this.width}×${this.height} 地形`);
    }
  }
}

/** 把逐格先前地形压缩为最少行程矩形：先按行合并连续同类格，再把上下相邻的同形行程合并 */
export function tileRunsToRects(rows: ReadonlyMap<number, ReadonlyMap<number, number>>): WorldPatchOp[] {
  type Run = { x0: number; width: number; kind: number; y0: number; height: number };
  const open = new Map<string, Run>();
  const done: Run[] = [];
  const ys = [...rows.keys()].sort((a, b) => a - b);
  let prevY: number | null = null;
  for (const y of ys) {
    const row = rows.get(y) as ReadonlyMap<number, number>;
    const xs = [...row.keys()].sort((a, b) => a - b);
    const runs: Run[] = [];
    for (const x of xs) {
      const kind = row.get(x) as number;
      const last = runs.at(-1);
      if (last && last.x0 + last.width === x && last.kind === kind) last.width++;
      else runs.push({ x0: x, width: 1, kind, y0: y, height: 1 });
    }
    const next = new Map<string, Run>();
    for (const r of runs) {
      const key = `${r.x0}:${r.width}:${r.kind}`;
      const above = prevY === y - 1 ? open.get(key) : undefined;
      if (above) {
        above.height++;
        next.set(key, above);
        open.delete(key);
      } else {
        next.set(key, r);
      }
    }
    done.push(...open.values());
    open.clear();
    for (const [k, r] of next) open.set(k, r);
    prevY = y;
  }
  done.push(...open.values());
  done.sort((a, b) => a.y0 - b.y0 || a.x0 - b.x0);
  return done.map((r) => ({ PaintTerrainRect: { x0: r.x0, y0: r.y0, width: r.width, height: r.height, tile_kind: r.kind } }));
}
