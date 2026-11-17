<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 私有 Agent 包：构建、消费与真实受控运行

## 当前可用产品段

`/arena` 连接 **本人已有 Private 单人 PvE** →「私有 Agent 包 / 真实工具委托」。可发现本人版本、上传SDK生成的签名JSON并扫描私有发布、验签下载、撤销、显式单步委托与停止。每步：原Actor当前阵营fog → 服务端QuickJS无I/O求值 → 严格候选工具 → 当前ArenaClient租约/fencing/seq → 原ToolBroker → 原命令Tick/WAL/真实回执。程序可以实际指挥己方单位，不是只打印JSON。一次只执行一个工具；再次点击沿用此runner的有界memory与上次真实工具结果。

这是**所有者显式委托自己当前控制**，不是新注册主体、外部LLM或人类排位自动代打。以上原单步runner仅会话内；第三十四批另接下节独立持久运行/受控循环，仍不冒称生产自主LLM。包可以无工具返回；求值接口本身永不执行工具、开新世界或取得控制。真实客户端必须先连接/取得原控制租约。独立Agent注册/赛道、外部模型服务与无人值守服务端provider尚未实现。

不向公共Hub或外部平台发行。公共Hub的agent发布现拒绝，含agent.json的RYPK也不能改报rpack绕过；原历史数据不删除。owner-private注册表与公共Hub索引隔离，制品在原私有world_snapshots CAS，而不是public blobs。

## 第三十四批：独立持久执行治理与Pi参考Adapter

原`/arena`私有包面板新增「独立持久运行 / Pi参考受控循环」。本人已连接且显式取得控制租约后，选择包、核对显示的精确CID/声明工具、填写**创建时不可追加**的轮次/工具次数/墙钟预算并勾明确授权，才能创建。可发现原记录、读取真实事件、执行一轮/显式启动预算内循环、暂停/取消、回收过期控制器、对账unknown、本人确认当前宿主/控制fence后续跑、关闭。循环在浏览器/可信Node进程，不在权威Tick；刷新/断开终止本地控制进程但不删除服务端记录。旧单步委托与新持久运行是两个有明确范围的入口，不是旧格式fallback。

### Pi选择：借鉴，不直接依赖或复制代码

