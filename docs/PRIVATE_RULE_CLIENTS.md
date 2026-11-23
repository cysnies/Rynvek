<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 私有规则客户端与撤销后的显式实付隔离续跑

阶段52，2026-11-24，中国标准时间 UTC+8。从干净 `b9fad2bace0bac283cb3c98d2e3eb40c30426676` 接续。**项目整体未完成、未达正式发布质量。** 原签名格式/私库及界限见[PRIVATE_RULE_PACKAGES.md](PRIVATE_RULE_PACKAGES.md)，不重复AST/包解析器/Actor或扩大为任意rpack。

## 本批真实产品闭环

- CLI `rule publish/list/download/observe/connect/acquire/tick/install`；`install --recover-tick` 是另一次明确同意成本的单Tick隔离维护，不是自动降级或自动补Tick。
- `@rynvek/agent-sdk` 导出 `PrivateRuleClient`、`privateRuleSigningMessage`、`verifyPrivateRule` 及对应类型。调用方传既有可信登记公钥和已认证request；发布回执/下载按该钥验签，不能把服务端自带的任意钥当信任锚。
- 实际CLI子进程和TS SDK均到达原General权限Actor、源能量实付、私有文件CAS/WAL恢复；TS真实大于2^53的主体ID不截断，Rust/TS签名字节逐字节一致。
- 撤销后原Tick仍拒绝；用户显式提交有源隔离维护6/7，再在Epoch8安装新包继续实付规则。中间停止/重启仍恢复隔离态，不能普通Tick偷续，也不能越Epoch、免费跳Tick或静默恢复旧规则。

## 正确性：隔离维护不是提前热换规则

原规则热换仍只在 `next_tick % 4 == 0`。为跨越撤销停在非Epoch的间隔，增加原宿主显式 `recover` 动作：

1. 仅当前确有已安装来源撤销时才能进入，或已处于该显式隔离态时继续。健康世界不能用它廉价跳过正常规则。
2. 人类所有者原Sovereign、WorldConnect/Edit、ABAC、Active、宿主/控制双fence、原CommandGate/APM/幂等全部保持。`GeneralInput.recovery` 与 `install` 分别对应明确动作，物理payload必须为空；没有自动获取lease、推进、重试或回退。
3. 用户先提供新包的完整可信签名闭包、全体已有Tier0命名模块映射。当前只支持有源生命、正价格、至少一个存活受控身体、无Tier1的受限剖面；替换图须覆盖全部现有Tier0图及binding。不会为救活无来源/欠费世界增能量。
4. **隔离期间旧图不运行，新图也不运行**。只是保留旧图/签名作历史，暂存候选新证据CID；原Phase07不执行作者VM，清零受控身体加速度，保持原速度的正常物理滑行/碰撞。不是默认控制器或新规则偷偷提前生效。
5. 每一个 `recover` 仍跑原完整Tick：Clock、场求解/摄食、物理、代谢、不变量、RYC2和耐久提交，`revision == tick` 不变。规则gas/quota及规则paid_q均为0；每个当时存活受控身体额外从原EnergyAccount实付 `4 × 已有rule_cost_micro_per_gas` 个EnergyQ，记入独立 `rule_recovery.maintenance_paid_q` 并热耗。**这是明确的宿主隔离维护成本，不是伪造执行过的指令gas、CPU配额或机械功**；原生命维护也不免除。
6. 用户通过观察 `recovery_maintenance_q_per_body` 得到报价；CLI必须给 `--accept-maintenance-q-per-body`，SDK `recoverNextTick` 必须给相同数值，才发维护请求。当前单身体价格1的示例为4 EnergyQ；多身体按实际存活控制身体累加，不能假称整个世界只付4。欠费/无存活供能身体/不合法图仍整Tick失败，不发布摄食、参数、根或回执。
7. 新候选来源CID/目标Epoch进原配置根，实付维护进原状态根/快照。显式维护只能走到Epoch之前；下一Epoch若无明确 `install`，停止而非自动激活。安装后才清隔离态、执行新规则并走原规则实付。
8. **原Store撤销提交锁代码零改动**。隔离期间历史旧图只存档不执行，原RulePermit改为守卫实际暂存、即将激活的完整新包闭包；仍在同事务检查owner/公钥/CID/撤销/editor/Active/fence/前驱。若新候选resolve后被撤销，最终提交锁照常拒绝，不能因“恢复”标志绕开。用户可明确提出另一个完整有效计划，不能静默降级。

