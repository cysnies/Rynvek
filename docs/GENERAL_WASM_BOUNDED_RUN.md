<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 阶段55：General真实WASM构建与有界实付运行

从干净PUBLIC main `724825de288a91d958d67b5e737274b4ca33e9ad`接续。主体继续，整体未完成、未达正式发布质量；不重复AST/签名私库/原Actor/最小投影。相关冻结边界为规范1.1的QuickJS沙箱、7.1原Dedicated Worker、原控制协议与Tick候选态提交。

## 先恢复已有工具，不重复下载

历史官方安装在本机仍完整，阶段54的“缺clang”实际是未恢复目标编译器环境，并非SDK被删除。本批没有下载或更换工具/依赖：

- WASI SDK32官方发行：https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-32 。既有`/workspace/rynvek-tools/wasi-sdk32/sdk.deb`重新计算SHA256：`894742fb4fac8fb73c2c9715accceda2bf713c002719b26a8bb99a454bb6291b`，与此前官方发行API元数据相同。
- 既有Clang/llvm-ar为`22.1.0-wasi-sdk`，根目录`/workspace/rynvek-tools/wasi-sdk32/root/opt/wasi-sdk`；不安装到系统，不换Rust浏览器目标。
- 既有官方wasm-bindgen归档SHA256：`9687f3bb551b9f66f37ed32a40d7913a00f7962db653054e56857bc306c3227f`，CLI为Cargo.lock同版`0.2.130`。
- 复用已有Node `22.23.5`，不是把此前Node24结果改名为Node22。没有网络重试/代理/镜像/安全策略绕过。

标准`build-wasm.mjs`新增显式`WASI_SDK_PATH`：仅在操作者指定时补全目标CC/AR，并在Cargo前检查工具可执行。已有目标CC/AR优先；没有写死本机目录、自动下载或悄悄换为WASI。CLI错版、工具不可用、Cargo失败仍立即报错，不用旧产物充数。

## 暴露并封闭真实Tier1平台缺口

恢复clang后，下一真实错误不是GPU：锁定`rquickjs-sys 0.11.0`提示无`wasm32-unknown-unknown`绑定，其C源码报缺`stdlib.h`。不通过借WASI libc/改目标/关闭沙箱或假JS执行器“修复”。

原生QuickJS宿主依赖移到正确的原生target；浏览器仍编译原组件Schema/Tier0和远程观察器，但明确拒绝含Tier1的本地Registry/World整体安装。防御直接执行也先拒绝整组，不先发布Tier0再跳过Tier1，不产生替代执行结果/假免费Tick。已有原生服务器QuickJS代码、预算/堆限制和Tier1生物实付不变。

**这不是完成规范的QuickJS-NG(WASM)。** 浏览器本地Tier1运行时依旧是主体/发布缺口，不能把目标隔离称全平台Tier1支持；General远程显示消费服务器权威身体，不需将服务器规则源码/完整快照带入Worker。新ABI同时验证原生实付Tier1身体投影可被远程General接收，且本地Tier1安装被拒后仍可安装原Tier0，证明拒绝没有部分污染世界。

## 用户有界运行：原SDK/Actor逐Tick，而非第二套调度服务器

`GeneralWorldSession.runTicks`与`/general`明确开始/停止入口：

- 用户先选世界、明确连接并取得原Sovereign，指定1..128个Tick；128只是单次产品操作上限，不是Gas/CPU/生命能量配额。
- 固定本次世界/生命周期/宿主与控制双fence，持同一会话互斥锁，复用原`PrivateRuleClient.tick`顺序发送下一Tick。每次新UUIDv7幂等键，只接受已确认的下一Tick/根/观察；没有新权限架构、本地物理或模型provider。
- 当前安装的原规则真实执行、原Actor实际计价。每个Tick是原独立事务，不是整批原子提交。不会自动安装/维护、重试、续租、降级、抢控制或跳Epoch；撤销/欠费/坏回执即停。
- 显式停止、离线、隐藏、会话/私有状态失效即停止后续请求；不把AbortSignal称服务器回滚。报告已确认数量与末请求是否未确认，清观察和双fence，须显式刷新/连接确认，晚回执不能复活视图。停止按钮不被运行中的fieldset禁用。
- 无已批准外部模型或支付调用，无私有内容发给新外部服务。

这是在线客户端有界驱动当前受控规则，不是耐久后台自治作业、协作主体准入、Agent独立赛道或presence/离线多主。断开客户端不会保留自治任务；不得以此声称这些缺口完成。

## 复验命令（本机已有工具）