本次实际打开官方[earendil-works/pi](https://github.com/earendil-works/pi)的[Agent README](https://github.com/earendil-works/pi/blob/main/packages/agent/README.md)、[manifest](https://github.com/earendil-works/pi/blob/main/packages/agent/package.json)与[LICENSE](https://github.com/earendil-works/pi/blob/main/LICENSE)。main manifest为`@earendil-works/pi-agent-core` **1.5.2**（非宣称已安装/核实npm最新），MIT，Copyright (c) 2025 Mario Zechner；core依赖pi-ai/pi-schema 1.5.2。此前已核agent.ts/agent-loop.ts机制继续适用。

选择**原创Rynvek受控Adapter实现，暂不直接加入Pi依赖**：本段只有同源私有包和明确本地测试provider，没有provider模型/凭据配置；接整套pi-ai、coding-agent/UI/bash/文件工具没有必要。未复制、安装、执行Pi代码，未改变项目原许可，故没有声称代码级复用或Pi API/drop-in兼容。未来若直接依赖/复制MIT代码，必须审查固定版本与传递依赖，并保留上述版权/完整MIT许可通知。

实际借鉴点：应用状态与provider上下文分层、provider只产候选、顺序工具执行/真实结果回灌、可中断循环、事件序列与续跑。区别：不用Pi默认并行写；服务端Store事务是持久事实源，不把Pi内存state/continue当灾备。SDK事件回调为观察性回调，不作为持久化或权限屏障；持久化屏障在服务端事务/WAL。没有实现完整Pi message/steering/follow-up/token-delta协议。

### 可运行SDK链

```ts
import { ControlledAgentAdapter, PrivatePackageProvider } from "@rynvek/agent-sdk";
// hub、meta和arena来自上节真实本人会话；已显式取得本人原控制租约。
const api = hub.runs();
const request = {
  submission_id: crypto.randomUUID(), // 超时后重用同键/原请求，勿盲造第二份
  world_id: worldId, package_id: meta.package_id, version: meta.version,
  artifact_cid: meta.artifact_cid, control_fence: String(arena.lease!.fencing_token),
  tools: [...meta.profile.tools], // 可以进一步缩小为声明工具的非空子集
  budget: { turns: 4, tool_calls: 4, wall_seconds: 120 },
  consent: "private-pve-tools/1" as const
};
const run = await api.create(request);
const adapter = new ControlledAgentAdapter(api, run.run_id,
  new PrivatePackageProvider(hub, meta), event => { /* 观察已持久化事件，勿打印凭据 */ });
await adapter.step(); // 一轮：候选→持久派发意图→真实工具→真实结果
// 或显式 await adapter.run(); 在预算内循环，真实Tick不足30只等观测，不伪造推进。
// adapter.stopLocal()仅停止候选/本地循环；已派发不可撤回。
```

`ScriptedTestProvider`是**明确本地测试provider**，只按持久turn计数选择声明候选；它不制造工具回执。可注入的`DecisionProvider`是可信宿主函数，不是QuickJS包，不可把任意函数/远端SDK伪称沙箱。当前边界只声明same-origin-package或explicit-local-test；没有第三方模型、密钥、账单或外部私有数据传输。未来真实LLM必须另获明确授权，不通过名称/标签冒称生产自主智能。停止会取消等待/尚未派发候选，不能杀死任意恶意宿主JavaScript或撤销已发网络/工具。

provider收到白名单`decisionContext`：本阵营fog、已批准六工具schema子集、包/世界标识、预算与计数、memory、真实上次结果投影、最近16个事件的seq/kind。**不传owner、控制fence/controller、操作表、凭据或全世界状态**。原始工具结果完整保留在私有运行事件；超过8KiB的provider结果明确truncated，仅给原tool/tick/error摘要，不伪造result。单次fog≤256KiB、memory≤16KiB。私有包provider仍在原无I/O QuickJS求值，返回候选而非直接写仿真。

### 真实API、原子性与恢复边界

| 方法 | `/api/v1`下路径 | 行为 |
| --- | --- | --- |
| POST / GET | `/agent-runs` | 显式授权创建/本人持久发现，最多64条；同owner/submission同规格返回原运行，异规格409；每owner/world一个未关闭运行 |
| GET | `/agent-runs/{id}` | 私有状态、预算消耗、memory、真实结果/事件/派发意图；未知/他人404 |
| POST | `/agent-runs/{id}/control` | operation_id、expected_revision与action：claim/heartbeat/pause/cancel/recover/ack_unknown/resume/finish；原操作键原样对账 |
| GET | `/agent-runs/{id}/context` | 本人当前Actor fog/批准schema和current_host_fence，标明execution_authorization_valid；不自动重授权 |
| POST | `/agent-runs/{id}/step` | controller_id/fence/operation_id与严格候选；Store首次登记才允许派发，重复候选永不重派 |

全部需既有WorldRead/WorldConnect/MatchPlay及本人Private PvE；只绑定本人已运行席位，不自动接管、开新世界或创建身份。根包/精确依赖CID、撤销与宿主fence在每次上下文/派发前检查；新派发仍由**原CommandGate→原Tick队列→ToolBroker租约/控制等级/30Tick/600APM/schema→原WAL提交→真实回执**。不是新增第七工具。已接受的在途调用可能在取消/撤销到达后完成，不承诺撤回。

复用原Store Memory锁/PG事务、Principal active检查和单独`0007_agent_runs.sql`，没有复制Lab worker队列或另造通用框架。控制器30秒租约/fence；墙钟10–900秒、轮次/工具各1–32。计数在派发意图事务中预留，真实Broker拒绝也消耗尝试次数；取消/恢复不重置预算。96个有界操作回执不驱逐；版本冲突409，同键异请求409。客户端从不提交“已执行结果”，结果只由原Actor回执写入；个人data-export包含同一运行记录。

`tool_dispatch_intent`先持久化，随后才向Actor发命令；工具回执只在原权威Tick/WAL提交后返回。HTTP/控制进程丢失或超时则可能已执行：pending保留，过期显式recover变unknown，**不自动重新派发**。请检查真实世界/WAL与事件后ack_unknown，该确认只放弃自动判定，不撤回或重发原命令。mutation命令键可在既有WAL对账；纯查询/拒绝未必有world命令记录，不假称能据此完整重建所有回执。若在世界提交和运行结果事务之间崩溃，保守unknown而非exactly-once成功。

恢复是**从持久已确认的轮次/记忆/结果续跑**，不是恢复provider的半条token流或进程内存。新SDK/进程先发现/读取；旧running租约过期先recover，无pending时paused，有pending时unknown。本人连接原世界并取得当前控制租约后，resume必须携当前`host_fence`和`control_fence`，保持原工具/预算/包锁并记录新绑定，再新claim；包撤销阻止续跑，但不阻止取消/关闭/读历史。真实LLM、独立注册Agent/排位、无人值守服务端provider、服务端断电与PG灾备未完成。

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

单包≤256KiB、源码≤64KiB、memory≤16KiB、求值输入上次工具结果≤8KiB、请求≤32KiB。原会话runner对过大工具结果只保存明确truncated摘要；新持久运行记录完整原始工具结果，但给provider/QuickJS的上下文投影可明确truncated，见下节。每次QuickJS上下文独立，仅通过显式memory延续；100000 gas、16MiB堆、256KiB栈，无宿主I/O。发布扫描64个有界决策步，**不是1000世界Tick/真实对局soak**；固定Trust 3，不因为扫描或官方样式ID晋升公共Trust 2/1。每AppState两份扫描/求值并发名额，阻塞任务实际结束前一直持有（HTTP取消也不提早释放）。

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

## 第三十批历史直接验证（保留，不冒充最新HEAD全验）

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

## 第三十四批必要直接验证

```sh
pnpm --dir packages/agent-sdk exec tsc -p tsconfig.build.json
cargo test -p rynvek-server --lib agent_runs -- --nocapture
cargo test -p rynvek-server --test agent_runs -- --nocapture
pnpm --dir packages/agent-sdk exec vitest run test/agent_run.test.ts
pnpm --dir apps/web-platform exec tsc --noEmit
pnpm --dir apps/web-platform exec vitest run test/private_agent_panel.test.ts --config vite.config.mjs
```

8个不同直接用例，重跑不累计：状态机3、真正HTTP/WS/两次独立Node控制进程/原Actor链1、SDK不合作provider取消不派发1、原面板JSDOM3。真实集成验证私有QuickJS包provider读取上次真结果、同源无I/O、原六工具中查询与战略HoldPosition实际写入/ack、事件/预算、重复请求不重派；另以明确ScriptedTestProvider执行原真实工具，取消后控制进程退出，再从新Node进程发现原记录/记忆/计数、取得新控制fence并显式续跑。不是假Broker、真实LLM、服务端进程重启、硬杀测试或PG灾备。PG迁移/事务仅Rust编译，未接实库。实际浏览器/硬件、PG多连接多机/断电/长稳仍未验；无全workspace/Clippy/license/SBOM/soak/CI轮询，不绕限制。下一产品主体转音频制品真实容器/校验/私有消费，再一般规则/模板/生命成本；整体仍未完成。
