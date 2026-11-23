// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// Rust临时身份只经stdin；真实Node→HTTP→原Actor，不打印令牌，不调用外部provider。
import assert from "node:assert/strict";
import {GeneralDriverClient,changeGeneralDriver,listGeneralDrivers} from "../dist/index.js";
import {validateGeneralFrame} from "../../engine/dist/general.js";
let input="";for await(const b of process.stdin)input+=b;const p=JSON.parse(input);input="";
const timer=setTimeout(()=>{console.error("协作SDK链超时，未重试");process.exit(1)},20000);
const request=token=>(path,method="GET",body,signal)=>fetch(p.base+path,{method,headers:{Authorization:`Bearer ${token}`,"Content-Type":"application/json"},...(body===undefined?{}:{body:JSON.stringify(body)}),signal,redirect:"error",credentials:"omit"});
const owner=request(p.ownerToken),driver=request(p.driverToken),renderWorld=p.world.replaceAll("-","");
const frame=(tick,h,l,key=`019aabbc-dddd-7eee-8fff-${(tick+100).toString(16).padStart(12,"0")}`)=>({world:p.world,hostFence:h,controlFence:l,tick,idempotencyKey:key});
const observe=async()=>{const r=await owner(`/api/v1/worlds/${p.world}/general`);assert.equal(r.status,200);return r.json()};
try{
    assert.ok(BigInt(p.driverId)>9007199254740991n);
    if(p.phase==="start"){
        const g=await changeGeneralDriver(owner,p.world,p.driverId,null,true);assert.deepEqual(await listGeneralDrivers(owner,p.world),[g]);
        const c=new GeneralDriverClient(p.driverId,p.world,g.grant_id,driver),initial=await observe();validateGeneralFrame(await c.frame(),renderWorld);
        const {hostFence:h,frame:visible}=await c.connect();validateGeneralFrame(visible,renderWorld);const l=await c.acquire(h);
        let last;for(let t=0;t<3;t++){last=frame(t,h,l);const result=await c.tick(last);assert.equal(result.duplicate,false);validateGeneralFrame(result.frame,renderWorld);assert.equal(result.commit.tick,t+1);}
        const paid=await observe();assert.equal(paid.commit.tick,3);assert.ok(paid.rule_meter.paid_q>0);assert.equal(paid.life.universe_energy_q,initial.life.universe_energy_q);
        assert.equal((await driver(`/api/v1/worlds/${p.world}/general`)).status,404);
        console.log(JSON.stringify({grant:g.grant_id,last,root:paid.commit.state_root,tick:3}));
    }else{
        const old=p.previous,c=new GeneralDriverClient(p.driverId,p.world,old.grant,driver);
        const restored=await c.frame();validateGeneralFrame(restored,renderWorld);assert.equal(restored.stateRoot,old.root);
        const {hostFence:h}=await c.connect();assert.ok(h>old.last.hostFence);const l=await c.acquire(h);
        await assert.rejects(c.tick(old.last));const dup=await c.tick({...old.last,hostFence:h,controlFence:l});assert.equal(dup.duplicate,true);assert.equal(dup.commit.state_root,old.root);assert.equal((await observe()).commit.tick,3);
        const revoked=await changeGeneralDriver(owner,p.world,p.driverId,old.grant,false);await assert.rejects(c.frame());
        const fresh=await changeGeneralDriver(owner,p.world,p.driverId,revoked.grant_id,true),next=new GeneralDriverClient(p.driverId,p.world,fresh.grant_id,driver);
        await assert.rejects(next.tick(frame(3,h,l)));const l2=await next.acquire(h);assert.ok(l2>l);
        await assert.rejects(next.tick({...old.last,hostFence:h,controlFence:l2}));assert.equal((await observe()).commit.tick,3);
        const result=await next.tick(frame(3,h,l2));validateGeneralFrame(result.frame,renderWorld);assert.equal(result.commit.tick,4);await next.release(h,l2);
        const paid=await observe();assert.ok(paid.rule_meter.paid_q>0);assert.equal(paid.life.genesis_energy_q,paid.life.universe_energy_q);
        console.log(JSON.stringify({root:paid.commit.state_root,tick:4,newGrant:fresh.grant_id}));
    }
}finally{p.ownerToken="";p.driverToken="";clearTimeout(timer);}
