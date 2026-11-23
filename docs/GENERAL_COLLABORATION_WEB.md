<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# General 目的隔离协作 Web 闭环（阶段57）

2026-11-24，中国标准时间 UTC+8。从干净 PUBLIC `eeaac42` 接续；主体继续，整体未完成、未达正式发布质量。

## 本批选择与未交付内容

本批落实请求允许的必要协作 Web 闭环：把阶段56已有准入/SDK/Actor能力接到用户可操作的所有者与协作者流程，不重复造服务权限或仿真。

核对原 LabJob 的租约/代次/幂等治理、AgentRun 的控制面预算和最终设计 D337/D569–570 后，**没有**把实验计算工作者、Pi事件或客户端循环当作 General 耐久调度器。本批**未实现** General 持久运行意图、后台预算调度、作业暂停取消与进程重启恢复；它们仍是下一优先主体。旧“同 Store+文件CAS新App”的恢复用例只证明私有链功能，不升格为耐久进程恢复实证。

## 用户可用流程

### 所有者模式：显式管理目的

1. `/general` 默认原所有者模式，验证正式身份、读取本人受控世界。
2. 新“规则协作目的准入”面板明确提示：仅最小身体观察与已有规则驱动，可能消耗原源能量，不授源码/私包/安装/维护。
3. 输入规范完整 u64 主体字符串，主动勾选目的同意后才可授权。已有记录用列表中的当前 grant UUID 作 CAS；列表过期则原服务拒绝，不能自动改用新版本重试。
4. 仅列此目的，显示世界/主体/当前版本；撤销绑定精确当前版本，原服务产生墓碑新版本。不会发送邀请、自动分享或把令牌放到链接。

### 规则协作者模式：只得明确能力

1. 切换模式会卸载旧工作台，销毁旧凭据/私有态/Worker；不是把所有者会话或视图交给协作者。重新验证协作者本人身份。
2. 输入所有者明确提供的世界 ID 与当前目的版本，点击“绑定目的并只读”。只请求原 `/general/drive` 最小帧，不试探世界元数据、owner观察、源码或私库。
3. 连接、取得原 Sovereign 租约、驱动一个规则 Tick、交还控制各自明确操作；没有自动循环、重试、续租、安装或维护。
4. 成功推进必须同时满足 SDK 提交元数据和原 Engine 六字段帧校验，Tick/revision/root 一致才确认。新请求若返回 duplicate，不拿当前帧冒充旧回执。
5. 撤权/旧版本/旧fence失败清帧与双fence。重授权需用户明确绑定新版本，再连接/取得新控制；不自动替换准入或降级。

协作者不显示 owner 的生命账户、完整身体结构、组件、规则计量详情或私包信息。费用仍由原权威规则配置结算；有源测试世界正 EnergyQ 实付并守恒，不把 CPU 预算或 Tier1 配额说成生物费用/逐指令 Gas。

## 生命周期、最小表现与取消边界

- `GeneralWorldSession.openRuleDriver` 复用原正式身份验证、可信 origin、只驻内存凭据、redirect:error、no-store、15秒请求硬界及取消传输。返回目的适配器，不暴露 owner 的 `observation/rules`。
- 世界/准入草稿一变化立即清旧私有帧和双fence、停止旧显示；重新明确绑定前禁止对旧世界操作。不会一边显示新草稿一边驱动旧世界。
- 会话互斥及 epoch/AbortSignal 拒晚成功；断开、切模式、卸载、离线、隐藏清私有态，不被晚回执复活。
- 即使不启 GPU，最小帧也有12秒未复验即失鲜清态的一次性计时器，无永久私有静态缓存。它不发送网络请求、不推进 Tick。明确只读刷新或原 Worker 读帧成功才重置新鲜期。
- 画布继续使用原 `GeneralViewport→createGeneralRenderHost→Worker→FrameScene/Renderer`，只把会话契约收窄到 world/读帧/私有失效，不另造 Canvas 或前端世界。GPU 只有用户明确开启后才沿原5秒只读复验/12秒失鲜机制工作。
- **本地清态/断开不是服务端交还、取消或回滚**：末请求可能已提交，旧服务租约仍可能存在。应先明确交还，或所有者显式撤销目的再收回；既有 Tick 不撤销。不把关闭页面称“暂停耐久作业”。

