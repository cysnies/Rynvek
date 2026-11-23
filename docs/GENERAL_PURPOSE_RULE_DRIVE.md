<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# General 按目的隔离的规则协作驱动（阶段56）

2026-11-24，中国标准时间 UTC+8。从干净 PUBLIC `bb6beb9` 接续。主体继续，整体未完成、未达正式发布质量；没有重做阶段55的新 WASM。

## 本批真实链与边界

所有者明确同意目的 → 原准入表持久 CAS 版本 → 正式人类身份及原 scopes → 最小身体帧 → 原 General Actor/LeaseManager/CommandGate → 原有规则与费用 → 原 TickCommitter/Store 提交锁复验 → 私有 WAL/快照恢复 → 相同目的版本的幂等回执。

- 只覆盖 Private `General_Circle_v1 / General_Controlled / D0_Strict` 的显式人类规则协作者，不是任意 World、组织共享或通用 Agent 准入。
- 协作者不取得所有者的 `/general` 完整观察、世界元数据/提交列表、创作源码、私有包下载/发现权限。单独取得 Studio 源码许可也不会成为规则驱动许可。
- 规则驱动仅允许显式 Connect、Sovereign Acquire、Release、空载荷 Tick；禁止 Close、安装、撤销维护、Despawn 及任何外部结构/物理命令。无已配置规则的纯物理世界不可借此目的推进。
- 费用沿用原世界配置，不引入第二套计价：有生命计价的测试世界实付 EnergyQ、正费用且宇宙能量守恒；仅 CPU 预算的规则世界不伪称生物能量结算，Tier1 配额不冒称逐指令 Gas。规则撤销仍在原提交锁阻断；维护/换装只能由所有者走既有显式机制。
- 本批只有 API/TS SDK 用户流及原 Engine 帧校验的真实 HTTP 链，没有协作 Web 面板/邀请投递/共享世界发现；没有后台耐久自治作业。阶段55在线有界循环仍不叫耐久自治。

## 目的准入复用与撤销

复用原 `EditorGrant/EditorPermit`、MemoryStore 锁及 PostgreSQL 世界行锁，不开第二套授权仓库。

| 项目 | 规则 |
| --- | --- |
| 持久目的 | `studio_source_edit=0`；`general_rule_drive=1`，未知码拒绝 |
| 持久键 | `(world_id, principal_id, purpose)`；原表迁移0012，旧数据默认只归 Studio |
| 显式同意 | `general-installed-rule-drive/1`，不能用 `studio-source-edit/1` 替代 |
| CAS | 初次 `expected_grant_id:null`，后续必须精确当前版本；撤销也更新 UUIDv7，保留当前墓碑、不改写旧 Tick 历史 |
| 上限 | 每世界每目的 16 个 active、64 个含撤销记录，两个目的的列表/配额/CAS 独立 |
| 当前身份 | 所有者和接收者 Active，General 接收者必须正式 Human；原 WorldConnect/Read/Edit scopes 不被 grant 替代 |
| 最终授权 | 原 `append_commit_guarded` 同一提交锁再次核身份、Private 世界剖面、目的和精确版本；保原宿主/控制双 fence 及 RulePermit |

Actor 将协作者控制租约绑定到 `(principal, grant版本, control_fence)`。撤销或重授权后，下一次有效 Actor 请求使用原 `LeaseManager.release` 清失效控制，不重置递增计数；新版本必须重新 Acquire。所有者不能偷抢仍有效的人类控制租约，须先显式撤销。内部存储错误停止处理，不能当作普通撤权降保护。

## HTTP 与 SDK

所有者 `GET/POST /api/v1/worlds/{world}/general/drivers` 列表/变更此目的；协作者 `POST /api/v1/worlds/{world}/general/drive` 二选一：

```json
{"grant_id":"当前UUIDv7版本","frame":true}
```

```json
{"grant_id":"当前UUIDv7版本","command":{"action":"connect"}}
```

控制动作继续使用原 Action/GeneralInput/CommandEnvelope，并未另起仿真协议。请求上限64KiB，拒未知字段；驱动成功和失败均 `private, no-store`，错误保稳定码、不向协作者回显私有恢复/规则细节。

`frame:true` 只从原私有链恢复并在回包前复验准入，不认领宿主、不取得控制、不推进 Tick。仅传六字段帧及 serial/Q32位置/半径，不传账户、完整快照或规则源码。

```ts
import { changeGeneralDriver, GeneralDriverClient } from "@rynvek/agent-sdk";
// ownerRequest/driverRequest 分别沿用原已认证传输；不能把所有者令牌交给协作者。
const grant = await changeGeneralDriver(ownerRequest, world, driverId, null, true);
const driver = new GeneralDriverClient(driverId, world, grant.grant_id, driverRequest);
const { hostFence, frame } = await driver.connect();
// SDK保持frame为unknown；显示前交原Engine唯一校验器。
validateGeneralFrame(frame, world.replaceAll("-", ""));
const controlFence = await driver.acquire(hostFence);
// 调用方显式提供本次UUIDv7及当前已确认Tick，不后台循环/重试/续租。
const receipt = await driver.tick({ world, hostFence, controlFence, tick, idempotencyKey });
```

