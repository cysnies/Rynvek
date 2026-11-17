// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/** 私有持久Lab作业：显式提交幂等键、结构化状态、代次控制及四表消费。 */
import {validateParameterModel,type ParameterModelCapture} from "./parameters.js";
import { RynvekErrorCode, errorNameOf } from "@rynvek/core";

import { type ExperimentSpec, type VariantSpec, type EcologyContract, toRequestBody } from "./spec.js";

export type LabTable = "runs" | "metrics" | "lineage" | "energy";
export const LAB_TABLES: readonly LabTable[] = ["runs", "metrics", "lineage", "energy"];

export type ExperimentStatus = "queued" | "running" | "completed" | "failed" | "cancelled";

/** 服务端实验记录（路由 17 响应体） */
export interface ExperimentRecord {
  readonly exp_id: string;
  readonly owner_principal_id: string;
  readonly submission_id:string;readonly spec_digest:string;readonly kernel_profile:string;
  readonly generation:number;readonly attempt:number;readonly lease_expires_at_unix_s:number|null;
  readonly name: string;
  readonly seeds: readonly number[];
  readonly ticks: number;
  readonly sample_every: number;
  readonly variants: readonly (Omit<VariantSpec,"players"|"ecology"> & {players:number|null;ecology:EcologyContract|null})[];
  readonly status: ExperimentStatus;
  readonly submitted_at_unix_s: number;
  readonly started_at_unix_s: number | null;
  readonly completed_at_unix_s: number | null;
  readonly runs_total: number;
  readonly runs_done: number;
  readonly parquet_cid: string | null;
  readonly tables: Readonly<Partial<Record<LabTable, string>>>;
  readonly error: string | null;
}

export interface LabRows {
  readonly exp_id:string;readonly table:LabTable;readonly cid:string;
  readonly page:{readonly total_rows:number;readonly offset:number;readonly next_offset:number|null;
    readonly columns:readonly string[];readonly rows:readonly Record<string,unknown>[]};
}

/** 下载到的 Parquet 表 */
export interface ParquetTable {
  readonly table: LabTable;
  readonly bytes: Uint8Array;
  /** BLAKE3 内容标识（64 位十六进制） */
  readonly cid: string;
  readonly status: string;
}

/** 服务端错误（统一错误体 `{code, code_hex, name, message}`） */
export class LabApiError extends Error {
  override readonly name = "LabApiError";
  constructor(
    readonly status: number,
    readonly code: number,
    readonly errorName: string,
    message: string,
  ) {
    super(message);
  }
}

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

export interface LabClientOptions {
  /** 服务器根地址，例如 `http://127.0.0.1:8080` */
  readonly baseUrl: string;
  /** 携带 `lab.submit` / `lab.read` 的 CapabilityToken（Base64） */
  readonly token: string;
  readonly fetch?: FetchLike;
}

export interface WaitOptions {
  readonly table?: LabTable;
  readonly intervalMs?: number;
  readonly timeoutMs?: number;
  readonly signal?: AbortSignal;
  /** 每次轮询未就绪时回调（消息含服务端给出的状态与进度） */
  readonly onPending?: (message: string) => void;
  /** 测试注入的等待函数 */
  readonly sleep?: (ms: number) => Promise<void>;
}

const defaultSleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Parquet 魔数 `PAR1`（文件首尾各一次） */
export function isParquet(bytes: Uint8Array): boolean {
  const m = [0x50, 0x41, 0x52, 0x31];
  return (
    bytes.length >= 12 &&
    m.every((b, i) => bytes[i] === b) &&
    m.every((b, i) => bytes[bytes.length - 4 + i] === b)
  );
}

export class LabClient {
  readonly #base: string;
  readonly #token: string;
  readonly #fetch: FetchLike;

  constructor(opts: LabClientOptions) {
    this.#base = opts.baseUrl.replace(/\/+$/, "");
    this.#token = opts.token;
    this.#fetch = opts.fetch ?? ((input, init) => fetch(input, init));
  }

