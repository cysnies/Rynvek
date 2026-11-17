# RYN-1 第四批：真实服务器权威 Web Arena 开发入口

日期：2026-11-17，中国标准时间 UTC+8。起点 b4b6423，继续 PRIVATE PR2 的原分支，不合并、不请求审阅。

## 起始核实与实际断点

开始时工作区干净、远端 HEAD=b4b6423、main=330a01a，PR2 OPEN；只查询了一次旧 CI `44021638216`，仍 IN_PROGRESS，未反复轮询或预报绿色。

阅读冻结项目文档 §2/§6 与开发规范 §7：最终方向是 React/TypeScript、Rust/wgpu/WebGPU、Dedicated Engine Worker 独占 Device/Queue、OffscreenCanvas、只读表现；视觉为 Neutral Dark + Industrial Lime 与 Mindustry 式工业像素。当前只接通最小真实入口，不删除这些目标。

发现 `apps/web-platform` 仅有四路由数据、没有 HTML/dev 入口；已有 `packages/arena.createArenaApp`、Controller、资源 HUD 与 `packages/client.ArenaClient` 可以复用。另一个真正断点：已有 WebSocketTransport 发送 Arena JSON，而正式服务端只接受 wire 二进制帧。先新增真实 TCP/WS 红测：请求 JSON 模式时，实际收到“实时流只接受二进制 WebSocket 消息”的二进制拒绝帧，断言失败、exit 101。实现适配后同一测试转绿，不把类型/编译错误称为行为红测。

## 本批实际实现

- 在现有 `apps/web-platform /arena` 挂载真实 HTML/Vite 入口，直接复用 ArenaApp、InputActionRouter、ArenaController、ArenaClient、TerrainGrid 的 BLAKE3 校验、buildingSpec 与协议类型。不复制 RTS 仿真，不从随机数/前端计时器生成世界。
- 服务器 `/stream?format=arena-json` 仅是传输适配。首条 Arena hello 的能力令牌翻译为原 ClientHello，通过同一个 `serve_link` 的 token、ABAC、世界身份与席位验证；Principal **只从认证后的 ServerHello 取得**，不相信浏览器自报身份。后续命令仍封装原 CommandEnvelope，经同一 Actor 的命令闸门、租约、APM、有序 Tick 事务、持久化及确认。
- 只有收到已提交 StateStream 帧才触发同一 Actor 的阵营 Observe 查询（每 3 Tick 最多一次；30Hz 仿真时约 10Hz 显示）。所有单位、资源、迷雾、施工和终局均取原权威裁剪观察，不发完整世界快照给浏览器。地图使用既有静态地图协议；未知地块黑色遮蔽，敌人裁剪仍在服务器执行。JSON 有 64KiB 入站上限、显示发送超时及事件积压限制。
- 终局关闭连接前通过既有 ArenaServerMessage 发布最后一份真实阵营观察，避免浏览器向已退出 Actor 查询。没有额外领域写入或幽灵修订。
- 显式 `--local-practice-origin http://127.0.0.1:5173` 才允许创建本地私有 PvE。默认关闭；启动时拒绝非回环监听、非回环 HTTP Origin、非内存 Store。创建请求要求精确 Origin 及 X-Rynvek-Local，自定义头强制跨站预检且不开放 CORS。每局是临时正常 Principal 和能力令牌，通过正式 WorldCreate 权限、私有 genesis、内部 attach_world_host 和真实 PvE 导演，不绕过生产登录/授权。最多四个活动练习；不是公网匿名服务。
- 临时身份只在当前标签页 sessionStorage 用于刷新重连；不在 URL、日志或文档中暴露令牌。数据与签名密钥为进程内临时开发数据，重启丢失，不冒充生产持久化。
- 可操作项：单选/拖框/全选、右键情境移动或采集、显式移动/采集落点、建筑落点、资源账本/施工、镜头缩放平移/主脑居中、真实错误与提交回执、投降终局、下载服务器签名回放。暂停明确禁用，因为当前权威协议没有暂停；没有前端暂停假象。

## 可重复启动与验收

开发目录 `/workspace/rynvek-dev-20261117`。当前复用已存在的 Rust target 和 Node 依赖，不下载大型 SDK。

