<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 私有 Agent 包：构建、消费与真实受控运行

## 当前可用产品段

`/arena` 连接 **本人已有 Private 单人 PvE** →「私有 Agent 包 / 真实工具委托」。可发现本人版本、上传SDK生成的签名JSON并扫描私有发布、验签下载、撤销、显式单步委托与停止。每步：原Actor当前阵营fog → 服务端QuickJS无I/O求值 → 严格候选工具 → 当前ArenaClient租约/fencing/seq → 原ToolBroker → 原命令Tick/WAL/真实回执。程序可以实际指挥己方单位，不是只打印JSON。一次只执行一个工具；再次点击沿用此runner的有界memory与上次真实工具结果。

这是**所有者显式委托自己当前控制**，不是新注册主体、外部LLM、独立持久自主Agent或人类排位自动代打。包可以无工具返回；求值接口本身永不执行工具、开新世界或取得控制。真实客户端必须先连接/取得原控制租约。独立Agent赛道、模型服务、后台自动循环与持久运行记忆不在本段完成范围。

不向公共Hub或外部平台发行。公共Hub的agent发布现拒绝，含agent.json的RYPK也不能改报rpack绕过；原历史数据不删除。owner-private注册表与公共Hub索引隔离，制品在原私有world_snapshots CAS，而不是public blobs。

## RYPK `.rpack` 严格 Agent profile

复用现有RYPK v1容器、条目与整体BLAKE3；不增加历史兼容解析。`engine_compat=rynvek-agent-tools/1`，只准三个文件：

| 文件 | kind | media_type |
| --- | --- | --- |
| agent.json | data | application/json |
| dependencies.lock | data | application/json |
| main.js | rule_source | text/javascript |

```json
{"schema":"rynvek.agent.v1","runtime":"quickjs-tools/1","role":"agent","entrypoint":"main.js","tools":["query_visible_clusters"]}
```

`role`只能是agent或library。library不能声明控制工具，声明`function transform(input)`；agent声明`function tick(input)`并返回严格`{call:null|{name,arguments},memory:{}}`。候选只可使用profile声明的既有六工具，参数由原ToolRegistry再次严格检查；未知字段/工具不忽略。

输入只包含`observation`、有界`memory`及`last_result`。observation来自原Actor本人固定阵营，而非调用者上传的假观测；没有Bearer、签名器、私有源码、完整世界/CID、文件系统或网络。服务端不把welcome里的全地图元数据喂给包。

精确依赖锁：

```json
{"lockfile_version":1,"packages":[{"id":"sample.policy","version":"1.0.0","blake3_256":"精确64位小写hex"}]}
```

只能依赖**同owner、已发布、未撤销、签名/摘要正确的library**，通过冻结词法`deps["sample.policy"](input)`实际消费纯函数。library也接受并应能处理有界的扫描观测上下文，不提供通用npm/module/OS加载器。每节点最多8直接依赖、整个图16库、深度4、组合源码256KiB，环与摘要冲突拒绝。发布事务重验整个依赖闭包，运行时按实际锁递归加载/验签；撤销任一精确依赖后，后续相关求值拒绝。已合法下载的字节无法从使用者手中抹除，撤销不承诺回收历史信息。

单包≤256KiB、源码≤64KiB、memory≤16KiB、上次工具结果≤8KiB、请求≤32KiB。SDK对过大工具结果只保存明确truncated的回执摘要。每次QuickJS上下文独立，仅通过显式memory延续；100000 gas、16MiB堆、256KiB栈，无宿主I/O。发布扫描64个有界决策步，**不是1000世界Tick/真实对局soak**；固定Trust 3，不因为扫描或官方样式ID晋升公共Trust 2/1。每AppState两份扫描/求值并发名额，阻塞任务实际结束前一直持有（HTTP取消也不提早释放）。

保守凭据标记扫描会拒绝Bearer/Authorization/api_key/password/私钥头等已知污染，不声称能识别所有编码秘密。发布者必须不把任何凭据/个人秘密放进包或memory；真正隔离依赖于无I/O、不注入凭据、私有CAS和原工具鉴权，而非字符串扫描证明绝对安全。沙箱错误不回显源码/输出。

## SDK用法（外部签名器，绝不把签名私钥装进包）

`@rynvek/agent-sdk`提供构建、签名回调、私有HTTP客户端与真实工具runner。下面函数接收调用者已有的正式会话、签名器和已连接ArenaClient；SDK不保管私钥，也不新注册假用户：

```js
import { signPrivateAgentPackage, PrivateAgentHubClient, PrivateAgentRunner } from "@rynvek/agent-sdk";

export async function publishAndStep({ origin, ownerId, capability, signWithRegisteredKey, arena, worldId }) {
  // 回调由已有凭据管理/签名器实现；其公钥必须是此Principal登记的Ed25519公钥。
  const upload = await signPrivateAgentPackage(ownerId, {
    id: "sample.observer", version: "1.0.0", name: "私有观测策略", license: "MIT",
    role: "agent", tools: ["query_visible_clusters"],
    source: 'function tick(input){return {call:{name:"query_visible_clusters",arguments:{include_enemy:true}},memory:{tick:input.observation.tick}};}'
  }, signWithRegisteredKey);
  // upload只含owner/包ID/版本/摘要/容器Base64/签名，可存为签名JSON供工作台上传；不含capability/私钥。
  const hub = new PrivateAgentHubClient(origin, ownerId, () => capability);
  const meta = await hub.publish(upload); // 版本不可覆盖，包括撤销墓碑；再次发布需明确新版本。
  const artifact = await hub.download(meta); // 验BLAKE3和owner域Ed25519，不执行下载源码。
  await arena.acquireLease("Sovereign"); // 本人已连接的原世界；不自动取得其他人的控制。
  const runner = new PrivateAgentRunner(hub, meta, worldId, arena);
  try {
    const step = await runner.step();
    return { artifact, step }; // dispatched/accepted及真实receipt；候选executed始终false。
  } finally { runner.stop(); hub.destroy(); }
}
```