  async #error(res: Response): Promise<LabApiError> {
    let body: { code?: number; name?: string; message?: string } = {};
    try {
      body = (await res.json()) as typeof body;
    } catch {
      // 非 JSON 错误体（代理等）：保留 HTTP 状态
    }
    const code = body.code ?? 0;
    return new LabApiError(
      res.status,
      code,
      body.name ?? errorNameOf(code) ?? "Unknown",
      body.message ?? `HTTP ${res.status}`,
    );
  }

  /** Explicit source copy, requires the current owner's/directed editor's world.edit permission.
   * Revocation stops future capture, not erasure of a source copy already authorized/exported. */
  async captureParameterModel(worldId:string,signal?:AbortSignal):Promise<ParameterModelCapture>{
    const res=await this.#fetch(`${this.#base}/api/v1/worlds/${encodeURIComponent(worldId)}/authoring/lab-model`,{...(signal?{signal}:{}),headers:{Authorization:`Bearer ${this.#token}`},cache:"no-store",redirect:"error",credentials:"omit"});
    if(res.status!==200)throw await this.#error(res);
    const result=await res.json() as ParameterModelCapture;
    if(result.world_id!==worldId||typeof result.revision!=="string"||!/^\d+$/.test(result.revision)||BigInt(result.revision)>(1n<<64n)-1n||!/^[a-f0-9]{64}$/.test(result.model_digest))throw new Error("模型导出身份/修订/摘要非法");
    validateParameterModel(result.model);return result;
  }

  /** 路由 17：提交实验（先在本地按服务端规则校验） */
  async submit(spec: ExperimentSpec, submissionId:string, signal?:AbortSignal): Promise<ExperimentRecord> {
    const body={...toRequestBody(spec),submission_id:submissionId};
    if(!/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(submissionId))throw new Error("submissionId必须是显式UUID；超时后重用同键和原规格");
    const res = await this.#fetch(`${this.#base}/api/v1/lab/experiments`, {
      ...(signal ? {signal}:{}),cache:"no-store",redirect:"error",credentials:"omit",
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${this.#token}` },
      body: JSON.stringify(body),
    });
    if (res.status !== 202) throw await this.#error(res);
    return (await res.json()) as ExperimentRecord;
  }

  /** 私有作业状态/进度/四张结果表CID；未知或非所有者均为404。 */
  async getExperiment(expId:string,signal?:AbortSignal):Promise<ExperimentRecord> {
    const res=await this.#fetch(`${this.#base}/api/v1/lab/experiments/${encodeURIComponent(expId)}`,{
      ...(signal?{signal}:{}),redirect:"error",credentials:"omit",headers:{Authorization:`Bearer ${this.#token}`},cache:"no-store"
    });
    if(res.status!==200) throw await this.#error(res);
    return await res.json() as ExperimentRecord;
  }

  /** 服务重启/刷新后从Store重新发现自己的作业；不依赖客户端缓存。 */
  async listExperiments(offset=0,limit=25,signal?:AbortSignal):Promise<{experiments:ExperimentRecord[];offset:number;limit:number}>{
    if(!Number.isInteger(offset)||offset<0||offset>512||!Number.isInteger(limit)||limit<1||limit>100)throw new Error("无效作业分页");
    const res=await this.#fetch(`${this.#base}/api/v1/lab/experiments?offset=${offset}&limit=${limit}`,{...(signal?{signal}:{}),headers:{Authorization:`Bearer ${this.#token}`},cache:"no-store",redirect:"error",credentials:"omit"});
    if(res.status!==200)throw await this.#error(res);return await res.json() as {experiments:ExperimentRecord[];offset:number;limit:number};
  }
  /** 同一operation_id绑定相同动作/代次；不确定结果后原样重试，不生成第二个操作。 */
  async control(expId:string,op:{operation_id:string;generation:number;action:"cancel"|"retry"},signal?:AbortSignal):Promise<ExperimentRecord>{
    const res=await this.#fetch(`${this.#base}/api/v1/lab/experiments/${encodeURIComponent(expId)}/control`,{...(signal?{signal}:{}),method:"POST",headers:{Authorization:`Bearer ${this.#token}`,"Content-Type":"application/json"},body:JSON.stringify(op),cache:"no-store",redirect:"error",credentials:"omit"});
    if(res.status!==200)throw await this.#error(res);return await res.json() as ExperimentRecord;
  }

  /** 直接读取同一CAS四表的有界页面；不在浏览器重跑仿真或经浮点解码。 */
  async rows(expId:string,table:LabTable,offset=0,limit=250,signal?:AbortSignal):Promise<LabRows> {
    if(!Number.isSafeInteger(offset)||offset<0||offset>0xffff_ffff||!Number.isInteger(limit)||limit<1||limit>500)throw new Error("无效分页边界");
    const res=await this.#fetch(`${this.#base}/api/v1/lab/experiments/${encodeURIComponent(expId)}/rows?table=${table}&offset=${offset}&limit=${limit}`,{
      ...(signal?{signal}:{}),redirect:"error",credentials:"omit",headers:{Authorization:`Bearer ${this.#token}`},cache:"no-store"
    });
    if(res.status!==200)throw await this.#error(res);
    const result=await res.json() as LabRows;
    const p=result.page;
    if(result.exp_id!==expId||result.table!==table||! /^[a-f0-9]{64}$/.test(result.cid)||!p
      ||!Number.isSafeInteger(p.total_rows)||p.total_rows<0||p.offset!==offset||!Array.isArray(p.rows)||p.rows.length>limit||!p.rows.every(r=>r!==null&&typeof r==="object"&&!Array.isArray(r))
      ||p.rows.length>Math.max(0,p.total_rows-offset)||(p.next_offset===null&&offset+p.rows.length<p.total_rows)
      ||!Array.isArray(p.columns)||!p.columns.every(c=>typeof c==="string")
      ||(p.next_offset!==null&&(p.next_offset!==offset+p.rows.length||p.next_offset<=offset||p.next_offset>=p.total_rows)))throw new Error("服务器返回非法分页身份或边界");
    return result;
  }

  /** 路由 18：下载一次；未就绪返回 `{ pending: 消息 }` */
  async tryParquet(expId: string, table: LabTable = "metrics", signal?:AbortSignal): Promise<ParquetTable | { pending: string }> {
    const record=await this.getExperiment(expId,signal);
    if(record.status==="queued"||record.status==="running")return {pending:`${record.status} (${record.runs_done}/${record.runs_total} run)`};
    if(record.status!=="completed")throw new LabApiError(409,0,"LabTerminal",`实验${record.status}：${record.error??"无可下载结果"}`);
    const url = `${this.#base}/api/v1/lab/experiments/${encodeURIComponent(expId)}/parquet?table=${table}`;
    const res = await this.#fetch(url, { ...(signal?{signal}:{}),redirect:"error",credentials:"omit", cache:"no-store", headers: { Authorization: `Bearer ${this.#token}` } });
    if (res.status === 200) {
      const bytes = new Uint8Array(await res.arrayBuffer());
      if (!isParquet(bytes)) throw new LabApiError(200, 0, "InvalidParquet", "响应体不是 Parquet 文件");
      return {
        table,
        bytes,
        cid: (res.headers.get("etag") ?? "").replaceAll('"', ""),
        status: res.headers.get("x-rynvek-experiment-status") ?? "completed",
      };
    }
    throw await this.#error(res);
  }

  /** 轮询直到结果表就绪（实验失败或实验不存在时立即抛出） */
  async waitForParquet(expId: string, opts: WaitOptions = {}): Promise<ParquetTable> {
    const interval = opts.intervalMs ?? 500;
    const timeout = opts.timeoutMs ?? 10 * 60_000;
    const sleep = opts.sleep ?? defaultSleep;
    let waited = 0;
    for (;;) {
      if (opts.signal?.aborted) throw new Error("已取消等待 Lab 实验结果");
      const r = await this.tryParquet(expId, opts.table ?? "metrics",opts.signal);
      if (!("pending" in r)) return r;
      opts.onPending?.(r.pending);
      if (waited >= timeout) {
        throw new LabApiError(408, RynvekErrorCode.TickDeadlineExceeded, "Timeout", `等待超时：${r.pending}`);
      }
      await sleep(interval);
      waited += interval;
    }
  }

  /** 等待并下载全部四张表 */
  async downloadAll(expId: string, opts: Omit<WaitOptions, "table"> = {}): Promise<Record<LabTable, ParquetTable>> {
    const metrics = await this.waitForParquet(expId, { ...opts, table: "metrics" });
    const [runs, lineage, energy] = await Promise.all([
      this.waitForParquet(expId, { ...opts, table: "runs" }),
      this.waitForParquet(expId, { ...opts, table: "lineage" }),
      this.waitForParquet(expId, { ...opts, table: "energy" }),
    ]);
    return { runs, metrics, lineage, energy };
  }
}