SDK 保留完整 u64 身份字符串，复用原受限响应读取器和 General CommandEnvelope 编码；不复制 Engine 帧解析器。拒错 Tick/revision/root、额外 owner 字段、错主体/控制域或不精确整数。无外部 provider、支付或邀请消息调用。

## 私有恢复与幂等

- 旧 owner `/1 /2 /3` 日志仍要求 owner 身份；新 `/4` 只接受带目的版本的非 owner 空规则驱动。旧格式不能偷带目的版本，`/4` 不能抹版本、带结构命令/安装/维护。
- 新记录把当前 grant UUID 写入私有 WAL；旧 owner 记录的新增字段为 None 且不序列化，保原字节。真实规则、费用、提交清单和所有领域根仍按原重演验证。
- 历史 grant 只是当时已在提交锁确认的出处，不是当前权限；撤销后所有者仍能恢复合法历史，协作者每次访问仍须当前准入。
- 重启恢复使用新的宿主 fence 与显式新控制租约；同一 grant 的同键同信封可取旧回执、不再扣费/推进。重授权后的新 grant 不能重用旧 key 的授权/回执。
- 驱动 duplicate 仅返回原提交元数据，不把当前身体帧冒充当时帧；新成功 Tick 才带对应权威帧。不改变 owner 既有回执格式。

## 必要验证与未验证

本批通过的不同定向用例：General HTTP18（7新增，含真实 Node SDK）、私有规则12（1新增）、原 Studio 协作2、SDK33（8新增）、Store3（1新增），合计68，新增17；重跑不重复累计。

- 六条新增 Rust General 场景覆盖双目的共存/CAS/列表隔离、最小观察与真实有源位移/付费、禁止动作/伪造主体、scope/Human/错世界、锁内撤权及 owner 收回、重授权双 fence/旧 key、同 Store+文件 CAS 新 App 恢复。
- 新私包场景：owner 安装真实签名精确依赖闭包后交出原租约，driver 真正消费已有规则；仍不能下载 owner 包；撤包后 driver 失败、根不变，重启历史可恢复。
- 真实 Node22.23.5→Rust HTTP：u64身份、显式目的授权、最小帧经原 Engine validator、三 Tick 实付守恒、重启同根、旧 host 拒绝、新双 fence 同键幂等、撤销/重授权、旧控制/旧 key 拒绝与新 Tick4 正常。不是真实浏览器。
- 初次实测发现 owner-only 旧日志检查阻断新协作恢复，已用显式 `/4` 区分、保旧格式严格检查；Node 测试首用连字符世界 ID 与原 Frame32hex 不符，改正确协议边界，未放宽原校验器。旧私包/Store 夹具绑定真实 General 目的，未弱化 fence。
- 相关 TS 类型/SDK和 Engine 构建、Rust 锁定离线构建/必要格式/diff 与敏感特征筛查通过。PostgreSQL 迁移/SQL 仅编译和人工核对，**没有实际 PG 执行证据**。未重建 WASM/重跑全 CI。

复现（先使用已安装 Node22，保原工具链及锁，不需要新增依赖）：

```sh
npm --prefix packages/agent-sdk run typecheck
npm --prefix packages/agent-sdk run build
npm --prefix packages/engine run build
npm --prefix packages/agent-sdk test
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p rynvek-server --test general_authority --test private_rules --test studio_collaboration -- --include-ignored
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p rynvek-server --lib store::
```

`--include-ignored` 还会真实运行已有 AST/SDK/Web 跨工具链用例，须保留阶段55已构建的 rules 与 web-platform 前置；不将跳过算通过。

下一主体仍有 General 耐久自治及协作 Web/发现/邀请产品流、全平台 QuickJS(WASM)、浏览器签名登录/分页/编辑发布 UI、Agent/C#规则链、组织信任/Schema迁移/来源长期保留、完整生命材料生长遗传、多Chunk/三维/任意World/presence/离线多主等。全项目 Node22 矩阵、C#本批重编、真实 browser/硬件 GPU、实际 PG SQL并发/多机OS断电/24h长稳、完整 workspace/Clippy/license/SBOM/安全发布门禁仍欠。

执行器没有浏览器工具，未尝试部署或受管 loopback；父代理仅在已有获准可达部署验证原 Worker/OffscreenCanvas/WebGPU、真实私有清屏/撤权/断线/换世界与停止末请求确认。禁止代理/隧道或策略绕过，不擅公开私有数据。阶段54 llvmpipe软件GPU与本批MemoryStore+文件CAS证据不冒充真实浏览器/PG。