私有签名消息为UTF-8 `rynvek-private-agent/1\0owner_decimal\0id\0version\0`后接完整容器32字节BLAKE3；与公共Hub签名域不同。下载公钥锚定于已认证服务元数据，不能冒称独立CA/外部信任链。可用SDK的`agentDigest`及`privateAgentSigningMessage`对接已有签名器；`buildPrivateAgentPackage`只输出字节、不执行源码。

SDK仅接受无凭据HTTP(S) origin；Bearer仅在认证头，`cache:no-store`、`redirect:error`，不放URL/下载文件/持久浏览器存储。销毁后provider失效。runner检查owner/世界/CID/阵营/模式/当前fence，busy期间不重复提交，stop/换租约后迟到候选不发送。工具已发而超时/连接丢失则停止，不能撤回已发命令、不自动重试未知回执；工具自身明确拒绝不能标accepted。刷新/断开/换包清空会话内Agent记忆，不宣称持久Agent进程恢复。

## HTTP入口与持久化

| 方法 | 路径（均 /api/v1 前缀） | 授权/效果 |
| --- | --- | --- |
| GET | /private-hub/agents | WorldRead，只有本人全部版本，最多256项，含撤销墓碑 |
| PUT | /private-hub/agents/{id}/{version} | HubPublish，严格artifact_b64/signature；owner只来自已验证会话 |
| GET | /private-hub/agents/{id}/{version} | WorldRead，本人元数据 |
| GET | /private-hub/agents/{id}/{version}/artifact | WorldRead，本人未撤销签名制品；无公共CID读取旁路 |
| DELETE | /private-hub/agents/{id}/{version} | HubPublish，幂等置revoked，不删除版本/原数据 |
| POST | /private-hub/agents/{id}/{version}/evaluate | WorldRead+WorldConnect+MatchPlay；严格world_id/artifact_cid/memory/last_result；本人Private PvE/固定席位/运行宿主fence |

成功响应private,no-store、Vary Authorization。求值后重验身份、私有世界/owner/模式/fence与根包/依赖撤销，失效丢候选；实际写入仍由之后原ToolBroker鉴权/租约/限速/命令事务作最终裁决，不把先前的求值当作持久授权。Profile/CID/签名同不可变版本绑定；每主体最多256版本，撤销不释放版本号或配额。预检避免已存在版本/满额重复落CAS，最终Store事务再串行化配额与全部依赖检查。

Memory复用原Store锁；PG新增幂等应用迁移`0005_private_agents.sql`，复用原Principal行锁/事务及私有CAS。PG持久元数据设计不等于已经实库执行；Memory数据不跨OS进程持久化。真正重启部署仍需原持久Store/私有持久CAS/稳定鉴权，运行中runner memory仅会话内。本阶段没有做PG多机灾备或硬件断电验收，也不自动删除预写但未被元数据引用的私有CAS字节。

## 必要直接验证（不是全门禁）

```sh
pnpm --filter @rynvek/agent-sdk build
pnpm --filter @rynvek/client build
# 第三项需要Node22和上述dist，显式启用；缺这些前提时普通Rust测试会列为ignored而非假通过。
cargo test -p rynvek-server --test private_agents --offline -- --include-ignored
pnpm --filter @rynvek/agent-sdk typecheck
pnpm --filter @rynvek/agent-sdk exec vitest run test/private_agent.test.ts
pnpm --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/web-platform exec vitest run test/private_agent_panel.test.ts
```

Rust三项实际通过：TS生成RYPK/签名域跨语言夹具与受限沙箱负例；签名/owner/未知字段/公共误发拒绝；真实Node SDK私有发布→发现→摘要/签名下载→原Actor fog→精确依赖函数→QuickJS→ArenaClient真实WS租约→查询回执→30Tick间隔→战略HoldPosition写入/己方真实单位order与回执→依赖撤销拒绝。SDK五项、JSDOM工作台两项，共十个不同直接用例，不累计重跑。

工作台挂载是JSDOM夹具，不是假称真实浏览器；Node+Rust是实际回环TCP/HTTP/WS和原Actor，不是多主机。未跑全workspace/Clippy/license/SBOM/soak/CI轮询，未重编WASM/Vite；真实受限浏览器/物理GPU/音频设备/PG迁移与多连接/多机/断电/24h未验，未规避浏览器或网络策略。下一主体优先C# SDK的真实鉴权/HTTP/WS工具消费，其后接原音频引擎到真实可见事件/用户手势和神经声化，而不是继续只打磨Arena。