```sh
export PATH=/workspace/rynvek-tools/node22/node-v22.23.5-linux-x64/bin:$PATH
export WASM_BINDGEN=/workspace/rynvek-tools/wasm-bindgen-0.2.130/wasm-bindgen-0.2.130-x86_64-unknown-linux-musl/wasm-bindgen
export WASI_SDK_PATH=/workspace/rynvek-tools/wasi-sdk32/root/opt/wasi-sdk
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0
node apps/web-platform/tools/build-wasm.mjs
pnpm --filter @rynvek/web-platform test:wasm
pnpm --filter @rynvek/web-platform exec vite build --config vite.config.mjs
pnpm --filter @rynvek/engine test
pnpm --filter @rynvek/engine typecheck
pnpm --filter @rynvek/web-platform exec vitest run test/general_workspace.test.tsx test/general_viewport.test.tsx test/general_route_mount.test.ts
pnpm --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/web-platform exec tsc -p tsconfig.build.json
pnpm --filter @rynvek/rules build
pnpm --filter @rynvek/agent-sdk build
cargo test --locked --offline -p rynvek-rules --test sandbox_quickjs -p rynvek-world --test browser_tier1_boundary
cargo test --locked --offline -p rynvek-server --test general_authority --test private_rules -- --include-ignored
```

原生测试用原唯一RYWL/RYPK写入器逐字复核`tests/engine/general_world_packages.json`，并实际执行有源Tier1付费位移。显式重生成夹具才设置`RYNVEK_UPDATE_BROWSER_FIXTURE=1`，普通测试不写文件。真实跨Node HTTP用例显式运行，涵盖有界三Tick实付/宇宙能量守恒、签名安装、撤销立即停、实付隔离维护、Epoch换装、最小帧与原私有日志恢复；不是DOM/HTTP拼接冒充浏览器。

## 验证结论与发布剩余

**新WASM标准构建、同版胶水、新General真实ABI与生产Worker打包均实际通过。** 浏览器目标仍是`wasm32-unknown-unknown`，ABI断言没有WASI导入；生成产物继续按原规则忽略，不把旧产物或二进制提交当源码交付。

- 新WASM：12,264,321字节，SHA256 `2524a68fe82e116fd49cc8a9c56570ce3a6064649e27677942b5986e8b2b4e23`；同版JS胶水SHA256 `830eedeaeede0b80e719f43b3cad80627710bf08f6e25731118a61ca7108d111`。
- Vite确实打包上述新WASM及`engine.worker-BS4CPyLP.js`，Worker约45.80kB。WASM是标准脚本的dev profile，未冒称release体积/浏览器性能或线上gzip验收。
- 本批109个不同定向用例通过，新增10，重跑不重复计数：Engine TS54、Web DOM/路由15（4新）、真实WASM/构建脚本10（5新）、原生QuickJS沙箱7、原生实付/可复现夹具1新、server General11/私库11。不是重跑阶段54全部130项。
- ABI10中6项实际加载新WASM、4项构建前置/失败路径；无GPU模式没有伪造FRAME_RENDERED。新General能力回显、最小帧回执、同Tick分歧后永久封闭、本地STEP/START禁用、Tier1本地整组拒绝及原生实付Tier1的远程投影均有断言。
- 原生7项保持死循环/堆炸弹、禁随机与跨Tick隐态等限制；新原生Tier1夹具真正产生有源费用与位移。真实Node→Rust HTTP还验证有界3Tick逐一确认/守恒、撤销停止、显式维护/换装与私有恢复，全部原有提交锁/双fence断言保留。
- 相关TS类型/生产tsc、原AST/SDK构建、Vite和必要Rust格式/diff/变更文件敏感特征筛查通过；实际Node22.23.5。不展开全CI，不冒称完整安全审计。

过程中纠正了夹具的Tier1自动实例绑定、一个TS测试括号、构建mock继承SDK环境，以及夹具生成编辑未匹配导致的测试失败；没有放宽生产校验或跳过失败用例。最终上述检查全部通过。

无浏览器工具，本批未尝试任何浏览器目标，没有原样重试既往受管loopback拦截，也不擅自部署或公开私有数据。父代理只能在已有获准可达部署实证原Worker/OffscreenCanvas/WebGPU握手、权威身体半径/位移、设备丢失/撤权复验/失鲜/换世界/断线清屏、无GPU不假成功、有界运行与停止末请求确认。

下一主体仍包括General按目的隔离的协作准入与耐久自治、浏览器签名注册登录、分页/映射编辑器/发布UI、全平台QuickJS(WASM)、Agent独立赛道/真实provider、C#规则链、全规范跨种类依赖锁/Schema迁移/组织信任、来源长期保留、完整生命材料生长遗传、多Chunk/三维/任意World、presence/离线多主。旧Studio grant明确仅授权创作源码，不因本批自动取得General控制。

C#本批重编/真实browser与硬件GPU/实际PG SQL并发多机OS断电24h长稳仍未验；本批Node22必要链实证不等于全项目Node22矩阵或长稳。阶段54 llvmpipe是历史软件回读，本批不冒称新增硬件GPU实测；MemoryStore+文件CAS不是PG替身。旧6个PvP_Custom夹具须补真实lease，禁降生产fence。未跑全workspace/完整Clippy/license/SBOM/安全/soak/CI轮询，主体齐后集中正式发布门禁，不以阶段55结束标整体完成。
