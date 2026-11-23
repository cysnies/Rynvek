<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# General权威Web操作工作台（阶段53）

从干净 `e4954793989102a71f02bc47e417c4b5c8d61342` 接续，2026-11-24，中国标准时间UTC+8。**主体继续，整体未完成、未达正式发布质量。** 当前是HTTP权威操作/观察，不是通用World GPU画布或任意World完成。

## 实际用户链

- `/general` 真实React挂载，Arena顶栏有入口；首次进入不发请求、不启练习身份、不造世界、不加载本地EngineWorker。沿原四个工作台另加General辅助入口，原Arena/Studio/Lab流程保留。
- 用户提供自己**已有原Ed25519挑战登录取得的能力令牌**；本批没有重做注册/签名登录、没有生成身份或索取/上传私钥。当前UI固定同origin；可编程客户端只接受可信HTTPS origin或明确本机HTTP开发地址。令牌仅内存，Authorization发送，redirect:error/credentials:omit/no-store，15秒请求界限，离开/断开中止请求、清状态，往返缓存重新验证。
- 新只读 `GET /api/v1/auth/session` 复用原authenticate/subject/Active检查，返回本人正式主体字符串/类型/登记公钥，**不返回token**；无认证/坏token拒绝，Deleted拒绝。不是第二套权限系统。私有成功响应no-store/Vary Authorization；原GET worlds同补私有缓存头。
- 正式身份来自服务端，不让客户端填主体ID自报成功；General仅接受Human。SDK信任锚取自用户选定可信服务的本人登记钥，**不是包自带公钥**。这依赖已信任部署/TLS；不主张可防恶意服务替换信任锚。
- 发现原worlds分页首100条元数据，筛本人General_Circle_v1/General_Controlled；有更多时支持精确世界ID，控制器有offset参数，UI尚未分页。上传自己明确准备的1..128KiB RYWL包，唯一Rust解析/编译/创建；不是前端JSON假创世。不复制包解析器。
- 读取世界先原WorldRead元数据确认本人/profile，再原General观察；**读取不连接/不取lease**。明确连接宿主→明确取得Sovereign→每次一个原Actor实付Tick。宿主/控制双fence来自真实回执；不自动续租或重试、不后台自治推进。
- 原PrivateRuleClient发现版本元数据→用户填写root精确引用及完整modules计划→独立按可信签名锁下载验签→明确Epoch安装。单独“验签”不声称已安装，SDK/原服务器安装时仍复验来源/撤销。
- 撤销后下一Tick仍拒绝，失败清本地观察和双fence；用户先刷新→重连/取得控制→明确接受每活控制身体维护报价→一个隔离维护Tick。目标Epoch/实付维护状态按原观察显示，到Epoch须再明确安装。语义及有源正价全Tier0有活控制身体限制完全沿阶段52，不弱化提交锁撤销或健康World不能廉价跳规则的保护。
- UI仅展示服务器提交回执中的Tick/revision/root/身体/组件/生命/计量；回执必须匹配world/剖面/根且Tick精确+1。整数超JS安全范围拒绝显示而非截断；不是全部u64观察格式支持。计划上限64KiB，响应1MiB，不渲染HTML。
- POST结果不确定时绝不显示成功、不自动重试；创建失败先重新发现，防重复创建；推进失败先权威刷新再取得新控制。断开不撤销服务端已提交Tick或删除世界；当前无UI release/close按钮，原租约会过期。

## 本批必要验证

58个不同定向用例通过，新增10，重跑不累计：Web DOM/路由12（新增General DOM/控制器7和真实路由1）、原SDK25、General权限10（新增身份Active/缓存1）、私有规则11（新增真实Web控制器HTTP链1）。原跨Node AST/SDK/Web用例显式include-ignored，不把跳过算通过。

真实Node运行生产编译出的GeneralWorldSession→原Rust HTTP：正式大u64身份读回、RYWL真实创建/发现/选择、原连接/双fence/Sovereign、逐Tick到3、可信规则安装4/实付5、撤销拒绝→6维护→新会话仍隔离→7维护/8安装/9实付；再新App+同MemoryStore/私有文件CAS恢复同根。不是浏览器DOM驱动真实服务器，不是PG或断电实证。

