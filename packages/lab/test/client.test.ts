// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { type FetchLike, LabApiError, LabClient, isParquet } from "../src/index.js";

const PARQUET = new Uint8Array([0x50, 0x41, 0x52, 0x31, 1, 2, 3, 4, 0x50, 0x41, 0x52, 0x31]);
const CID = "ab".repeat(32);

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const pending = (status: string, done = 0) =>
  json(404, {
    code: 0x2002,
    code_hex: "0x2002",
    name: "EntityNotFoundOrTombstoned",
    message: `实验处于 \`${status}\`（${done}/4 run），尚无 Parquet 产出`,
  });
const ready = () =>
  new Response(PARQUET, {
    status: 200,
    headers: { etag: `"${CID}"`, "x-rynvek-experiment-status": "completed" },
  });

/** 按顺序返回预设响应并记录请求 */
function scripted(responses: Array<() => Response>) {
  const calls: Array<{ url: string; init: RequestInit | undefined }> = [];
  const fetch: FetchLike = async (url, init) => {
    calls.push({ url, init });
    const next = responses.shift();
    if (!next) throw new Error("意外的请求");
    return next();
  };
  return { fetch, calls };
}

const noSleep = async () => {};

describe("LabClient", () => {
  it("提交实验：本地校验后 POST 路由 17", async () => {
    const { fetch, calls } = scripted([() => json(202, { exp_id: "e1", status: "queued", runs_total: 2 })]);
    const c = new LabClient({ baseUrl: "http://srv/", token: "tok", fetch });
    const rec = await c.submit({ name: "x", seeds: [1, 2], ticks: 60 });
    expect(rec.exp_id).toBe("e1");
    expect(calls[0]?.url).toBe("http://srv/api/v1/lab/experiments");
    expect(calls[0]?.init?.method).toBe("POST");
    expect(new Headers(calls[0]?.init?.headers).get("authorization")).toBe("Bearer tok");
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({ name: "x", seeds: [1, 2], ticks: 60 });
    await expect(c.submit({ name: "", seeds: [1], ticks: 1 })).rejects.toThrow("name");
    expect(calls).toHaveLength(1);
  });

  it("服务端拒绝映射为 LabApiError", async () => {
    const { fetch } = scripted([
      () => json(429, { code: 0x8004, name: "CommandRateLimitExceeded", message: "同时排队或执行中的实验不能超过 8 个" }),
    ]);
    const err = await new LabClient({ baseUrl: "http://srv", token: "t", fetch })
      .submit({ name: "x", seeds: [1], ticks: 30 })
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(LabApiError);
    expect((err as LabApiError).code).toBe(0x8004);
    expect((err as LabApiError).status).toBe(429);
  });

  it("轮询路由 18 直到 Parquet 就绪", async () => {
    const { fetch, calls } = scripted([() => pending("queued"), () => pending("running", 2), ready]);
    const seen: string[] = [];
    const c = new LabClient({ baseUrl: "http://srv", token: "t", fetch });
    const t = await c.waitForParquet("e/1", { table: "lineage", sleep: noSleep, onPending: (m) => seen.push(m) });
    expect(t.cid).toBe(CID);
    expect(t.table).toBe("lineage");
    expect(isParquet(t.bytes)).toBe(true);
    expect(seen).toHaveLength(2);
    expect(calls[0]?.url).toBe("http://srv/api/v1/lab/experiments/e%2F1/parquet?table=lineage");
  });

  it("实验失败或不存在时立即停止轮询", async () => {
    const failed = scripted([() => pending("failed")]);
    await expect(
      new LabClient({ baseUrl: "http://srv", token: "t", fetch: failed.fetch }).waitForParquet("e", { sleep: noSleep }),
    ).rejects.toThrow("failed");
    const missing = scripted([() => json(404, { code: 0x2002, name: "EntityNotFoundOrTombstoned", message: "实验不存在" })]);
    await expect(
      new LabClient({ baseUrl: "http://srv", token: "t", fetch: missing.fetch }).waitForParquet("e", { sleep: noSleep }),
    ).rejects.toBeInstanceOf(LabApiError);
  });

  it("超时与取消", async () => {
    const { fetch } = scripted(Array.from({ length: 5 }, () => () => pending("running")));
    const c = new LabClient({ baseUrl: "http://srv", token: "t", fetch });
    await expect(c.waitForParquet("e", { intervalMs: 10, timeoutMs: 20, sleep: noSleep })).rejects.toThrow("超时");
    const ac = new AbortController();
    ac.abort();
    await expect(c.waitForParquet("e", { signal: ac.signal })).rejects.toThrow("取消");
  });

  it("downloadAll 下载三张表；非 Parquet 响应被拒绝", async () => {
    const { fetch } = scripted([ready, ready, ready]);
    const all = await new LabClient({ baseUrl: "http://srv", token: "t", fetch }).downloadAll("e", { sleep: noSleep });
    expect(Object.keys(all).sort()).toEqual(["lineage", "metrics", "runs"]);
    const bad = scripted([() => new Response("nope", { status: 200 })]);
    await expect(new LabClient({ baseUrl: "http://srv", token: "t", fetch: bad.fetch }).tryParquet("e")).rejects.toThrow(
      "Parquet",
    );
  });
});
