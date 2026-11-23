<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# General：显式有界耐久运行意图与原Actor（阶段58）

从干净 `02fab1473990242c3c5966e874f1c53bf946fef7` 接续。阶段57协作Web不重做。
本批交付服务器主体（`bc15da6`），定向HTTP/私包撤销/真实PG子进程实证为 `532d3dc`，不宣称整个项目已达发布质量。研究了最终讨论D337的可恢复任务/过期命令/不重复应用，以及D570长期任务撤权边界；复用原Lab的Claim、OwnerAction、WorkerAction、30秒作业租约、5次故障认领及8活动/512历史上限，不把Lab实验或Pi事件当General权威执行。

## 用户授权与入口

只有当前正式Human所有者，在原WorldConnect/WorldEdit及ABAC通过、已有Sovereign控制双fence有效时，才能向原 `POST /api/v1/worlds/{world_id}/general` 提交 `action: start_run`。
`host_fence`、`control_fence` 是原租约值；`spec` 明确包含下列字段，禁止未知字段：

| 字段 | 约束与含义 |
| --- | --- |
| submission_id | 规范小写连字符UUIDv7；同owner唯一，精确规格冲突拒绝，不自动换键重试 |
| expected_revision / expected_root | 原观察的精确修订与64位小写十六进制根，不用过期起点暗启 |
| ticks | 1..128个原规则Tick，不是CPU指令gas，也不是无限运行 |
| wall_seconds | 10..900秒，自Store启动线性化时起，包括排队、暂停及故障等待；恢复不延长 |
| interval_ms | 200..5000毫秒调度节流，不改变世界的确定性时间/生物能价 |
| max_energy_q | 规范正整数字符串，至多1000000000000 EnergyQ；实际费用沿原有源计价 |
| consent | 必须精确为 `run_installed_rules_with_bounded_budget`，手动drive许可不暗含此同意 |

只运行已有生命来源且声明正生物费用的已装规则，拒无规则物理推进、CPU-only配额和维护态。
不新增神经/provider/模型调用、支付、邀请或私有外发。正常人类控制租约在意图持久化成功后交还，后台取得原LeaseManager的新控制fence；不重置计数或偷抢另一人的有效控制。

回执直接是GeneralRun，包含ID、世界、精确规格、`storage_backend`、进度、已实付能量、deadline、状态、generation及作业fence。
`storage_backend: memory` 只代表同进程治理，绝不是跨进程耐久；PG新启动还必须配置私有文件CAS目录，不能只把计数写PG而丢失世界日志。
持久意图不是保存Bearer：原令牌到期不取消已经明确授权的有界任务；撤回请用cancel。每步仍重新检查当前身份状态、所有权、原ABAC/目的与精确规则依赖。

## 对账、暂停、取消与撤权

- `GET /api/v1/worlds/{world_id}/general/runs?submission_id={原提交键}`：失回执只读对账，无宿主认领、无Tick。
- `GET /api/v1/worlds/{world_id}/general/runs/{run_id}`：私有当前记录，只有所有者可读，不给driver全知观察或私有包。
- 对同一run URL `POST` 原OwnerAction：`operation_id` UUIDv7、`generation`、`action: pause|resume|cancel`。控制代次不随每Tick进度变化，不必与后台抢一个瞬时进度版本。
- 操作键重复返回当前记录，同键不同动作/代次拒绝，最多保留64个原控制收据，不驱逐旧收据来放行重用。提交键与运行步号由Store持久保存。
- pause/cancel在同一最终世界锁中增加作业fence并去租约，返回之后旧候选不能再提交；先赢得锁的Tick可以在暂停返回前提交，已付历史不回滚。
- 暂停/blocked仍保留精确世界起点，普通Tick/安装/换装不可混入。resume须有剩余原预算、deadline未过且根/进度相符；不会跳Epoch或重置已付费用。
- 暂停过原deadline后恢复必拒绝，可cancel释放保留；不会因暂停永久暗耗计算。取消是终止而非重试；新执行须新意图/再次明确同意。
- 包/依赖撤销、主体禁用、费用不足或规则失败都封闭本次运行，不跳过VM、不自动降级；撤包后可先cancel，再沿原显式维护换装流程启动新意图。

## 唯一原子提交链

新增的是General运行领域记录，不是新的身份/权限体系，也不是另一份世界仿真。
服务器启动一个有限串行后台槽，Weak持有App，轮询Store中的明确queued/过期running意图。
PG用 `FOR UPDATE SKIP LOCKED` 认领作业，续用原Claim的node/fence与30秒租约；故障认领最多5次，超过后blocked须明确恢复。普通启动不会给未提交意图的世界自动计算。

