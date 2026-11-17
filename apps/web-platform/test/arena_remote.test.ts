// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// 仅传输单测：复用原记录观测夹具，不代表真实HTTP/浏览器/重演算法验收。
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {expect,it,vi} from "vitest";
import {createArenaApp} from "@rynvek/arena";
import type {ArenaServerMessage} from "@rynvek/client";
import {ArenaWorldClient,ReplayObservationTransport,type PlaybackFrame} from "../src/arena-remote.js";
const transcript=JSON.parse(readFileSync(resolve(process.cwd(),"../../tests/arena/opening.transcript.json"),"utf8")) as {recv:{to:string;message:ArenaServerMessage}[]}[];
const welcome=transcript[0]!.recv[0]!.message as Extract<ArenaServerMessage,{type:"welcome"}>;
const message=transcript[5]!.recv.find(e=>e.to==="human:p0")!.message as Extract<ArenaServerMessage,{type:"observation"}>;
const world="a0000000-0000-4000-8000-000000000001",match="10000000-0000-4000-8000-000000000002",cursor="10000000-0000-4000-8000-000000000003";
function frame():PlaybackFrame{return {match_id:match,world_id:world,faction:0,mode:welcome.mode,initial_tick:message.observation.tick,final_tick:10,
 tick:message.observation.tick,revision:"7",state_root:"ab".repeat(32),initial_revision:"7",initial_state_root:"ab".repeat(32),target_tick:message.observation.tick,ready:true,
 signature_verified:true,complete_verified:false,verified_records:0,total_records:10,map:welcome.map,observation:message.observation,events:message.events,events_truncated:0,from_record:0};}
it("已有世界只走正式run，凭据只在头/hello，断开后不能继续请求",async()=>{
 const fetcher=vi.fn(async(_url:unknown,_init?:RequestInit)=>Response.json({world_id:world,match_id:match,principal_id:"123",faction:0,mode:"PvE_Survival",match_state:"Running",tick:1,recovered:false}));
 const client=new ArenaWorldClient("http://srv","only-memory",fetcher);
 const result=await client.run(world.toUpperCase());expect(result.world_id).toBe(world);expect(fetcher.mock.calls[0]?.[0]).toBe(`http://srv/api/v1/worlds/${world}/arena`);
 const init=fetcher.mock.calls[0]?.[1] as RequestInit;expect(init.headers).toMatchObject({Authorization:"Bearer only-memory"});expect(init.redirect).toBe("error");expect(init.cache).toBe("no-store");
 expect(client.streamURL(world)).not.toContain("only-memory");expect(client.streamURL(world)).toContain("format=arena-json");
 const hello={type:"hello",protocol:"rynvek-arena/1",principal_id:"123",kind:"Human",faction:0} as const;
 expect(client.hello(hello)).toMatchObject({capability_token:"only-memory"});expect(hello).not.toHaveProperty("capability_token");
 client.destroy();await expect(client.run(world)).rejects.toThrow("断开");expect(fetcher).toHaveBeenCalledTimes(1);expect(()=>client.hello(hello)).toThrow("断开");
});
it("只消费匹配世界/阵营的验签重演回执，未验签或虚假的终态标记拒绝",async()=>{
 let response={...frame(),cursor_id:cursor};const fetcher=vi.fn(async(_url:unknown,_init?:RequestInit)=>Response.json(response));const client=new ArenaWorldClient("http://srv","owner",fetcher);
 expect((await client.openPlayback(match,world,0)).complete_verified).toBe(false);
 response={...response,world_id:match};await expect(client.openPlayback(match,world,0)).rejects.toThrow("不一致");
 response={...frame(),cursor_id:cursor,complete_verified:true};await expect(client.openPlayback(match,world,0)).rejects.toThrow("不一致");
 await expect(client.seek(match,cursor,world,0,Number.MAX_SAFE_INTEGER+1)).rejects.toThrow("安全");
});
it("只读回放复用原Arena reducer/控制器/地图，任何写请求拒绝，不产生替代观测",async()=>{
 const source=new ReplayObservationTransport(frame()),app=createArenaApp(source,{principalId:"owner",kind:"Human",faction:0});
 await app.client.connect();expect(app.client.observation).toEqual(message.observation);expect(app.client.terrain?.width).toBe(welcome.map.width);
 expect(app.client.lease).toBeNull();expect(()=>source.send({type:"acquire_lease",seq:1,level:"Sovereign"})).toThrow("禁止写命令");
 expect(()=>source.send({type:"command",seq:2,fencing_token:1,command:{type:"surrender"}})).toThrow("禁止写命令");
 app.dispose();expect(()=>source.send({type:"hello",protocol:"rynvek-arena/1",principal_id:"owner",kind:"Human",faction:0})).toThrow("关闭");
});
