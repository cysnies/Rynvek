// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 路由 17–18 客户端：提交实验并轮询下载 Parquet 结果表。
 *
 * - `17 POST /api/v1/lab/experiments`（`lab.submit`）→ `202` + 实验记录；
 * - `18 GET /api/v1/lab/experiments/{exp_id}/parquet?table=`（`lab.read`）：
 *   产出就绪前返回 `404` / `0x2002 EntityNotFoundOrTombstoned`（消息带状态与进度），
 *   就绪后返回 `application/vnd.apache.parquet`，`ETag` 为 BLAKE3 内容标识，
 *   `x-rynvek-experiment-status` 为实验状态。
 *
 * 失败的实验在路由 18 上同样表现为 `0x2002`，消息中状态为 `failed`；
 * 客户端据此立即停止轮询并抛出 {@link LabApiError}，而不是等到超时。
 */

import { RynvekErrorCode, errorNameOf } from "@rynvek/core";

import { type ExperimentSpec, toRequestBody } from "./spec.js";

export type LabTable = "runs" | "metrics" | "lineage";
export const LAB_TABLES: readonly LabTable[] = ["runs", "metrics", "lineage"];

export type ExperimentStatus = "queued" | "running" | "completed" | "failed";

/** 服务端实验记录（路由 17 响应体） */
export interface ExperimentRecord {
  readonly exp_id: string;
  readonly owner_principal_id: number;
  readonly name: string;
  readonly seeds: readonly number[];
  readonly ticks: number;
  readonly sample_every: number;
  readonly variants: readonly { label: string; mode: string; players: number | null; policy: string }[];
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

  /** 路由 17：提交实验（先在本地按服务端规则校验） */
  async submit(spec: ExperimentSpec): Promise<ExperimentRecord> {
    const res = await this.#fetch(`${this.#base}/api/v1/lab/experiments`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${this.#token}` },
      body: JSON.stringify(toRequestBody(spec)),
    });
    if (res.status !== 202) throw await this.#error(res);
    return (await res.json()) as ExperimentRecord;
  }

  /** 路由 18：下载一次；未就绪返回 `{ pending: 消息 }` */
  async tryParquet(expId: string, table: LabTable = "metrics"): Promise<ParquetTable | { pending: string }> {
    const url = `${this.#base}/api/v1/lab/experiments/${encodeURIComponent(expId)}/parquet?table=${table}`;
    const res = await this.#fetch(url, { headers: { Authorization: `Bearer ${this.#token}` } });
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
    const err = await this.#error(res);
    if (err.code === RynvekErrorCode.EntityNotFoundOrTombstoned && /`(queued|running)`/.test(err.message)) {
      return { pending: err.message };
    }
    throw err;
  }

  /** 轮询直到结果表就绪（实验失败或实验不存在时立即抛出） */
  async waitForParquet(expId: string, opts: WaitOptions = {}): Promise<ParquetTable> {
    const interval = opts.intervalMs ?? 500;
    const timeout = opts.timeoutMs ?? 10 * 60_000;
    const sleep = opts.sleep ?? defaultSleep;
    let waited = 0;
    for (;;) {
      if (opts.signal?.aborted) throw new Error("已取消等待 Lab 实验结果");
      const r = await this.tryParquet(expId, opts.table ?? "metrics");
      if (!("pending" in r)) return r;
      opts.onPending?.(r.pending);
      if (waited >= timeout) {
        throw new LabApiError(408, RynvekErrorCode.TickDeadlineExceeded, "Timeout", `等待超时：${r.pending}`);
      }
      await sleep(interval);
      waited += interval;
    }
  }

  /** 等待并下载全部三张表 */
  async downloadAll(expId: string, opts: Omit<WaitOptions, "table"> = {}): Promise<Record<LabTable, ParquetTable>> {
    const metrics = await this.waitForParquet(expId, { ...opts, table: "metrics" });
    const [runs, lineage] = await Promise.all([
      this.waitForParquet(expId, { ...opts, table: "runs" }),
      this.waitForParquet(expId, { ...opts, table: "lineage" }),
    ]);
    return { runs, metrics, lineage };
  }
}