每步：当前Run claim与原身份准入 → 原General宿主/恢复 → 原LeaseManager Sovereign控制fence → 原CommandGate → 原空规则命令Tick → 原SimulationEngine有源收费候选 → 原私有CAS日志/周期锚点 → 原TickCommitter/Store。
步幂等键由run UUIDv7与步号确定，不因重领/恢复生成新的同一步ID；`/5`私有Tick日志记录run/步号/job fence，旧`/1..4`语义不放宽。

Memory同一表锁先验完所有可失败条件再同时发布；PG同一事务保原世界行锁、身份/目的/host fence/精确RulePermit锁及前驱守卫，再检查作业node/fence/租约/deadline/步号/累计预算，把运行进度、费用、根、WorldRow与CommitRow一起提交。
任一步失败只遗留可能的未引用CAS候选，不发布免费世界，也不先计进度或重复扣费。规则和准入撤销仍在原最终锁复验。

停机等待当前Actor请求结束，再以原WorkerAction::Release撤销claim并重新排队；崩溃没有这一步，必须等待旧作业/宿主租约失效。
即使宿主属于本节点，也不绕过原健康30秒窗口重置控制fence。远端健康宿主不抢占/不转发凭据；等待可能耗尽短墙钟预算，单节点实证不能泛化成多节点调度性能结论。

## 已执行的必要定向实证

42个不同必要定向通过，新增9；重跑不累计，辅助子进程入口不计业务用例。实际Node22.23.5，非全项目矩阵；新代码格式/diff及本批高置信敏感特征筛查通过，非完整安全审计。

- 新状态机3：严格consent/上限/UUIDv7与步幂等；暂停恢复不延原预算、旧claim失效；操作冲突/晚失败不能覆盖取消。
- 新真实HTTP后台4（Memory明确只进程内）：无意图不自启；后台四Tick根/实付费用与原Engine直接确定性对账；失回执查询/重复提交与冲突；目的driver不能启动；pause/resume/cancel、旧候选最终锁拒绝；能量预算不足不发布世界/费用/进度；主体禁用后提交锁撤权。
- 新已签名私有包组实证1：原安装Tick4 → 耐久后台正费用Tick → 精确依赖撤销 → 后台blocked，撤销返回后根/进度不再推进，仍可cancel；不是绕过原撤销/维护流程。
- 新真实PG独立进程恢复1：官方Debian可信源的PostgreSQL17.12，独立合成测试库，首次DDL/扩展至0013及重复启动迁移实际执行；只开0700私有Unix套接字，禁TCP，不读取原用户数据库或凭据。
- PG测试父进程只轮询对账，子进程运行真实服务/后台/Actor：先实付、暂停恢复，第三步后SIGKILL整个服务OS进程；新的独立OS进程从同一PG+私有文件CAS认领、恢复原日志并自动完成8步。原健康窗口不重置fence，最终原Engine同根、160 EnergyQ、仅9个含genesis提交；重复原提交键不再推进。另以独立父PG连接验证pause后旧候选最终锁拒绝、世界/进度都不变，再取消。
- 原General19、私库原12、Studio2与原Node22 AST/SDK/Web HTTP链定向回归，保已有协作目的/fog/有源事务/双fence与提交锁保护。没有全workspace/完整CI泛化。

复现（只用隔离合成测试数据库；勿指向实际用户库）：

```sh
export PATH=/workspace/rynvek-tools/node22/node-v22.23.5-linux-x64/bin:$PATH
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline -p rynvek-server --lib general_runs
# RYNVEK_TEST_DATABASE_URL必须由操作者明确提供隔离测试库；PG案例是ignored，以下显式执行。
cargo test --locked --offline -p rynvek-server --test general_durable --test general_authority --test private_rules --test studio_collaboration -- --include-ignored
```

## 未验与剩余边界

本批实证的是同机PG17 + 两个先后独立服务进程的SIGKILL恢复；不是PG进程断电、OS断电、多机网络分区、迁移并发压力或24小时长稳。新PG测试保留合成私有CAS与库，不删除原资料。
只开放所有者后台意图，没有把general_rule_drive授权升级为协作者耐久委托。SDK/CLI/管理Web的此功能产品封装、队列公平/多节点宿主亲和与更精细错误诊断仍须完善；不是本批改选回已完成协作Web。
全平台QuickJS(WASM)、C#、真实browser/硬件GPU、完整Node22矩阵、PG多机断电长稳、组织聚合配额/信任、Schema迁移/来源长期保留和全规范World仍欠。完整安全/Clippy/license/SBOM/发布门禁待主体齐后集中，整体未完成。