安装日志原v2、普通Tick原v1兼容；隔离维护为严格v3（同一GeneralTickRecord和重放器，不另造日志系统）。恢复对签名证据/原图/新计划逐一复验，历史已提交旧Tick仍按历史重演；“不运行撤销规则”指后续活控制执行，不抹除历史。隔离态下任意下一正常Tick均拒绝，重启后也一样。

## CLI实际入口

先沿已有 `keygen/login` 流程取得自己登记身份、公钥及短生命周期能力令牌。令牌只放调用进程环境 `RYNVEK_TOKEN`，不放URL/包/源码/日志，不提交 `.env`。下例变量由调用方自己的既有登录/上一步回执提供，不是要求上传真实凭据。

当前共用的原极简Rust HTTP传输没有TLS；新规则命令因此**仅允许明确回环literal origin**（如 `http://127.0.0.1:端口`、`http://[::1]:端口`），不静默把Bearer明文发外网，不用代理/隧道冒充生产TLS。

```sh
# 先cargo build -p rynvek-cli；BIN、SERVER、OWNER、PUB、WORLD及令牌取自自己的会话。
R() { "$BIN" rule "$@" --server "$SERVER" --owner "$OWNER" --trusted-key "$PUB"; }
R publish --package steer.rpack --key own-signing-key --dependencies deps.json
R list
R download --id rules.mine.steer --version 1.0.0 --cid "$CID" --out verified.rpack
R observe --world "$WORLD"
R connect --world "$WORLD"
R acquire --world "$WORLD" --host-fence "$HOST_FENCE"
# 每次操作使用自己生成的唯一UUIDv7幂等键；Tick/fence必须来自当前观察/租约。
R tick --world "$WORLD" --host-fence "$HOST_FENCE" --control-fence "$CONTROL_FENCE" --tick 2 --idempotency-key "$K3"
R install --world "$WORLD" --request install.json --host-fence "$HOST_FENCE" --control-fence "$CONTROL_FENCE" --tick 3 --idempotency-key "$K4"
# 假定已撤销并停在Tick5。先观察维护报价FEE、准备新完整签名包组，明确逐Tick操作：
R install --recover-tick --accept-maintenance-q-per-body "$FEE" --world "$WORLD" --request next.json --host-fence "$HOST_FENCE" --control-fence "$CONTROL_FENCE" --tick 5 --idempotency-key "$K6"
R install --recover-tick --accept-maintenance-q-per-body "$FEE" --world "$WORLD" --request next.json --host-fence "$HOST_FENCE" --control-fence "$CONTROL_FENCE" --tick 6 --idempotency-key "$K7"
R install --world "$WORLD" --request next.json --host-fence "$HOST_FENCE" --control-fence "$CONTROL_FENCE" --tick 7 --idempotency-key "$K8"
```

`deps.json` 是精确引用数组 `{id,version,blake3_256}`；发布客户端排序并沿服务器原校验拒绝重复/范围。`install.json` / `next.json` 是 `{root: 精确引用, modules: {包ID: 已有模块名}}`，须一对一覆盖完整闭包，不创建任意绑定/组件。每条操作都是一次，示例不替用户隐含运行缺少的Tick。

`download` 自己按每层已验签锁遍历，不盲信resolved列表；共用原Rust `verify_download` / 唯一RYPK/IR重编，闭包1MiB/16包/深度8。全部通过后才create_new写根制品，不覆盖原文件；依赖逐一验证但不隐含安装。无自动重试，网络不确定须先观察/按同幂等键取回执。

## TS SDK

```ts
import { PrivateRuleClient } from "@rynvek/agent-sdk";
const hub = new PrivateRuleClient(ownerIdString, trustedRegisteredPublicKeyHex, authenticatedRequest);
await hub.publish(actualRpackBytes, exactManifest, signEd25519Callback);
const discovered = await hub.list(); // 元数据发现不等于制品已经验签
const verified = await hub.download(explicitRootRef);
const connected = await hub.connect(worldId);
const lease = await hub.acquire(worldId, connected.host_fence);
// frame包含当前world/hostFence/controlFence/tick与UUIDv7，不会自动取代调用方时序。
await hub.install(frameAtTick3, explicitPlan);
await hub.recoverNextTick(frameAtTick5, explicitReplacementPlan, acceptedMaintenanceQPerBody);
```

