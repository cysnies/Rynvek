<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# RYN-1 第十批：真实 CI 失败与 GPU 意图叠层

## CI 不再只报一个 IN_PROGRESS

2026-11-17 14:44:14（中国标准时间 UTC+8）对三个相关 run 及各自 jobs 各读取一次；没有取消任务、重新触发旧运行或轮询。下表为该次快照，不冒充之后的实时状态：

| Run / HEAD | 实际状态 | 创建 → Job 开始 | 所处步骤 / 结束时间 |
|---|---|---|---|
| 44028349057 / 8a15503 | COMPLETED / FAILURE | 14:03:04 → 14:03:07，约3秒 | Full Gate失败；Job于14:18:39结束 |
| 44031512056 / b4b322b | IN_PROGRESS / 无结论 | 14:28:50 → 14:28:53，约3秒 | WASM/ABI已于14:30:10成功，Full Gate自14:30:10执行 |
| 44033229519 / 204e4c6 | IN_PROGRESS / 无结论 | 14:42:42 → 14:42:44，约2秒 | WASM/ABI已于14:43:59成功，Full Gate自14:43:59执行 |

后两个 Job 已分配不同 Runner（1657、1658），确实是并行执行状态，不是等待同一 Runner 的并发队列；pending 的是收尾步骤，不是整个 Job 排队。仅凭 API 整段 Full Gate 状态不能断言内部仍健康推进或已完成，未把它当作存活/通过证明。

对已失败的 44028349057 读取真实失败日志：fmt 0.5秒通过、严格全仓 Clippy 104.2秒通过；`cargo test --workspace` 阶段620.4秒后在 `rynvek-audio/src/lib.rs:136` 失败，实际 `ChemotaxisSync`、预期 `Homeostasis`。Full Gate步骤14:03:33–14:18:35约15分02秒，包含xtask本身编译；内部已报告阶段约725.2秒。因此既有真实失败，也有冷构建/全工作区测试的长耗时，不能简单称全部排队或全部通过。

## 修复真实失败，不修改门禁

本机对当前源码执行 `cargo test --locked -p rynvek-audio --lib`，重现同一失败。原因是所有发射输出均为0时，平均绝对偏差也是0，被算成1000的一致性，从而静默开局误进同步档位。

`e926827` 增加全静默保护：无单位或全静默时一致性为0；不改现有活跃输出MAD计算、Weaver档位、战斗及危急优先级。原失败断言原样保留，并增加静默/一致活跃/分歧输出、空世界、Weaver/战斗/突然死亡/主脑危急优先级测试，共4项通过。注释明确该字段是发射输出一致性，不是Kuramoto相位测量；字段名保留兼容。

没有删减/跳过CI门禁、延长超时假掩盖失败、取消其他任务或把断言改成接受错误值。本批没有改workflow或xtask。

## GPU 建造预览与拖框轮廓

- `611a8a3`：既有32字节GPU实例记录新增 RectangleOutline 标签；宽/高按标签使用两个现有标量槽，不扩实例ABI、不建另一套渲染器。原方块/圆/环旋转语义保持；矩形走原WorldRenderer实例管线，片元生成约1.5物理像素空心边线。Naga与原渲染/拾取回归均运行。
- `ARENA_OVERLAY` 仅含绑定世界、拖框端点与建筑类型/Tile原点；有界有限值校验、错误原子保留旧状态。反向拖动归一化；建筑尺寸复用 `building_spec`。只按公开地图边界裁剪，不读隐藏占地来染色、不预测资源/科技/权限。黄色永远只是用户落点意图，不代表可建。
- `af1c699`：页面按同一屏幕→世界解算和floor原点转发预览；仅选择模式绘拖框，建造/armed命令模式不冒充框选。Esc、右键、pointercancel、丢失捕获清理拖框；建造提交后模式清空，下一帧清除预览，dispose不再发送。
- 真实命令仍走原控制器/SDK/租约/fencing，预览不会发送游戏命令或修改观察。集成测试核对预览与权威建造请求同为原点[4,4]，服务器拒绝后不乐观新增建筑/改世界，保留错误通知。
- Arena GPU ID拾取仍未启用，不把显示编号当dense-index，不为清单重写原CPU受控几何选择；本批另外运行原渲染器6项GPU像素/拾取/图层测试，防止共享shader退化。

## 红绿与软件像素

新功能先在旧实现实测失败：14:47 TS缺renderer.overlay；Rust协议拒绝未知ARENA_OVERLAY。修复后真实软件GL绘制反向非正方拖框与建造空心轮廓：

| 像素 | 实际RGBA | 含义 |
|---|---|---|
| (64,176)、(96,223) | 200,239,160,255 | 2×3格反向拖框的侧边/底边 |
| (160,128) | 234,214,134,255 | 信标2×2格中性落点轮廓 |
| (96,176)、(160,160) | 9,11,10,255 | 空心内部，不填充未知格或显示隐藏敌人 |

隐藏占地中注入敌人和完全移除该敌人，两次纹理回读逐字节整帧相同；清空意图后边线恢复背景。GPU uncaptured errors为空，显示侧simulation tick=0。该证据为真实wgpu / llvmpipe LLVM19.1.7 / GL / CPU / TierC，不是浏览器或硬件GPU截图。

## 最终 HEAD 有界复验

使用既有Node22、Rust1.87、同版wasm-bindgen0.2.130及官方SDK32 Clang；共享target、两个编译任务、禁增量和调试符号。没有新依赖下载/安装、全量工作区大构建或删除旧数据/缓存。

```sh
cargo fmt --all -- --check
cargo clippy --locked -p rynvek-audio -p rynvek-render -p rynvek-engine -p rynvek-wasm --all-targets -- -D warnings
cargo test --locked -p rynvek-audio --lib -- --nocapture
RYNVEK_REQUIRE_GPU=1 cargo test --locked -p rynvek-render --lib --test render -- --nocapture
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
```

范围共146项通过：Rust68（音频4、Render13+真实GPU6、Engine/WASM45），TS73（Arena5、Engine49、Web19），真实生成WASM ABI5。另1项可选实况stdio-server测试未配置而跳过，不计通过。新增意图协议也经过真实WASM ABI接受/拒绝/清空验证；不是只编TypeScript声明。最终完整HEAD冻结后复跑该范围，推送及新CI单次jobs快照记录在同一Page。

源码SPDX保留，清单/锁文件未变，无新DAG边；未宣称完整license-gate。磁盘约85GB可用，target约1GB。

## 尚未验收

真实WASM/胶水与生产Worker构建、原生软件GPU像素、浏览器硬件验收三者明确分开。浏览器仍未打开或重试，不改/绕过管理网络策略、代理、例外端口或隧道，不启动服务、不拿旧PID作健康证据。

还缺浏览器真实输入/捕获生命周期和硬件验收、施工/血条与正式美术、GPU ID拾取设计接线、静态地形缓存/视口裁剪/性能、既定React迁移；本批无新的实况服务器/真PG/长浸泡。完整CI尚待最终运行结果，可能继续暴露后续门禁失败，不能由146项定向通过推出全CI绿。
