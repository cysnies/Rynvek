<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# RYN-1 第八批：真实 WASM 胶水、模块 Worker 与 Arena 挂载

## 范围与证据边界

本批沿用现有 EngineWorker、Worker 代理、Arena 观察与服务端控制租约链，没有另造演示世界或客户端模拟。代码实现提交为 `7c9ed9e`、`8586cc0`；本文件另行提交。浏览器仍未验收，没有重试浏览器、启动服务、修改管理网络策略、代理、隧道或例外端口，也不把旧 PID 当作当前证据。

- 已具备代码链：真实 `wasm-bindgen --target web` → ES 模块 Worker → 原有 Worker 代理 → Arena 页面；异步加载前先注册消息监听，避免初始化期间丢失 INIT/观察消息。
- 已实际构建：Rust `wasm32-unknown-unknown`、同版本胶水及 Vite 生产资源。生成目录忽略，不提交二进制或旧胶水冒充新构建。
- 已实际验证：Node 加载生成的真正 WASM 做 ABI/只读状态机测试；原生真实 wgpu 的软件 GPU 回归。
- 尚未验证：浏览器 Worker/WASM/WebGPU 能力握手、GPU 页面显示、硬件 GPU、浏览器性能与部署。Node 无 GPU 模式不是渲染验收，原生 llvmpipe 也不是浏览器验收。

## 受支持的编译器恢复

遵循 unblock-research，累计三次搜索、五次页面读取，包含 Grok Bot 与公开 X 查询；前两者没有提供可采纳的安装方案，最终依据官方项目文档与发行元数据。没有把先前 APT 500/SSL EOF 推断为网络策略封锁，也没有重试 APT、换镜像或关闭 TLS/签名校验。

来源：

- [WASI SDK 官方说明](https://raw.githubusercontent.com/WebAssembly/wasi-sdk/main/README.md)
- [WASI SDK 32 官方发行](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-32)
- [同一发行的官方 API 元数据](https://api.github.com/repos/WebAssembly/wasi-sdk/releases/tags/wasi-sdk-32)
- [LLVM 官方发行入口](https://releases.llvm.org/)

从官方发行下载 Linux x86_64 `.deb`（100,310,292 字节），核对 SHA256 后仅在工作区无特权解包：

```text
SHA256 894742fb4fac8fb73c2c9715accceda2bf713c002719b26a8bb99a454bb6291b
clang version 22.1.0-wasi-sdk
/workspace/rynvek-tools/wasi-sdk32/root/opt/wasi-sdk/bin/clang
```

只借用其 Clang/llvm-ar 编译现有 zstd C 依赖，Rust 目标仍是浏览器 `wasm32-unknown-unknown`，没有换成 WASI 运行时。真实生成模块的 ABI 测试检查无 WASI 导入；未禁用压缩、删除持久化或调整 Rust 依赖边界。原有 SQLite 本就限原生目标，本批未改 Cargo 清单/锁文件。

`wasm-bindgen` CLI 固定 `0.2.130`，必须匹配 Cargo.lock，生成器在调用 Cargo 前拒绝错版本。CLI 官方归档 SHA256：

```text
9687f3bb551b9f66f37ed32a40d7913a00f7962db653054e56857bc306c3227f
```

CI 增加同版 CLI 下载、校验、实际 WASM 构建与 ABI 测试，然后运行原有全门禁，不减少既有检查。托管 Ubuntu 使用其已有 Clang，不在 CI 下载整套 SDK。

## 页面行为

默认入口暂留明确标注的 Canvas2D。真实 GPU 入口为相对路由 `/arena?renderer=webgpu`（不是已启动的可访问服务链接）。该路径不会取得 2D context，只有真实服务连接和控制租约成功后才创建 Worker、转移 OffscreenCanvas，并要求 GPU 能力；失败明确显示，不静默回退。

沿用权威观察、既有 SDK 命令/租约与隔离语义；RAF 只同步相机和尺寸，不推进世界。转移后不再改 HTMLCanvas 尺寸，每次重连换新 canvas，退出清理观察订阅与 Worker。覆盖加载失败、messageerror、GPU 降级、初始化超时等边界。GPU 选择圈/建造预览叠层仍待实现，侧栏选择与坐标换算保留。

## 最终 HEAD 的有界复验方法

已有固定依赖与共享 target 的本机环境如下；新环境需先恢复冻结依赖和已有依赖包构建，不能用历史输出冒充复验：

```sh
export PATH=/workspace/rynvek-tools/node22/node-v22.23.5-linux-x64/bin:$PATH
export WASM_BINDGEN=/workspace/rynvek-tools/wasm-bindgen-0.2.130/wasm-bindgen-0.2.130-x86_64-unknown-linux-musl/wasm-bindgen
export CC_wasm32_unknown_unknown=/workspace/rynvek-tools/wasi-sdk32/root/opt/wasi-sdk/bin/clang
export AR_wasm32_unknown_unknown=/workspace/rynvek-tools/wasi-sdk32/root/opt/wasi-sdk/bin/llvm-ar
export CARGO_TARGET_DIR=/workspace/rynvek/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo fmt --all -- --check
cargo clippy --locked -p rynvek-engine -p rynvek-wasm --all-targets -- -D warnings
RYNVEK_REQUIRE_GPU=1 cargo test --locked -p rynvek-engine --lib --test protocol_fixture --test arena_render --test engine_gpu -p rynvek-wasm --test host -- --nocapture
pnpm --filter @rynvek/engine test
pnpm --filter @rynvek/web-platform test
pnpm --filter @rynvek/engine typecheck
pnpm --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/engine build
pnpm --filter @rynvek/web-platform build
pnpm --filter @rynvek/web-platform test:wasm
git diff --check
```

本批计数：Rust 40、Engine TS 48、Web TS 13、真实 WASM ABI 4，共 105 项不同测试。ABI 测试编写时曾纠正 fixture 世界 ID 与无 GPU 模式不应发 FRAME_RENDERED 的错误测试前提，不将其计作产品回归或“先红后绿”修复。真实断言包括观察 tick 不前进、旧 tick/错误世界拒绝、START/STEP 拒绝和关闭终态。

实际原生 GPU：llvmpipe LLVM 19.1.7 / GL / CPU / TierC。生产构建生成模块 Worker 约 45.39 kB、WASM 8,952.46 kB、主 JS 60.11 kB；Vite 的 gzip 估算不代表部署压缩已验证。

没有全工作区大构建或删数据/缓存。文档落盘前磁盘可用约 86 GB；共享 target 约 908 MB，SDK 下载加解包约 556 MB。

## CI 与剩余工作

按用户要求，CI `44028349057` 仅检查一次，2026-11-17 14:10:36 中国标准时间（UTC+8）仍为 IN_PROGRESS，没有可读的失败结论；未据此声称通过，也不高频轮询。新 HEAD 推送后单次查询的 CI 状态记录在同一项目 Page。

下一步应先看新 HEAD 的真实 CI 结论；若失败读实际日志修复。浏览器限制解除前不重复尝试，不声称浏览器验收。仍缺真实浏览器 GPU/Worker 交互与性能验收、GPU 地形/美术和选择/放置叠层、DOM 向既定 React 架构迁移；本批未重跑完整服务端/PG/全工作区门禁。