`authenticatedRequest(path, method, body, signal)` 只连接调用方选定的可信服务，令牌放Authorization，拒绝重定向/凭据外送，响应读取由SDK再做硬界。实际Web/HTTPS传输由宿主注入；本批真实Node回环调用不是浏览器/生产TLS实证。退出登录由宿主清令牌/abort，不把认证状态塞入规则包。

签名消息按原固定ASCII键序，Rust/TS逐字节一致；下载逐层校验owner、公钥、Ed25519、CID、精确DAG及总量。**SDK不复制AST、RYPK或IR编译器，也不声称本地TS重新证明graph→IR**；服务器仍以唯一Rust解析器强制复验，Rust CLI额外本地复验。`privateRuleSigningMessage` 对任意字节做摘要/签名本身不是IR证明。

原 `CommandEnvelope.principal_id` 新兼容规范十进制u64字符串输入，原数字输入/数字序列化不变。SDK发完整owner字符串，真实测试身份大于2^53；拒绝不安全的Tick/fence整数，不将浮点截断值当权限身份。包ID/版本为已有剖面的保守ASCII子集，不宣称全u64观测SDK或全规范客户端均已实现。

## 必要验证与发布阻塞

87个不同定向用例通过，新增14，重跑不累计：SDK25（新增7）；server General9/私有规则10（新增3，真实Node SDK用例显式include-ignored）；Rust私库客户端/HTTP边界2新增；CLI真实子进程1新增；仿真规则10/生命8；协议22（新增u64兼容1）。

- 真实CLI发布四个签名版本/两个精确双包组、发现/可信下载、错钥拒绝/拒覆盖、原连接/lease/逐Tick、明确接受维护费、6/7维护→8安装实付及新App私有文件CAS恢复同根。
- 真实TS SDK签名发布四个版本、Rust/TS签名消息一致、双包闭包独立可信验签、大u64原身份/原Sovereign、撤销后6维护→重启仍隔离→7维护→8安装→9实付、再重启同根。
- resolve后待激活包撤销，原提交锁拒绝；显式另换有效候选可续；健康世界不能进隔离、普通Tick不偷续、维护不能越Epoch、原能量闭包和幂等恢复保持。
- 生产SDK typecheck/build通过；修正一个既有测试未决Promise的类型为never，不改生产Provider行为。server/CLI锁定编译、必要格式/diff/敏感筛查；未改Store提交锁、未降fence。

```sh
pnpm --filter @rynvek/agent-sdk test
pnpm --filter @rynvek/agent-sdk typecheck
pnpm --filter @rynvek/agent-sdk build
cargo test -p rynvek-server --test general_authority --test private_rules --offline --locked -- --include-ignored
cargo test -p rynvek-cli --test private_rules --offline --locked
cargo test -p rynvek-simulation --test general_rules --test general_life --offline --locked
cargo test -p rynvek-protocol --lib --offline --locked
cargo test -p rynvek-server --lib private_rule_client --offline --locked
cargo test -p rynvek-server --lib bounded_response_tests --offline --locked
```

真实Node24.22.1/TS5.8.3，Node22约束有警告，不冒称Node22实证。仅MemoryStore+文件CAS/真实回环HTTP；PG仅类型编译，未做实际SQL/多机/OS断电/24h长稳。C#该流程、wasm32、真实browser硬件GPU、生产TLS客户端未交付/未验。仍为单Chunk Private所有者受限Tier0包组；General浏览器用户流、定向协作准入/自治调度、Agent独立赛道/真实provider、全规范跨种类锁/Schema迁移/组织信任、完整生命材料生长遗传、多Chunk/三维/任意World、presence/离线多主等主体仍欠。

旧6个PvP_Custom Actor夹具应补真实lease，不能降低生产fence。未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询；主体齐后集中完整门禁、安全/运行时及正式发布打磨，不以阶段52完成宣布整体完成。
