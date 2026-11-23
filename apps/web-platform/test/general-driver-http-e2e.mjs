// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// 仅合成临时身份stdin；真实Web控制器/SDK→HTTP→原Actor，不是浏览器或耐久进程恢复证明。
import assert from "node:assert/strict";
import {GeneralWorldSession} from "../dist/general-remote.js";
let input="";for await(const b of process.stdin)input+=b;const p=JSON.parse(input);input="";
const timer=setTimeout(()=>{console.error("Web协作链超时，未重试");process.exit(1)},20000);
let owner,driver;const paths=[];
try{
 owner=await GeneralWorldSession.open(p.base,p.ownerToken);
 await owner.select(p.world);const initial=owner.observation;
 const granted=await owner.changeDriver(p.driverId,null,true);assert.equal((await owner.listDrivers())[0].grant_id,granted.grant_id);
 const transport=(url,options)=>{paths.push(new URL(String(url)).pathname);return fetch(url,options)};
 driver=await GeneralWorldSession.openRuleDriver(p.base,p.driverToken,transport);
 assert.ok(BigInt(driver.identity.principal_id)>9007199254740991n);
 await driver.join(p.world,granted.grant_id);assert.equal(driver.hostFence,undefined);assert.equal(driver.controlFence,undefined);assert.equal(driver.frame.tick,0);assert.equal(Object.hasOwn(driver,"observation"),false);
 await driver.connect();await driver.acquire();const oldControl=driver.controlFence;
 for(let n=0;n<3;n++)await driver.advance();assert.equal(driver.frame.tick,3);
 const projected=await driver.renderFrame();assert.equal(projected.stateRoot,driver.frame.stateRoot);assert.deepEqual(Object.keys(projected).sort(),["bodies","format","revision","stateRoot","tick","worldId"]);
 await owner.refresh();assert.equal(owner.observation.commit.tick,3);assert.ok(owner.observation.rule_meter.paid_q>0);assert.equal(owner.observation.life.universe_energy_q,initial.life.universe_energy_q);
 const revoked=await owner.changeDriver(p.driverId,granted.grant_id,false);
 await assert.rejects(driver.refresh());assert.equal(driver.frame,undefined);assert.equal(driver.controlFence,undefined);
 const fresh=await owner.changeDriver(p.driverId,revoked.grant_id,true);assert.notEqual(fresh.grant_id,granted.grant_id);
 await assert.rejects(driver.join(p.world,granted.grant_id));await driver.join(p.world,fresh.grant_id);await driver.connect();await driver.acquire();assert.ok(driver.controlFence>oldControl);await driver.advance();assert.equal(driver.frame.tick,4);await driver.release();assert.equal(driver.controlFence,undefined);
 await owner.refresh();assert.equal(owner.observation.commit.tick,4);assert.equal(owner.observation.life.universe_energy_q,initial.life.universe_energy_q);
 assert.ok(paths.every(path=>path==="/api/v1/auth/session"||path===`/api/v1/worlds/${p.world}/general/drive`),"协作Web不得试探owner元数据/全知观察/私包");
 console.log(JSON.stringify({tick:4,root:owner.observation.commit.state_root,paid:owner.observation.rule_meter.paid_q,driverRequests:paths.length}));
}finally{driver?.destroy();owner?.destroy();p.ownerToken="";p.driverToken="";clearTimeout(timer);}
