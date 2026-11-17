# RYN-1：裁剪观察接入既有 Worker / wgpu（2026-11-17）

## 完成边界

本批实现提交：`77228e2`、`6bb2fa3`，沿用 PRIVATE 仓库现有 PR2。
冻结依据：开发规范 §7.1、项目文档 GAP-02：Dedicated Worker 独占 Rust/wgpu Device/Queue；显示只读消费权威状态。

- `ENGINE_INIT.arenaFaction` 选择只读远程竞技场显示模式；默认旧 Simulation 模式保持兼容。
- `ARENA_OBSERVATION` 接收同一 `FactionObservation` 线上结构，绑定世界和阵营，拒绝倒退 Tick、非法网格和坐标；全部校验后才替换观察。
- 场景走既有 `FrameScene`、`WorldRenderer`、`Engine::render_frame` 与 wasm Presenter，不新造 TS GPU 引擎或第二套 Device。
- 远程模式初始化前为空场景，不显示四实体演示；禁止 STEP/START，直接 step 也拒绝；不产生伪造 TickCommitted/root。
- `createArenaRenderHost` 订阅现有 ArenaClient 的服务器观察，一次移交画布；dispose 退订并关闭 Worker。输入仍走原租约、fencing 和命令 API；Ack 不触发乐观位置修改。
- Worker 初始化 Promise 拒绝或帧异常会显式发 ENGINE_ERROR、停止调度，不再让拒绝的队列永久静默。

这只是已实测的**库级显示接线**。现有 `/arena` 仍为明确标注的临时 DOM/Canvas2D，没有声称已挂上浏览器 WebGPU，也没有新增可用 GPU 页面。

## 迷雾与权威边界

只输入服务器已经裁剪的观察，不把 ArenaWorld、私有快照、能力令牌交给渲染器。敌方单位必须位于当前可见格，建筑任一占地格可见才绘制；未知或仅探索区域中的敌方动态实例在 GPU 上传前即丢弃，关闭 Fog 图层也不能恢复。

Q32.32 判界和迷雾索引保持整数精度，避免 f32 把 `4 tiles - 1 raw` 舍入到相邻可见格。远程场景不绑定演示 Simulation 的场缓冲。可见/已探索格暂为纯色背景，**不是完整真实地形材质**；幽灵建筑与地图纹理待接。

远程 GPU 拾取暂时明确返回未命中，不把 RTS EntitySerial 冒充 dense index；现有输入控制器仍从裁剪观察选择。旧 Simulation 渲染和 GPU 拾取回归保留。

## 先红后绿及实际验证

1. Rust Worker 行为红测：旧宿主忽略 arenaFaction，STEP 后 Tick 实际 1、期望 0，exit 101；加入只读模式后原用例通过。
2. TS 生命周期红测：初始化 Promise 拒绝后 stopped 实际 false、期望 true，并出现未处理拒绝；同一用例在修复后通过，无忽略断言。
3. 开发中有过测试 Q32_32 加法类型错误、正式枚举大小写/租约字段错误、未处理 Result 和 clippy identity_op；均修正，没有降低门槛。

下列用例在电脑更换前确实执行通过；重复回跑不累加：

| 范围 | 本批保留用例数 | 覆盖 |
| --- | ---: | --- |
| engine --lib / arena_render / engine_gpu / protocol_fixture | 10 + 6 + 7 + 2 | 真实权威移动后场景变更、像素读回、隐藏敌人不上传、Q32 边界、错误绑定/旧帧原子拒绝、既有根与 DeviceLost |
| wasm --lib / host | 6 + 8 | 初始化/排队/关闭、远程 STEP 拒绝、原固定步长和恢复 |
| server active_transactions / http_api / match_stream / web_arena | 4 + 8 + 4 + 3 | 事务冲突、周期锚点恢复、权限/迷雾、真实移动建造和签名回放 |
| @rynvek/engine | 31 | SDK 观察桥、输入仍走权威命令、Ack 不改画面、类型夹具、一次移交、生命周期异常与退订 |
| @rynvek/web-platform | 4 | 原入口/投影/迷雾/失败可见性 |

共 93 个交付用例对应的针对性通过记录；不是最终 HEAD 上的一次统一全量运行，更不是全 CI。换机前 engine/wasm --all-targets clippy -D warnings、全仓 fmt、diff，以及 TS typecheck/build 也通过。

真实 GPU 用例强制 RYNVEK_REQUIRE_GPU=1：llvmpipe LLVM 19.1.7 / Gl / Cpu、Mesa 25.0.7、TierC。是原生软件 GL 的真实 wgpu 执行，不是物理 GPU、浏览器 WebGPU 或性能验收。

## 实际依赖清点与未通过项

换机前可解析 React/react-dom 19.3.1，Rust wasm32 目标已安装；不能说 React 不存在。未找到 wasm-bindgen CLI，也没有浏览器生成胶水。旧 release `.wasm` 的存在不能证明当前源码。

实际运行 `cargo check -p rynvek-wasm --target wasm32-unknown-unknown` **失败**：zstd-sys 的 cc-rs 找不到 clang；clang、clang-17/18/19、wasi-sdk/Swift clang 路径清点均无可用编译器。失败发生在依赖构建阶段，不能记为当前 Rust/WASM 桥通过。没有安装大型工具链或修改检查。

## 执行中发生的真实阻塞：宿主电脑更换

2026-11-17 13:47（中国标准时间 UTC+8），最终复验 Shell 返回：`The computer was replaced during this call, so it may have run even though no result came back`。随后核查：无 cargo/rustc/旧开发服务进程，`rustup toolchain list` 为 no installed toolchains；原 target/debug/rynvek、target/debug/deps、Node 依赖目录均不存在，仅旧 PID 文件仍在。磁盘由约 12GB 可用变为约 88GB。这不是执行器清盘或主动换机。

源码也回退到较早同步快照。已从本轮实测记录恢复 Q32 精度、Result、正式枚举/租约字段和 clippy 修复；最后的额外能力握手试验及其用例未纳入本次交付。**恢复后的最终 HEAD 未重新编译、测试或 fmt，必须以新 CI 为准**；不把换机前结果冒充该 HEAD 已全绿。

原 Vite/Rust 服务没有被我停止或重启，但已不能称仍在运行；旧 PID=838258/838256 不能继续当作存活证据。由于原可执行文件和依赖已不存在，遵守不大下载/重装/全量重建约束，本轮不重建服务，不再浏览器或网络路由诊断。

## CI 与下一步

13:33:44（中国标准时间 UTC+8）仅查询一次原 run 44023667954：IN_PROGRESS、结论为空，之后没有轮询它。提交推送状态由交付报告记录；不请求审阅、不合并、不 force、不跳 hooks。Rust Cargo 依赖边、许可证和门槛未改，仅为 TS engine 声明实际使用的 client 工作区依赖；离线链接 downloaded=0、added=0。

剩余：先让 CI 验证换机后恢复的提交；恢复受支持工具/缓存后补最终本地复验。浏览器生成胶水、同版本/能力握手（拒绝旧 WASM 忽略新模式）、React 实际挂载、地图材质、幽灵建筑和带版本的 GPU 拾取仍需接入。当前库 API 只供同版本 Worker/WASM 配对，正式挂载前必须补握手。浏览器受管限制证据保留在同一纠偏 Page，仍未通过，不再试回环、改策略或走代理/隧道绕过。
