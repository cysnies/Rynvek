<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# RYN-1 第九批：真实地形、己方选择环与权威输入坐标

## 起点与边界

起点 `b4b322b` 的真实 WASM/模块 Worker 链已经构建，但 `observation::scene` 仍仅画可见格统一底色和实体；静态地形没有进入场景、GPU 没有选择反馈。更严重的是 Rust `Camera2D`/实例 WGSL 使用 Y 向下，Web 的 `unproject` 却按临时 2D 的 Y 向上解算，GPU 显示位置与发给权威控制器的点击目标竖直镜像。本批先修这些可验证缺口，没有为 React 迁移换皮。

2026-11-17 14:30:18（中国标准时间 UTC+8）仅查询 CI `44031512056` 一次：IN_PROGRESS，实际 WASM 构建/ABI 步已在 14:30:10 成功，完整门禁仍在运行，无失败结论或已完成失败日志可读。不再轮询该运行，不声称完整 CI 已绿。

本批三个细粒度中文提交，其中实现为 `5ce1c9b`、`10c51e2`，本文件另行提交；最终完整 HEAD、正常推送与新 CI 的单次状态写入同一项目 Page。

## 实现：复用而非旁路

- `ArenaClient` 已按服务端 welcome 中的 BLAKE3 解码地图；现有桥在第一条观察前转发一次 `ARENA_MAP`，不重新生图。Worker 再验尺寸（最大 1024×1024）、地形种类、哈希、世界绑定和观察网格一致性；静态地图不能在同一 Worker 中被替换，失败原子保留上一场景。
- 仍使用 `FrameScene` / `WorldRenderer` 的 L0 地形实例与 L7 Ring。原 L0 被 `FrameScene::push` 当作纯全屏层丢弃，现允许真实静态地图实例，L1 仍只读场缓冲，保留图层开关与顺序测试。没有另一套渲染器或 CPU 伪造 GPU 位图。
- 未知格只上传统一不透明背景，不上传地形种类；已探索格按真实种类暗化；敌方实体仍只在当前可见格允许进入实例。禁用迷雾图层也不能取消这些过滤。
- `ARENA_SELECTION` 仅传实体编号、不传坐标。Rust 从当前观察查找己方单位/建筑绘制环，外敌、死亡/离开观察的编号不能生成环。所有 Arena 实例拾取编号仍为 0，不把 EntitySerial 冒充 dense-index。现阶段点选/框选继续用既有控制器的几何命中，不宣称已接通 GPU ID 拾取。
- GPU 页面统一 Y 向下投影/反投影，方向键由原控制器按表现相机轴映射；临时 Canvas2D 的默认 Y 向上保留。物理缩放与 Rust Camera2D 的 0.25–512 上下限一致，避免高 DPI 截断后命中漂移；相机/选择同步不推进世界。
- 真实命令仍走既有 InputActionRouter → ArenaController → SDK command/租约/fencing。Ack 不修改观察；租约被收回后命令拒绝，不通过 Worker 绕行。

GPU 路由仍是 `/arena?renderer=webgpu`，不是已启动服务链接。默认临时 2D 保留。GPU 建造预览/拖框轮廓未做，页面明确说明，不再误称已有黄色预览。

## 红绿证据

- 14:32:27：GPU 点 `(34,35)` 预期屏幕 `(340,270)`，旧实现得到 `(340,210)`；选择 API 缺失，实际两项失败。修复后通过。
- 14:32:33：真实地图/选择测试在旧 WASM 协议处报 `unknown variant ARENA_MAP`，确认缺口，而非只写声明。
- 14:34:29：接线后的真实软件 GL 像素仍为背景 `[9,11,10,255]` 而不是地形 `[39,54,43,255]`，发现 L0 实例被原场景过滤；修复后像素与层顺序/开关回归通过。
- 14:36:28：GPU 上方向键期望相机 y=24，实际 y=40；轴适配后通过，旧 2D 行为另保留断言。
- 14:39:59：物理相机请求 1536，而 Rust 实际上限 512；前端命中与 Worker 同步采用同一有效尺度后通过。

