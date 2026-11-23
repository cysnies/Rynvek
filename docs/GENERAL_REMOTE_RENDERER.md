<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# General远程最小观察→原Worker/FrameScene/Renderer（阶段54）

2026-11-24，中国标准时间UTC+8，从干净 `a1be0c3fb2271a0930b467fc680b59ae52db2948` 接续。**整体未完成、未达正式发布质量。** 本批交付Rust/TS桥接主体、真实HTTP/原生软件渲染回读；**新wasm32二进制未构建成功，不能声称浏览器已经可显示新General帧或完成硬件验收**。

## 唯一真实链

原General权限Actor/耐久恢复 → `GET /api/v1/worlds/{id}/general/frame` → 原会话认证fetch → 最小 `GeneralRenderFrame` → 原EngineHost/Dedicated EngineWorker → 原Rust FrameScene/EntityProvider圆体生成 → 原WorldRenderer。不是第二份SimulationEngine权威、不载同包本地推演、不假扮Arena faction、不新造Canvas2D/SVG渲染器。

- 新GET复用原permit/ABAC/Active/Private所有者WorldRead+WorldConnect，耐久restore后再次准入；不抢宿主、不取控制lease、不提交Tick、不扣生物能量。其他人404、无认证401，成功private,no-store/Vary Authorization。
- DTO顶层严格六字段：format/worldId/tick/revision/stateRoot/bodies。每体仅serial/positionQ32/radiusQ32，来自实际Physics ECS；按原serial排序，最多原128体。**没有规则源码、组件动态态、完整私有快照、账户、场网格、能力令牌或基因**。Worker仅收到该白名单投影，原完整观察留在原所有者操作入口而不是送给Worker。
- 复用原EntityProvider的同一圆体实例函数（位置Q32.32→f32、直径=2×真实半径、原serial/pickID），原FrameScene/WorldRenderer不平行重写。圆外可保留原Renderer坐标网格，不将它称权威地形/生命场。根由原耐久权威恢复生成；客户端绑定/格式检查不是重算完整世界根的密码学证明。
- 初始化显式generalRemote:true，与arenaFaction互斥。Rust/TS握手必须回显General能力且所需后端确为WebGPU；旧WASM忽略新字段时，在任何帧调度前关闭，绝不伪造“就绪”。原本地/竞技场缺省线上编码保持，跨语言共享夹具覆盖新增GENERAL_FRAME及GENERAL_FRAME_APPLIED。
- 原Worker严格验证世界、Tick/revision、根格式、真实物理半径/坐标界、唯一有序serial、精确字段集合；旧Tick/同Tick不同内容拒绝。坏帧清空私有投影并封闭该远程Worker，晚到帧不能复活，须重新创建Worker/复验授权。
- 远程General禁止STEP/START/GENERAL_WORLD和Arena混用；无本地积分、外推、摄食或规则执行，也不生成TICK_COMMITTED。只读渲染不会给原世界造物质或能量。

## Web生命周期

`/general`选择已授权世界后，用户另按“明确开启WebGPU只读显示”。真实canvas只移交原Worker一次，每次重新开启/换世界新canvas和Worker；不自动回退Canvas2D。元数据根匹配的GENERAL_FRAME_APPLIED只显示“已接收、待GPU”，随后原FRAME_RENDERED才显示“WebGPU已提交”，不把消息确认称硬件浏览器验收。

开启期间每5秒只读最小帧，不调用任何Tick/租约/安装。请求失败/撤权/离线/隐藏/换世界/退出/坏帧即隐藏并卸载画布、终止Worker、中止请求、清本地观察/双fence/包清单/计划；晚回执不复活。握手12秒超时、最近一次授权投影超过12秒未复验也清除。**撤权不是瞬时服务端推送：下次复验失败清除，最迟12秒失鲜清除；不承诺追回已经看到的信息或物理内存取证擦除。** 无后台心跳替用户续控制租约，停止显示不暂停/回滚已提交世界。