## 原权威链不变

本批服务器生产 `src`、Store、Simulation、规则解析/安装/计价、ABAC/fog、宿主/控制双fence、准入/RulePermit提交锁与私有隔离零修改。复用阶段56唯一入口，不能凭前端成功绕服务授权，也没有新后台作业/外部模型/provider/支付调用。

## 必要实测

58个不同定向用例通过，新增11，重跑不累计：

| 范围 | 实际结果 |
| --- | --- |
| Web General/路由/viewport | 25（10新）：正式身份与最小流、角色互斥、明确同意/CAS/撤销、单Tick/交还、错根/额外owner字段拒绝、私有取消/晚回执、离线、草稿换世界和非GPU12秒失鲜 |
| server General | 19（1新真实协作Web控制器HTTP） |
| 原私库链/Studio协作 | 12+2，含原AST/SDK/Web ignored显式执行 |

新真实 Node22.23.5→Web控制器→原SDK→HTTP→原Actor链：所有者明确授权、协作者本人身份/只读绑定、连接/控制、三次真实规则Tick、原Engine帧校验、owner对账正费用/能量守恒、撤销拒绝清态、旧版本拒绝、明确新版本/新控制fence、Tick4与交还。测试还限制协作者所有请求只能是 auth/session 或此世界 general/drive，不能暗中取得owner完整视图。

这条新用例在同一个临时服务器实例执行，**不是浏览器、实际PG或耐久进程恢复测试**。旧恢复用例保留各自边界。生产TS类型/tsc与Vite JS/原Worker打包通过；Vite复用阶段55既有WASM，不代表新WASM构建/ABI复验，本批无WASM源码改动。

红测实修：React sibling 复用同一 world key 导致目的面板状态被重建，改为独立 drivers/render key；最小帧初次读取漏启动失鲜计时被新测试抓出并修复。另修 React19 useRef 显式初值类型问题。未删断言、未放宽服务fence或原帧校验器。

复现（使用现有 Node22/锁定依赖）：

```sh
npm --prefix apps/web-platform run typecheck
npm --prefix apps/web-platform test -- general_workspace general_driver_workspace general_viewport general_route_mount
apps/web-platform/node_modules/.bin/tsc -p apps/web-platform/tsconfig.build.json
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p rynvek-server --test general_authority --test private_rules --test studio_collaboration -- --include-ignored
```

跨工具链用例沿用已有 rules/agent-sdk/engine 前置构建，不把 ignored 算通过。JS/Worker生产打包在 apps/web-platform 目录运行 `vite build --config vite.config.mjs`；不声称这代替原标准 WASM 构建。

## 剩余主体与发布阻塞

第一优先仍是 General 耐久自治：显式持久预算/运行意图、原Actor服务调度、作业与世界提交同一最终锁内撤权/预算/进度、双fence、暂停取消、稳定幂等与真实独立进程恢复。不能只保存客户端计数或复用 Lab/Pi 名称冒称已做。协作私有世界发现/邀请投递也未做，本批仅明确世界/版本入口。

全平台 QuickJS(WASM)、C#规则链、浏览器签名注册登录/分页编辑发布UI、Agent真实provider及全规范World等仍欠；无外部模型/支付新授权。真实browser需已有获准可达部署，执行器无浏览器工具；不原样重试受管loopback，不代理/隧道或策略绕过，不擅部署私有数据。

全项目Node22矩阵、真实硬件GPU、实际PG迁移SQL并发/多机OS断电/24h长稳、C#本批重编与全workspace/Clippy/license/SBOM/完整安全发布门禁仍欠。旧6个PvP_Custom夹具仍须补真实lease，不能降生产fence。不以阶段结束标整体完成。
