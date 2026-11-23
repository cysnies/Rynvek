// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// 只接收测试身份stdin；不输出令牌/种子。由Rust真实回环服务器运行，并非mock权限Actor。
import { PrivateRuleClient, privateRuleSigningMessage } from "../dist/index.js";
let input = "";
for await (const part of process.stdin)
    input += part;
const c = JSON.parse(input);
input = "";
const timeout = setTimeout(() => { console.error("SDK真实私库链超时，未自动重试"); process.exit(1); }, 20000);
const request = (path, method = "GET", body, signal) => fetch(c.base + path, { method, headers: { Authorization: `Bearer ${c.token}`, "Content-Type": "application/json" }, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal, redirect: "error", credentials: "omit" });
const hub = new PrivateRuleClient(c.owner, c.trustedKey, request);
const decode = s => Uint8Array.from(Buffer.from(s, "base64"));
const frame = (tick, h, l) => ({ world: c.world, hostFence: h, controlFence: l, tick, idempotencyKey: `019aabbc-dddd-7eee-8fff-${tick.toString(16).padStart(12, "0")}` });
try {
    if (c.mode === "start") {
        const prefix = Buffer.from("302e020100300506032b657004220420", "hex"), key = await crypto.subtle.importKey("pkcs8", Buffer.concat([prefix, Buffer.from(c.testSeed, "hex")]), "Ed25519", false, ["sign"]);
        const sign = async (m) => new Uint8Array(await crypto.subtle.sign("Ed25519", key, m));
        for (const p of c.packages) {
            if (Buffer.from(privateRuleSigningMessage(p.manifest, decode(p.bytes))).toString("hex") !== p.signingMessage)
                throw new Error("Rust/TS签名字节不一致");
            await hub.publish(decode(p.bytes), p.manifest, sign);
        }
        if ((await hub.list()).length !== 4)
            throw new Error("真实私有发现缺少版本");
        if ((await hub.download(c.old.root)).length !== 2)
            throw new Error("可信闭包下载不完整");
        let denied = false;
        try {
            await new PrivateRuleClient(c.owner, "00".repeat(32), request).download(c.old.root);
        }
        catch {
            denied = true;
        }
        if (!denied)
            throw new Error("不能信响应自附公钥");
        const h = (await hub.connect(c.world)).host_fence, l = (await hub.acquire(c.world, h)).lease.fencing_token;
        for (let t = 0; t < 3; t++)
            await hub.tick(frame(t, h, l));
        await hub.install(frame(3, h, l), c.old);
        await hub.tick(frame(4, h, l));
        await hub.revoke(c.oldDependency);
        denied = false;
        try {
            await hub.tick(frame(5, h, l));
        }
        catch {
            denied = true;
        }
        if (!denied)
            throw new Error("撤销后不能继续原规则");
        denied = false;
        try {
            await hub.recoverNextTick(frame(5, h, l), c.next, 0);
        }
        catch {
            denied = true;
        }
        if (!denied)
            throw new Error("不能隐式接受维护成本");
        await hub.recoverNextTick(frame(5, h, l), c.next, 4);
        const v = await hub.observe(c.world);
        if (v.commit.tick !== 6 || v.rule_meter.tier0_gas !== 0 || v.rule_recovery.maintenance_paid_q !== 4)
            throw new Error("维护不是有源隔离Tick");
        console.log(JSON.stringify({ observation: v, verifiedPackages: 2, principal: c.owner }));
    }
    else {
        const v = await hub.observe(c.world);
        if (v.commit.tick !== 6 || !v.rule_recovery)
            throw new Error("私有隔离恢复状态丢失");
        const h = (await hub.connect(c.world)).host_fence, l = (await hub.acquire(c.world, h)).lease.fencing_token;
        await hub.recoverNextTick(frame(6, h, l), c.next, 4);
        await hub.install(frame(7, h, l), c.next);
        await hub.tick(frame(8, h, l));
        const result = await hub.observe(c.world);
        if (result.commit.tick !== 9 || result.rule_recovery !== null || result.rule_meter.paid_q <= 0)
            throw new Error("Epoch安装后未恢复实付规则");
        console.log(JSON.stringify({ observation: result, verifiedPackages: 2 }));
    }
}
finally {
    c.token = "";
    c.testSeed = "";
    clearTimeout(timeout);
}
