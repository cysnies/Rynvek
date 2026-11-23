<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 私有规则签名包组与通用World显式在线安装

阶段51，2026-11-24，中国标准时间 UTC+8。**主体继续，项目整体未完成、未达正式发布质量。** 本段从干净 `ef19014a7be910f63fdef0ee516ac526648aa3e2` 接续，不重写阶段50 AST，不把本地创世嵌图冒充线上安装。

## 真实端到端链

真实TS5.8.3 AST → 原KernelBuilder/Graph → 原唯一RYPK/RYIR重编复验 → 同owner精确依赖签名发布 → 私有HTTP发现/下载并用登记公钥验签 → 人类Sovereign原双fence Actor显式安装 → 原下一Coupling Epoch实付Tick/Store事务 → 私有来源证据、快照/日志恢复同根。

使用原 `General_Circle_v1` / `General_Controlled` 单Chunk、Private所有者显式Tick入口。安装并不创建新的ACL、租约系统、解释器或提交架构；不声称任意World、任意TS、跨主体共享或自治服务器调度已经完成。

## 制品、签名与精确锁

- 阶段50 `rynvek.tier0-build.v1` 原RYPK叶子制品仍严格只有 `rule.json` / `graph.json` / `rule.rir`；没有额外TS附件或第二套包解析器。原 `Tier0RulePackage::parse` 重新编译实际图并逐字节比较IR，不执行作者JS。
- `rynvek-private-rule-release/1` 是**另外版本化且整体签名的发布信封**，不是给原叶子剖面偷塞未签锁。清单字段为 `format`、规范十进制字符串 `owner_id`、`package_id`、精确 `version`、小写 `artifact_cid`、按包ID严格递增的 `dependencies`。
- 每条直接依赖为 `{id, version, blake3_256}`，三项均精确；不接受范围、浮动版本、重复、自依赖、不同分支对同ID的版本/CID冲突、环、缺失或撤销。只能访问同owner的私库，不向公共Hub或外网回退。
- 每个直接锁最多8包、闭包最多16包/深度8、总制品最多1MiB；仍有原单图256KiB/World配置与图总量/Registry预算限制。签名安装证据最多64KiB、原私库每主体每种类256个不可变版本（撤销墓碑计入）；没有为本链放宽旧容器、图、权限或物理fence。
- 签名字节精确为：原 `api::hub::package_signing_message(id, version, artifact)`，追加零字节、ASCII `rynvek-private-rule-release/1`、零字节，再追加完整清单的固定JCS子集JSON。显式按ASCII键序编码（直接依赖键序 `blake3_256,id,version`；清单键序 `artifact_cid,dependencies,format,owner_id,package_id,version`），不受serde_json `preserve_order` 特性影响；所有值只用ASCII字符串/对象/数组，没有浮点整数。
- `publisher_key` 必须等于原principal登记公钥。`verify_download(owner, expected_key, release, bytes)` 同时验owner/制品摘要/Ed25519/包身份/原图IR；**expected_key必须来自登记身份或调用方既有可信钥，不得信任响应自带公钥即宣称身份认证**。每层信封自己的签名保护该层精确锁，原私库的owner+ID+版本永久不可覆盖；安装时整个实际闭包（含签名/锁/模块映射）CID进一步进入World根。

这是有界Tier0**显式共同安装规则包组**，不是JS模块import/link，也不是规范10.3全部跨种类 `dependencies.lock` / Schema迁移图/全信任级注册表已经完成。依赖不能仅下载作装饰：安装映射须一对一完整覆盖闭包，每个图都接到已有规则binding、经原Registry真实执行。没有冒称任意第三方规则协议兼容。

## 原私库与HTTP入口

复用原 `PrivateArtifactRow`、Active/登记公钥、不可变版本/配额、私有CAS、精确依赖与原Store锁。把原Agent/音频/视觉几乎相同的存取核心泛化为封闭 `PrivateArtifactKind`，旧接口/三张表/资料原位保留；仅加同机制 `0011_private_rules.sql`，不平行复制第四套ACL/事务算法。

| 路径 | 方法与实际权限 |
|---|---|
| `/api/v1/private-hub/rules` | GET，原WorldRead及Active，只列自己 |
| `/api/v1/private-hub/rules/{id}/{version}` | GET自己的签名元数据；PUT原HubPublish及Active签名发布；DELETE原HubPublish永久撤销 |
| 同版本 `/resolved` | GET，逐层复验同owner精确DAG/签名/CID，返回签名信封/pins，不执行、不安装 |
| 同版本 `/artifact` | GET，复验闭包后仅给私有制品原字节；下载方仍须用可信登记公钥复验 |
| `/api/v1/worlds/{world_id}/general` | 原POST，显式 `action: install`，其余仍为原connect/acquire/tick/close入口 |

发布请求为 `{artifact_b64, manifest, signature}`；路径、清单owner、容器ID/version、CID均须相同。请求最多1536KiB，并复用原私有制品复验并发名额；图/IR验证放在blocking任务。原私库锁内再次检查Active、公钥、整个闭包CID/撤销、配额和不可覆盖，包括墓碑。请求失败可能留下不可发现的私有CAS孤儿，但不产生版本、控制权或World提交。

发现、下载、观察均 `private, no-store` / `Vary: Authorization`，制品另加octet-stream/nosniff。其他主体404、未认证401、缺scope/已停用拒绝；不写原公共包索引/CAS，知道CID不能从公共Hub下载。元数据损坏不会降级返回一个貌似有效的空签名对象。

## 显式安装仍是原事务Tick