终端一：

```sh
cd /workspace/rynvek-dev-20261117
export CARGO_TARGET_DIR=/workspace/rynvek/target
export CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2
cargo build -p rynvek-cli
"$CARGO_TARGET_DIR/debug/rynvek" server --http 127.0.0.1:8787 --local-practice-origin http://127.0.0.1:5173
```

终端二：

```sh
cd /workspace/rynvek-dev-20261117
pnpm --filter @rynvek/web-platform dev
```

浏览器入口 `http://127.0.0.1:5173/arena`（**不要改用 localhost**，精确 Origin 有意绑定 127.0.0.1）。默认无需用户输入账号，点击“开始本地练习”由显式回环开发模式创建临时身份，并非取消生产认证。

1. 点击开始，等“服务器仿真中 / 已获控制租约”，应出现真实主脑和单位、Tick 递增与三资源账本。
2. 点“全选己方单位”，在主脑附近空地右键，观察单位实际移动和“已提交 Tick / order_issued”，不是只有本地目的地标记。也可左键点单位或拖框，再下令。
3. 点“采集”后点黄绿色沼泽，或工蜂右键沼泽；观察后续真实生物质变化。采集效果仍受实际距离与 RTS 机制影响，不保证点下立即加资源。
4. 选“信标”，在主脑旁可见空地落下 2×2 占地，观察生物质/神经胶扣除和施工；或在黄绿色沼泽放萃取器。黄色虚影不代表服务器已接受。若地形不合法，错误应可见，可另选位置，不造成功动画。
5. 再把建筑放在主脑/已有建筑占地上，应收到明确稳定错误码，不新增建筑、不静默失败。
6. 查看黑色未知区，不能看到未探索敌人。滚轮缩放，WASD/方向键平移，Home/主脑居中返回。
7. 点击结束练习，看到真实终局；待服务器归档后下载 `.rreplay`。下载不是播放器；若归档尚未就绪会显示实际错误，可稍后重试。

由父代理安排真实浏览器验收。本执行器没有打开或操纵浏览器，不能把 JSDOM/网络测试称为人工可玩验收。

## 验证与边界

- `cargo test -p rynvek-server --test web_arena --test active_transactions --test http_api --test match_stream`：19 个项目测试通过（3+4+8+4），0 失败/ignored。新真实网络路径覆盖 JSON welcome、私有 PvE、伪造 Principal 字段不能改变身份、未知区/敌方裁剪、真实移动后持久化恢复根一致、建造与资源扣除、重叠拒绝、投降终局、下载并重新仿真验证签名回放、陌生人拒绝，以及开发模式默认关闭/跨 Origin/非回环拒绝。
- Web `typecheck`、标准 `build`（tsc+Vite）、`build:web` 通过，产物约 50.5kB JS / 2.5kB CSS（未压缩）。Vitest 4 项通过：原路由、投影反投影、未知格保守遮蔽、真实入口挂载与服务失败不生成假状态。JSDOM 对画布/API 的 mock 仅用于 UI 失败路径，不是服务端仿真证据。
- `cargo clippy -p rynvek-server -p rynvek-cli --all-targets -- -D warnings`、全仓 fmt 与 git diff --check 通过；未削弱门禁。Rust 1.87；本机 Node **24.22.1**，与冻结/CI Node 22 不同，已有 engine warning 如实保留，Node22 兼容仍待 CI。
- 添加 Vite 仅复用锁内已有 7.3.8，更新两项直接依赖声明，锁文件离线更新 downloaded=0/added=0。无大型下载安装、全工作区本地构建、旧 target/缓存/用户数据删除。
- **临时 DOM 面板 + Canvas2D 开发显示层**，不是最终 React/shadcn 组件集或 Worker/wgpu/WebGPU 渲染。不改变最终方向、不声称硬件/GPU/WASM 性能验收。完整神经生态、开放规则实际注入、真实 Agent 三方共局、回放播放器、断线控制面完整恢复、PG/断电、24h soak 仍未完成或未验证。
- 本批最小 PvE 切片不代表整个 Web 平台/Studio/Lab/Hub 产品完成。最新完整 CI 与浏览器实测结论以同一纠偏 Page 后续记录为准，针对性通过不可替代。