中间开发还补齐新结构的 Debug 派生并先重建引擎类型声明；不把这些构建前提问题冒充旧产品回归。

## 软件 GL 像素证据（不是浏览器）

测试 `software_gpu_pixels_cover_terrain_fog_selection_and_y_down_mapping` 在真实 wgpu / llvmpipe LLVM19.1.7 / GL / CPU / TierC 上渲染 256×256 的非对称地形夹具，回读真实纹理，逐字节断言：

| 像素 | 含义 | RGBA |
|---|---|---|
| (16,16) | 当前可见真实地形 | 39,54,43,255 |
| (176,16) | 已探索暗化地形 | 12,17,13,255 |
| (16,144) | 未知格统一底色 | 9,11,10,255 |
| (48,48) | 己方实体、Y 向下位置 | 230,159,0,255 |
| (66,48) | 己方选择环 | 228,255,158,255 |
| (176,48) | 隐藏敌人位置，没有实体/敌方选择环 | 12,17,13,255 |

关闭 Overlay 后 (66,48) 恢复地形 83,108,50,255；禁用 FogOfWar 仍保留未知/敌方过滤。GPU uncaptured errors 为空，显示侧 SimulationEngine tick 始终为 0。另有从真实 ArenaWorld 地图/观察构造场景的测试，不仅依赖合成像素夹具。

## 最终 HEAD 有界验证范围

沿用前批固定 Node22.23.5、Rust1.87、wasm-bindgen0.2.130、官方 SDK32 Clang22.1.0 及共享 target；没有安装或下载新依赖。与前批相同设置两个编译任务、禁增量、去调试符号。

```sh
cargo fmt --all -- --check
cargo clippy --locked -p rynvek-render -p rynvek-engine -p rynvek-wasm --all-targets -- -D warnings
RYNVEK_REQUIRE_GPU=1 cargo test --locked -p rynvek-render --lib -- --nocapture
RYNVEK_REQUIRE_GPU=1 cargo test --locked -p rynvek-engine --lib --test protocol_fixture --test arena_render --test engine_gpu -p rynvek-wasm --test host -- --nocapture
pnpm --filter @rynvek/arena build
pnpm --filter @rynvek/engine build
pnpm --filter @rynvek/arena test
pnpm --filter @rynvek/engine test
pnpm --filter @rynvek/web-platform test
pnpm --filter @rynvek/arena typecheck
pnpm --filter @rynvek/engine typecheck
pnpm --filter @rynvek/web-platform typecheck
pnpm --filter @rynvek/web-platform build
pnpm --filter @rynvek/web-platform test:wasm
git diff --check
```

本批完整有界范围：Rust55（Render12 + Engine/WASM43）、Engine TS49、Web TS18、Arena TS5、真实生成 WASM ABI5，共132项通过；另1项可选实况 Arena stdio-server 测试未配置而跳过，不能计通过。TS 输入闭环使用现有真实服务端开局转录，经实际 SDK 解码、相机反投影、控制器选择和采集命令/租约撤回；不是本批新启动服务端或浏览器的实况证明。WASM ABI 新增真实模块接受地图/选择并拒绝错哈希、错网格、错世界的断言。

所有触及源码保留 AGPL SPDX，Cargo/Node 清单与锁文件零变化，没有新增 DAG 依赖边；未执行完整 license-gate/全工作区构建。复用已有资源、不删除旧数据或缓存。

## 尚未完成

浏览器仍未验收；本批不打开或重试浏览器，不改/绕过管理网络策略、代理、例外端口或隧道，也不启动服务或拿旧 PID 报健康。软件 GL、真实 WASM 构建/ABI、静态生产资源引用与真实浏览器硬件验收严格分开。

仍待 GPU 建造预览/拖框轮廓、施工/血条和正式美术、GPU ID 拾取（当前用受控几何命中）、静态地形缓存与视口裁剪/性能、真正浏览器交互及硬件适配器验收。静态 welcome 地图不代表支持局中动态改图协议。完整 CI、服务端/真 PG、长浸泡未在本机全量重跑；不因定向测试通过宣称产品完成。
