<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# RYN-1 第六批：换机复验与只读阵营能力握手

记录时间：2026-11-17，中国标准时间 UTC+8。基线为 `03431d40a6f9a1c4584026a03068936fb88aefbe`；本批代码提交为 `cdc92ba`、`43c60bd`，交付 HEAD 见本文档提交与同一纠偏 Page。不是浏览器验收报告。

## 环境与基线实证

- 按 `/home/box/reference/debugging-the-box.md` 运行当前 box-doctor：11 项通过；anyrun，当前目录 `/workspace/rynvek-dev-20261117`，起始工作树干净、远端分支同为 03431d4。PRIVATE 仓库与 PR2 OPEN 经 gh API 实查；仅输出登录名，不读取或输出凭据。main 仍为 `330a01a553dfb504feb06c5e7bcdcaa85c9a1995`。
- 起始磁盘 128G/可用88G；当前可用86G。原 Rust 工具链、编译缓存和旧 Node 模块目标目录缺失。恢复项目锁定 Rust1.87.0（含项目声明的组件/目标）；仅对引擎/Worker定向构建，两个任务、去调试符号、禁增量。当前 target 627M，不是全工作区构建。
- 旧 node_modules 悬空链接导致 pnpm ENOTDIR。将原链接保留在 `/workspace/rynvek-dependency-links-backup-20261117`，恢复真实模块目录，再锁定安装选中依赖：115 包。没有清理旧 target、缓存或用户数据，没有使用安装脚本。
- 系统 Node24 不符合项目版本范围；另从 Node 官方发行站下载并校验31,062,912字节归档，隔离恢复 Node22.23.5 到 `/workspace/rynvek-tools/node22/node-v22.23.5-linux-x64/bin`，未改变系统 Node。最终 TS 验证使用该 Node22 与 pnpm10.21.0。
- **基线03431d4本机重新验证**：Rust定向32项通过；TS engine31 + Web4与类型检查通过（初轮为系统Node24，不能当作CI Node22结论）。`cargo fmt --all --check` 实际失败于 observation.rs 与 arena_render.rs 两处。归类为恢复代码格式回归，不是硬件/工具链导致；已在 cdc92ba 单独修正。不能用换机前“fmt通过”覆盖本次失败。
- CI44026967162 在13:52:54仅查询一次：IN_PROGRESS、conclusion为空。未重复轮询，查询时没有已完成的失败日志，不能杜撰其最终失败原因或通过结论；新推送CI状态由同一Page/最终报告记录。

## 本批唯一桥接缺口：旧 WASM 被误认为支持只读显示

实际03431d4只有 ENGINE_INIT.arenaFaction，没有 ENGINE_READY 回显或 Worker确认。Rust serde 对未知字段兼容，旧WASM可能忽略只读模式却发布就绪。此处比未备齐工具的胶水生成更短、能立刻防止假就绪，且复用原 Engine/Worker/观察结构。

1. Rust真实 Engine::announce_ready 回显实际配置的 `arenaFaction`；本地模式省略，旧本地协议保持原样。
2. TS协议严格校验可选u8（0与255有效；null/字符串/负数/越界/小数无效），共享夹具覆盖有回显与无回显两种事件，Rust/TS对称往返。
3. 远程Worker在发布任何READY、调度帧、回放排队观察之前，要求真实阶段ready、恰好一条匹配阵营READY、无非法事件；required GPU还必须报告webgpu。失败发显式ENGINE_ERROR并停止后续调度/消息回放，不把CPU空白或旧WASM当成功。真实设备初始化失败保留原错误；组件仍负责dispose终止Worker。
4. 不新增仿真、命令API或权限旁路；移动/建造等输入继续走原ArenaClient租约/fencing/服务器宿主。现有雾裁剪、整数边界与GPU实例过滤保持不变。

## 先红后绿与最终范围

- TS先复现10种握手失败仍被当成功；补充未完成初始化却伪发READY的1项反例亦真实失败。Rust新增用例实际得到arenaFaction=null而非0；均是行为失败，不是编译错误。
- 最终定向运行应为 **90项**：Rust40（engine lib10、arena_render6、engine_gpu7、protocol_fixture2、wasm lib6、host9），TS46 + Web4。最终提交后再次复跑这些完整选中套件；不累加基线或红测轮次，也不拿历史93项冒充本批结果。
- Rust GPU部分设置 `RYNVEK_REQUIRE_GPU=1`：llvmpipe LLVM19.1.7 / Gl / Cpu / TierC 的原生wgpu软件适配器；场景像素、隐藏敌人、原设备丢失/根恢复通过。不是硬件GPU、浏览器WebGPU或性能结论。
- 严格 engine/wasm `clippy --all-targets -- -D warnings`、全仓fmt、git diff --check、TS引擎构建、双方typecheck、Web Vite产物构建均实跑。未改Cargo/JS依赖声明或锁文件、许可证、tier依赖边及CI门槛。

可重复命令（在本目录运行；先将上述Node22目录加入PATH）：

```sh
export CARGO_TARGET_DIR=/workspace/rynvek/target CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo fmt --all --check
RYNVEK_REQUIRE_GPU=1 cargo test --locked -p rynvek-engine --lib --test protocol_fixture --test arena_render --test engine_gpu -p rynvek-wasm --test host -- --nocapture
cargo clippy --locked -p rynvek-engine -p rynvek-wasm --all-targets -- -D warnings
pnpm --filter @rynvek/engine build
pnpm --filter @rynvek/engine --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/engine --filter @rynvek/web-platform test
pnpm --filter @rynvek/web-platform build:web
git diff --check
```

## 剩余与边界

- 尚无clang、wasm-bindgen CLI与浏览器生成胶水；本批不重复声称 wasm32交叉构建通过，也不为此大安装Clang。本批验证是原生WASM宿主Rust逻辑与TS胶水协议，不是运行生成的浏览器WASM ABI。
- `/arena`仍为明确标注的临时DOM/Canvas2D；新握手只完成既有Worker库桥接安全缺口，尚未挂载React/Worker/WebGPU页面。下一最短路线为准备可复现WASM编译/同版本胶水与模块Worker入口，再挂载现有createArenaRenderHost；不是重造渲染器。
- 本机本批未重跑server/PG/断电注入、完整license-gate、全工作区CI、Python或长浸泡。CI全门禁仍须真实通过，定向90项不能替代它。
- 未打开或重试浏览器，未诊断/修改管理策略、代理、隧道、路由或例外端口；没有重启服务。旧PID和历史HTTP状态只作历史记录，浏览器仍未验收。不请求review、不自动合并、不更改私有可见性。
