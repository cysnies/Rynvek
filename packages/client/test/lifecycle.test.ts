// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import {expect,it,vi} from "vitest";
import {ArenaClient,type ArenaServerMessage,type ArenaTransport} from "../src/index.js";
it("销毁拒绝握手/租约/观测等待并清空私有状态，迟到消息和重复销毁不复活身份",async()=>{
 let receive!:(message:ArenaServerMessage)=>void;const off=vi.fn(),close=vi.fn();
 const transport:ArenaTransport={send:vi.fn(),onMessage(handler){receive=handler;return off;},close};
 const client=new ArenaClient(transport,{principalId:"owner",kind:"Human",faction:0});
 const first=client.connect();await expect(client.connect()).rejects.toThrow("尚未完成");
 const results=Promise.allSettled([first,client.acquireLease("Sovereign"),client.nextObservation()]);
 client.dispose();client.dispose();
 expect((await results).every(r=>r.status==="rejected")).toBe(true);expect(off).toHaveBeenCalledTimes(1);expect(close).toHaveBeenCalledTimes(1);
 receive({type:"observation",observation:{tick:999} as never,events:[]});expect(client.observation).toBeNull();expect(client.lease).toBeNull();expect(client.terrain).toBeNull();expect(client.events).toHaveLength(0);
 await expect(client.connect()).rejects.toThrow("销毁");await expect(client.nextObservation()).rejects.toThrow("销毁");
});
it("只读/已关闭传输的同步拒绝会结清请求，不留下悬挂租约Promise",async()=>{
 const transport:ArenaTransport={send(){throw new Error("只读回放拒绝写入")},onMessage(){return ()=>{}}};
 const client=new ArenaClient(transport,{principalId:"viewer",kind:"Human",faction:0});
 await expect(client.acquireLease("Sovereign")).rejects.toThrow("只读回放拒绝写入");client.dispose();
});