1. 所有者经原WorldConnect/WorldEdit、ABAC、Active与原Store editor准入；人类原Sovereign控制lease、宿主及控制双fence，每次仍复验。发布/下载不自动拿lease。
2. 请求 `action: install` 的 `input` 保持原 `host_fence` / `control_fence` / `envelope`，另显式带 `install: {root: {id,version,blake3_256}, modules: {包ID: 已有模块名}}`。精确下一Tick且 `target_tick % 4 == 0` 才允许；payload必须为空，不能同时混入物理命令，普通 `action: tick` 不接受偷带安装。
3. 服务器解析和验签完整闭包，映射每个包到不同的已有命名模块/binding，不创建任意组件、不覆盖Tier1。已安装包组只能整体替换，禁止只换根或某个依赖而保留与精确锁不符的执行图。
4. 原SimulationEngine构造隔离候选，只换规则图和来源引用；保留组件动态参数、生命来源账、身体、控制器、gas预算与价格，复用原Registry编译/类型/访问冲突/reconcile。**不会重置组件值、初始能量或重新摄食旧账**。
5. 紧接原 `tick_with_commands`，仍经原Phase07真实计量/正价格与有来源EnergyAccount热耗、整Tick失败原子性。纯CPU参数态不冒称生物能量，Tier1粗CPU额度仍不是逐指令gas；未增加物质、能量或免费运动旁路，也不是严格机械功模型。
6. 仅候选Tick的原RYC2/配置根、私有WAL写入CAS后，原TickCommitter统一发布；MemoryStore同一锁、PG原世界/主体/依赖行锁事务内同时复验editor/Active/fencing/前驱以及全部已安装pins的当前公钥/CID/撤销。**路由预检不代替这个最终线性化点**。PG仅类型检查，未实库证明。
7. 成功才切换Actor内存、APM与命令幂等候选；失败仍消耗原帧速率令牌，但不能提前占用已提交幂等回执。安装计一次原APM且从私有日志恢复；同幂等键不能换安装请求，重启回执仍须新有效双fence才可重取。

原 `world_revision == tick` 不改：安装不是另起不消耗Tick的配置修订。旧无安装World的空来源字段不编码、原配置根字节保持；原v1 Tick日志仍读，安装事件为严格v2，不能把可选字段混入v1伪装旧日志。

## 私有恢复与撤销

每个安装包组的规范证据含owner、显式根/模块映射和全部实际签名清单，存原私有CAS；模块→证据CID进入原 `GeneralRulesState` / 配置根 / 快照。v2日志保存原输入及服务器从已验签制品提取的图/证据CID，不接受客户端自报图代替下载制品。

恢复先验原锚点/提交链/CID/领域根，再对历史安装证据及实际制品重验登记公钥、签名、精确DAG、图和模块映射；原重仿真得到完全相同的RYC2。300Tick锚点和后缀、1800Tick回执/APM窗口仍沿原机制。不能只保最后内存镜像或把软件替身称实库断电证据。

**撤销不抹除已提交的签名历史**：恢复历史不以当前revoked拒绝；后续活执行/下载仍fail closed，原Store最终提交守卫拒绝撤销后再发布。当前策略是立即停止含撤销包的后续Tick，不做静默卸载/回滚/恢复旧包；若当前不是下一Coupling Epoch，尚无安全的暂停后换装推进/迁移产品流程，不能声称撤销后任意时点可直接续跑。历史观察仍须原所有者准入，不公开日志/来源证据。

## 必要快验与真实未验

46个不同定向用例通过，新增7，重跑不累计：

- 私有规则新7（含**显式执行**缺省ignore隔离Node/dist前置的真实AST链）：双签名依赖发布/发现/下载验签、整包组安装、原实付位移/有源闭包、301Tick含300锚点后缀/新App文件CAS恢复同根、同幂等回执、撤销后历史可读而后续拒绝；锁篡改/错钥/制品篡改/owner隔离/scope/Active/墓碑、精确菱形冲突/环/深度、Epoch/旧双fence/不全映射/部分包组替换、真实欠费整Tick回滚、resolve后撤销在原Store提交锁再拒绝。
- 原General权限9；原Agent2/音频3/视觉2（合7，另一Agent跨SDK用例仍缺省ignore，**未计入**）；原仿真规则10/生命8；原命令闸门5。
- 必要原生server/CLI锁定编译、PG代码类型、改动格式/diff及必要敏感模式筛查；不是全workspace/Clippy/完整安全或发布门禁。

```sh
cargo test -p rynvek-server --test private_rules --offline --locked -- --include-ignored
cargo test -p rynvek-server --test general_authority --test private_agents --test private_audio --test private_visual --offline --locked
cargo test -p rynvek-simulation --test general_rules --test general_life --offline --locked
cargo test -p rynvek-security --lib anticheat --offline --locked
cargo check -p rynvek-server -p rynvek-cli --offline --locked
```

实际Node24.22.1 / 原TS5.8.3前端，不是Node22实证。真实回环HTTP+Ed25519+MemoryStore+文件CAS，不是PG并发SQL/跨机/OS断电。Node22、C#本批重编、wasm32、真实browser硬件GPU、PG多机断电/24h长稳均未验；旧6个PvP_Custom Actor夹具仍应补真实lease前提，不降低生产fence。

## 剩余主体/发布阻塞

当前没有私有规则发布/安装的专用CLI/TS/C#客户端工作流或General浏览器流UI，没有全规范跨种类锁文件/Schema迁移/信任级/组织共享，也没有撤销后非Epoch安全续跑流程。源证据长期保留/分页/吞吐和高并发尚未认证。General通用协作准入与统一调度、完整生命材料/生长/自然遗传迁入、多Chunk/三维/任意World、Agent独立赛道、presence/离线多主等主体仍欠。主体齐后集中完整门禁、安全/运行时与正式发布打磨，不以阶段51结束宣布整体完成。