Web typecheck、`tsc -p tsconfig.build.json`、直接Vite生产打包通过。**直接Vite复用了仓库已有wasm制品，没有运行build-wasm或本批wasm32重编**；当前General路由并不加载该制品。实际Node24.22.1/TS5.8.3，根Node22 engine警告保留。必要原生server编译、diff/敏感筛查通过，不是完整发布门禁。

```sh
pnpm --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/web-platform exec tsc -p tsconfig.build.json
pnpm --filter @rynvek/web-platform exec vite build --config vite.config.mjs
pnpm --filter @rynvek/web-platform exec vitest run test/general_workspace.test.tsx test/general_route_mount.test.ts test/web_platform.test.ts test/lab_route_mount.test.ts test/studio_route_mount.test.ts test/browser_mount.test.ts
pnpm --filter @rynvek/agent-sdk test
cargo test -p rynvek-server --test private_rules --test general_authority --offline --locked -- --include-ignored
```

## 父代理后续实际浏览器验收清单（本批未做）

本执行器没有浏览器工具。**不要再次尝试既往已拦截的受管loopback目标，不用代理/隧道/内部API绕过，不原样重试**。仅当父代理已有获准可达部署时，按正常受管浏览器验证：

1. `/general` 不自动联网/造世界；用已有正式会话验证身份，确认主体及登记钥，非法/Deleted/Agent身份拒绝。
2. 用自己准备的RYWL包明确创建，发现/精确ID读取；确认读取不抢宿主，连接/取控制分别显式。
3. 单步实付及根更新；错误/撤权/旧fence无假成功。规则发现、完整计划、可信下载、Epoch安装。
4. 从现有撤销管理入口撤销后确认普通Tick失败；刷新重连后逐次明确维护报价到Epoch换装；刷新/换页仍还原隔离状态。
5. pagehide/往返缓存不保留凭据/旧观察；网络不确定不自动重试。检查真实HTTP、DOM、键盘/文件选择、无凭据日志/存储。

## 真实剩余主体/发布阻塞

- **原Renderer/Worker的远程General只读观察桥接尚未接入**。规范7.1要求Worker独占GPU，不应把服务端同包载入本地Worker运行第二份World、借Arena faction/fog假投影或另写假世界/Canvas替身。本批有意义闭环是原SDK/权限Actor用户操作与观察，下一优先补原渲染管线的真实远程General剖面；真实浏览器/GPU另验。
- 浏览器直接Ed25519登录/注册体验、发现分页、规则映射编辑器/发布UI、General协作主体准入与自治调度仍欠；同origin部署/CORS/生产TLS需真实验证，不放宽浏览器/网络策略。
- Agent独立赛道/真实provider仍欠；无新外部模型/支付授权，未调用外部模型、未发送私有内容。
- C#规则消费、全规范跨种类锁/Schema迁移/组织信任、来源长期保留、完整生命材料生长遗传、多Chunk/三维/任意World、presence/离线多主仍欠。
- Node22、C#本批重编、wasm32、真实browser硬件GPU、实际PG并发SQL多机OS断电24h长稳仍未验。旧6个PvP_Custom夹具须补真实lease，禁止降生产fence。
- 未跑全workspace/完整Web集/Clippy/license/SBOM/完整安全/soak/CI轮询；主体齐后集中安全/运行时/正式发布门禁，不宣布整体完成。

## 阶段54追加：原Worker远程桥接代码与构建阻塞

已有最小权威帧→原EntityProvider/FrameScene/Renderer及显式只读画布生命周期，详见[GENERAL_REMOTE_RENDERER.md](GENERAL_REMOTE_RENDERER.md)。上文“尚未桥接”保留阶段53历史；本箱新wasm32因缺clang构建失败，旧产物会被General能力握手主动拒显；不能将原生软件回读/TS打包称浏览器可用或硬件验收。

## 阶段55续：新WASM已真实重构与有界运行

阶段54缺clang源于未恢复已有官方SDK环境，本批复核原归档并正常构建新wasm32/胶水、真实General ABI与Worker打包通过，不再复用旧WASM。另有明确有界逐Tick实付运行/停止；非耐久自治或协作准入。浏览器本地Tier1当前明确整组拒绝，原生权威Tier1及其远程身体投影保持，不能称规范QuickJS(WASM)已完成。完整证据、Node22相关快验和剩余真实浏览器/硬件/PG欠项见[GENERAL_WASM_BOUNDED_RUN.md](GENERAL_WASM_BOUNDED_RUN.md)；上文阶段53/54未验口径保留为历史。
