// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { type FetchLike, LabApiError, LabClient, isParquet } from "../src/index.js";

const PARQUET = new Uint8Array([0x50, 0x41, 0x52, 0x31, 1, 2, 3, 4, 0x50, 0x41, 0x52, 0x31]);
const CID = "ab".repeat(32);

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const key="00000000-0000-4000-8000-000000000001";
const pending = (status:string,done=0)=>json(200,{status,runs_done:done,runs_total:4,error:null});
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
    const rec = await c.submit({ name: "x", seeds: [1, 2], ticks: 60 },key);
    expect(rec.exp_id).toBe("e1");
    expect(calls[0]?.url).toBe("http://srv/api/v1/lab/experiments");
    expect(calls[0]?.init?.method).toBe("POST");
    expect(new Headers(calls[0]?.init?.headers).get("authorization")).toBe("Bearer tok");
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({ name: "x", seeds: [1, 2], ticks: 60,submission_id:key });
    await expect(c.submit({ name: "", seeds: [1], ticks: 1 },key)).rejects.toThrow("name");
    expect(calls).toHaveLength(1);
  });

  it("服务端拒绝映射为 LabApiError", async () => {
    const { fetch } = scripted([
      () => json(429, { code: 0x8004, name: "CommandRateLimitExceeded", message: "同时排队或执行中的实验不能超过 8 个" }),
    ]);
    const err = await new LabClient({ baseUrl: "http://srv", token: "t", fetch })
      .submit({ name: "x", seeds: [1], ticks: 30 },key)
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(LabApiError);
    expect((err as LabApiError).code).toBe(0x8004);
    expect((err as LabApiError).status).toBe(429);
  });

  it("轮询路由 18 直到 Parquet 就绪", async () => {
    const { fetch, calls } = scripted([() => pending("queued"), () => pending("running", 2),()=>pending("completed",4), ready]);
    const seen: string[] = [];
    const c = new LabClient({ baseUrl: "http://srv", token: "t", fetch });
    const t = await c.waitForParquet("e/1", { table: "lineage", sleep: noSleep, onPending: (m) => seen.push(m) });
    expect(t.cid).toBe(CID);
    expect(t.table).toBe("lineage");
    expect(isParquet(t.bytes)).toBe(true);
    expect(seen).toHaveLength(2);
    expect(calls[0]?.url).toBe("http://srv/api/v1/lab/experiments/e%2F1");
    expect(calls.at(-1)?.url).toBe("http://srv/api/v1/lab/experiments/e%2F1/parquet?table=lineage");
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

  it("downloadAll 下载四张表；非 Parquet 响应被拒绝", async () => {
    const { fetch } = scripted([()=>pending("completed",4),ready,...Array.from({length:3},()=>()=>pending("completed",4)),ready,ready,ready]);
    const all = await new LabClient({ baseUrl: "http://srv", token: "t", fetch }).downloadAll("e", { sleep: noSleep });
    expect(Object.keys(all).sort()).toEqual(["energy", "lineage", "metrics", "runs"]);
    const bad = scripted([()=>pending("completed",4),() => new Response("nope", { status: 200 })]);
    await expect(new LabClient({ baseUrl: "http://srv", token: "t", fetch: bad.fetch }).tryParquet("e")).rejects.toThrow(
      "Parquet",
    );
  });
});

it("结构化实验状态可直接用于工作台而不解析404消息",async()=>{
  const {fetch,calls}=scripted([()=>new Response(JSON.stringify({exp_id:"e/1",status:"completed",tables:{energy:"a".repeat(64)}}),{status:200})]);
  const job=await new LabClient({baseUrl:"http://srv",token:"t",fetch}).getExperiment("e/1");
  expect(job.status).toBe("completed");
  expect(calls[0]?.url).toBe("http://srv/api/v1/lab/experiments/e%2F1");
});

// 仅传输夹具：生产工作台不生成实验或四表替代数据。
it("四表分页保持大整数原文，拒绝错身份、错CID及不前进/越界页面",async()=>{
  const q=(2n**96n).toString();
  const result={exp_id:"private",table:"energy",cid:CID,page:{total_rows:1,offset:0,next_offset:null,columns:["stored_q"],rows:[{stored_q:q}]}};
  const {fetch,calls}=scripted([()=>json(200,result),()=>json(200,{...result,exp_id:"other"}),()=>json(200,{...result,cid:"not-cid"}),()=>json(200,{...result,page:{...result.page,rows:[null]}}),()=>json(200,{...result,page:{...result.page,total_rows:0}}),()=>json(200,{...result,page:{...result.page,total_rows:2}})]);
  const client=new LabClient({baseUrl:"http://srv",token:"owner",fetch});
  expect((await client.rows("private","energy")).page.rows[0]?.stored_q).toBe(q);
  expect(calls[0]?.init?.cache).toBe("no-store");
  for(let i=0;i<5;i++)await expect(client.rows("private","energy")).rejects.toThrow("非法分页");
  await expect(client.rows("private","energy",0,501)).rejects.toThrow("分页边界");expect(calls).toHaveLength(6);
});

it("持久发现/控制保留同一操作键与代次，取消终态不解析错误消息",async()=>{
 const {fetch,calls}=scripted([()=>json(200,{experiments:[],offset:0,limit:25}),()=>json(200,{status:"cancelled",generation:0}),()=>pending("cancelled")]);
 const c=new LabClient({baseUrl:"http://srv",token:"private",fetch});expect((await c.listExperiments()).experiments).toEqual([]);
 const op={operation_id:key,generation:0,action:"cancel" as const};expect((await c.control("id",op)).status).toBe("cancelled");expect(JSON.parse(String(calls[1]?.init?.body))).toEqual(op);
 expect(calls.every(c=>c.init?.redirect==="error"&&c.init.credentials==="omit"&&c.init.cache==="no-store")).toBe(true);
 await expect(c.waitForParquet("id")).rejects.toThrow("cancelled");expect(calls).toHaveLength(3);
});