## 已验与明确失败

130个不同定向用例通过，新增10，重跑不累计：TS engine54（General Host2、旧WASM能力拒绝1新增）；Web DOM/路由15（生命周期3新增）；Rust engine lib17（General2新增）、wasm原生host lib6、跨语言协议2、Arena表现13、General软件回读1新增；server General11（最小帧权限/原场景1新增）/私库11。

- 真实HTTP最小帧与原提交根/身体一致，无宿主认领，无额外私有字段，原生Engine通过线上JSON进入场景且本地Tick仍0；非本人/Deleted拒绝。
- 阶段53真实Node General用户链新增读取最小投影并校验Tick9/root，原签名安装/撤销维护/私有恢复整链仍显式执行。
- 原生真实wgpu回读：**llvmpipe LLVM19.1.7 / GL / Cpu / TierC**，圆中心/半径内外像素、坏帧后空场景和背景像素、无GPU未捕获错误；不是硬件GPU/浏览器。Arena原13用例含fog/隐敌/地图/主题软件像素均保持。
- TS类型/生产模块编译、直接Vite打包、Rust原生检查/格式/diff及必要敏感筛查通过。直接Vite复用旧wasm，只证明JS打包，不证明新WASM可用。
- 实际Node24.22.1/TS5.8.3，不称Node22验收。
- **标准build-wasm尝试失败**：先定位既有官方wasm-bindgen0.2.130（脚本原支持WASM_BINDGEN配置）；锁定公开依赖正常下载后，cc-rs明确报缺少clang。没有生成新wasm/胶水、没有改依赖绕过编译或网络策略，没有把旧产物当成功。新General wasm32 ABI未验；旧产物会被新握手主动拒显。后续须补正常工具链后重新跑标准构建及真实ABI测试。

```sh
pnpm --filter @rynvek/engine build
pnpm --filter @rynvek/engine typecheck
pnpm --filter @rynvek/engine test
pnpm --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/web-platform exec tsc -p tsconfig.build.json
pnpm --filter @rynvek/web-platform exec vite build --config vite.config.mjs
cargo test -p rynvek-engine -p rynvek-wasm --lib --offline --locked
RYNVEK_REQUIRE_GPU=1 cargo test -p rynvek-engine --test protocol_fixture --test general_render --test arena_render --offline --locked -- --nocapture
cargo test -p rynvek-server --test general_authority --test private_rules --offline --locked -- --include-ignored
```

## 父代理后续实证与剩余阻塞

本执行器无浏览器工具，没有尝试任何浏览器目标；既往受管loopback拦截不能原样重试或用代理/隧道/内部API绕过。补齐标准wasm工具链及新产物后，仅在已有获准可达部署验：原OffscreenCanvas/Worker真实GPU握手、权威身体半径/位置随已提交Tick变化、同Tick根回执、无本地STEP/预测、换世界/离线/撤权/隐藏/失鲜清屏、旧WASM拒显/设备丢失、凭据与源码不入Worker/日志/存储。

General协作主体准入/自治调度、浏览器签名注册登录、分页/映射编辑器/发布UI、Agent独立赛道/真实provider、C#规则消费、全规范跨种类锁/Schema迁移/组织信任、来源长期保留、完整生命材料生长遗传、多Chunk/三维/任意World、presence/离线多主仍欠。无新外部模型/支付授权，未调用外部模型或发送私有内容。

Node22/C#本批重编/新wasm32 ABI/真实browser硬件GPU/实际PG SQL并发多机OS断电24h长稳仍欠；MemoryStore+文件CAS/原生llvmpipe非替身。旧6个PvP_Custom夹具须补真实lease，禁降生产fence。原Store、Simulation、规则计价与提交锁零改动；未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询，主体齐后集中正式发布门禁，不以阶段54结束标整体完成。
