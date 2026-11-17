# RYN-1 第三批：活动宿主的同一 Tick 事务链

日期：2026-11-17（中国标准时间 UTC+8）。起点 `da4ead69059269c6d7968a08cd6f65eb390e0a9f`，现有 PR2 / `fix/ryn-1-runtime-patch-chain`。开始时工作区干净，远端分支与本地一致，main 仍为 `330a01a`。原 CI run `44017125260` 开始时仍 IN_PROGRESS，故继续实施此切片；后续结论与修复见下节。没有把等待 CI 作为停止开发的理由，也不预报通过。

## 本批选定的完整切片

统一的是**活动 Actor 的输入 → 真实领域操作 → Tick → 私有日志/检查点 → Store CAS → 对外确认**，不是宣称整个产品或所有引擎阶段均已完成。

- `ArenaSession.world` 现在持有 `ArenaRuntime`，不再另持独立 ArenaWorld。只读解引用保持地图/观察读取；命令和工具写入统一经 Runtime 事务。`WorldRuntime::Arena` 本来已使用同一 Runtime，本批不是新增平行引擎。
- `candidate_tick` 创建隔离候选，将当前已提交根作为前驱；一帧的所有成功命令和补丁共享一个新 revision。每条失败操作仍在独立候选上失败，不留下半条修改；`finish_tick` 只推进一次。投降可在输入边界终局，实际 Tick 不伪增，目标边界和实际 Tick 分开记录。
- 活动 MatchHost 的单一有序队列同时接收 Command/ToolCall 与 WorldPatch；已到期目标统一在下一 Tick 的 Phase 01 前执行。租约获取/释放是控制面操作，可即时回复，但不直接修改仿真。对外 Ack、ToolResult 和 `status=applied` 只有整 Tick 成功持久化后才发送；返回的 Tick 是实际已提交 Tick。
- HTTP 先做原有 Capability/ABAC 检查；存在活动宿主时，把请求交给其 Actor，不从配置/检查点另恢复一个旁路世界。未配置持久化的宿主拒绝编辑。只有租约没有本地宿主时仍拒绝旁路。**排位世界禁止对局中创作编辑**，不能让世界所有者借 WorldEdit 给自己作弊；可编辑的自定义创作世界走真实共享队列，不取消开放创作目标。
- `TickCommitter` 每 Tick 向私有 CAS 写有序 `rynvek-tick/1` 输入；每 300 Tick 与终局保存完整检查点，再以 fencing + 精确前驱 revision CAS 发布。恢复通过最近完整锚点及最多 300 条有序后缀逐根验证，不保存每秒数十份全量快照。CAS 失败恢复旧 Runtime、不给成功确认，并终止该宿主，防止继续向客户端广播未提交候选。失败可留下不可达私有对象，未实现 GC。
- 输入上限同时受 1024 条 / 4 MiB 排程预算约束，同一待处理补丁最多 64 个等待者，超出明确拒绝，不静默丢弃。

## 恢复、幂等和回放

- 匹配创建不再只存模板根：初始 Runtime 的完整私有检查点与空日志随 genesis 提交保存。`attach_world_host` 是服务端内部启动/恢复入口，要求已建立匹配的对局与席位记录；从完整检查点及有序日志后缀恢复，检查 World ID 和配置一致后认领新的 fencing。没有新增绕过授权的公共启动 HTTP 路由。
- 认领宿主是 Memory/PG 的原子 revision+token CAS；在途旧 HTTP 候选使用旧 token，无法再覆盖新宿主提交。PG 代码通过编译/静态检查不等于真实 PG 验证。
- 补丁 ID 与完整载荷绑定；待处理相同请求合并等待、不同载荷拒绝。已提交相同补丁从持久化日志重建原回执，不再次推进 Tick。一个基线同帧的两个不同编辑只能有一个成功。旧创作 JSON 日志中的已用补丁 ID 保守拒绝复用，而非猜测旧回执。
- 成功世界命令的 Principal 与幂等键写入 Tick 记录；Actor 启动从已提交记录重建去重集合。重复命令明确拒绝，不重复执行；失败命令不冒充成功录入。尚未成功提交的队列不承诺崩溃后保留，也不会提前回复已应用。
- 新增显式载荷 `rynvek-replay/2`：初始完整检查点 + 同序命令/补丁 + 每帧规范根。验证器复用 Runtime 和相同补丁实现逐帧重算，验证身份、前驱、revision、目标/实际 Tick、结果根及终局。容器仍是 RYRP v1，载荷版本明确升级；旧 `rynvek-replay/1` 解码与命令回放保留，不能让旧解码器误读新的根语义。从非零修订恢复宿主后导出的是明确以该检查点为起点的片段，不冒充全局完整历史。
- 本批恢复的是仿真状态、提交日志和成功命令幂等，不是完整网络会话的无缝迁移。席位控制租约、APM、未确认队列及连接状态仍须重建/重新授权；世界 fencing 与原有连接/租约权限防线保留，不把控制面恢复缺口隐藏在“全部 Runtime 已统一”中。

## 先红后绿及实际验证

最先新增 `transactions_preserve_accepted_log_for_replay`：原 Runtime 在事务中通过快照克隆候选，快照不保存观察事件/回放日志，实际红测为已接受投降日志长度 **0，期望 1**（exit 101）。本批候选保留这些不参与领域根的瞬态输出后转绿；最终直接复用既有 ArenaWorld 的独立 Clone 保留事件、日志和导航缓存，防止接入活动宿主后悄悄丢失回放记录。没有把编译错误当作功能红测。

公共环境：

```sh
export CARGO_TARGET_DIR=/workspace/rynvek/target
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=2
cargo test -p rynvek-rts --lib
cargo test -p rynvek-engine --lib provider
cargo test -p rynvek-engine --test arena_session --test arena_loop
cargo test -p rynvek-server --test active_transactions --test match_stream --test http_api -- --nocapture
cargo clippy -p rynvek-server -p rynvek-engine -p rynvek-rts --all-targets -- -D warnings
cargo fmt --all --check
git diff --check
```

额外的真实红测：终局 finalizer 原来又追加一个没有完整检查点的修订。活动 HTTP/命令/签名回放用例实际断言失败：Store revision=3，Actor revision=2。修复是删除该旁路提交，终局任务仅校验 Actor 已提交根/检查点后写比赛结果和回放；创作自定义比赛只记录比赛得分，不写排位 Glicko-2。没有改变断言来迁就假修订。

同一 2431 Tick transcript 的快照候选实现实际耗时 473.29 秒（通过但不可当作合理热路径）；改用既有独立内存 Clone 后，同一字节级断言用例实测 13.56 秒。尝试停止旧慢测试时进程已经正常结束，未终止任何运行中测试。这个比较只是本机同用例观察，不是浏览器/GPU 帧率或标准性能认证。

最终本批项目测试 **109 个不同用例通过，0 失败、0 ignored**：RTS 库 85、engine provider 2（另 8 项未选中）、arena_loop 1、arena_session 5、活动事务 HTTP/Actor 集成 4、http_api 8、match_stream 4。末次 server 三目标共 16 项全部通过，包含真实第 300 Tick 完整锚点、第 301 Tick 无全量快照、最近锚点加日志恢复后第 302 Tick 与活动宿主同根，以及真实 WS/WT 排位宿主拒绝内部创作补丁。

最后 `cargo clippy -p rynvek-server -p rynvek-engine -p rynvek-rts --all-targets -- -D warnings` 通过（6.82s）；全仓 fmt 与 git diff --check 通过。上述项目测试与前两批重叠，不能累加为产品验收；CI profile 独立探针 2 项另计，不混为 111 项项目测试。离线复用旧 target，磁盘收尾约 14GB 空闲，没有安装下载、全工作区本地构建或删除任何旧 target/缓存/用户数据。开发中类型导入与接口对齐编译错误不列作功能红测或通过。

## 真实边界与下一步

1. 没有新增浏览器可玩入口，不启动空壳 dev server 冒充游戏。仍需对接现有 WebArena/Worker/场景提取、真实框选下令/建造、胜负与回放 UI；由父代理安排浏览器验收。
2. 内存候选已避免逐帧快照解码/导航重建，持久化改为逐 Tick 日志及周期锚点；仍有整世界内存复制、完整锚点体积与历史增长，尚未认证 30Hz、大规模 World 或 WebGPU。分块/COW、索引和 GC、分区与实测优化必须继续，不能以正确性通过替代性能验收。
3. 新 Tick 记录覆盖本切片成功命令/补丁顺序与提交，不能宣称是全部七类 `.rjournal` WAL、分布式故障接管或全部控制面持久化。
4. 真 PG、断电/故障注入、硬件 GPU、WASM、浏览器、24h soak 未在本轮运行。神经能量/吸收分泌/生死繁殖/谱系因果闭环、开放规则实际注入和真实三方共局仍须继续完成，不新增 ERRATA 降低原目标。
5. 继续既有 PRIVATE PR2，不请求 review、不合并、只正常推送本批一次；完整 CI 以远端真实 run 为准。


## 继承 CI 的最终失败：标准 Runner 磁盘不足

`44017125260`（HEAD da4ead6）最终于 2026-11-17 12:52:43（中国标准时间 UTC+8）失败：**fmt 0.6s、全工作区 clippy -D warnings 195.3s 均通过**；下一步 `cargo test --workspace` 编译测试二进制时，链接器报 `No space left on device`，不是用例断言失败。本批活动队列代码当时尚未推送，因此不能把此故障归为新队列回归。

最小 CI 修复：仅在 workflow 环境设置 dev/test 的 `debug=0`、关闭增量缓存并限制两个构建任务。`ci-full-gate --soak-duration 90s`、全部步骤/测试/阈值保持不变，未用 release 模式或关闭 debug_assertions，也没有删除 Runner 上预装软件来掩盖容量问题。代价是 CI 产物不带源行号调试信息。

本地用无依赖独立小探针验证相同 Rust 1.87.0 / test opt-level=1 配置：两项测试通过，实际 rustc 命令包含 `-C debug-assertions=on -C strip=debuginfo`，证明没有因为去调试符号而关闭断言。探针没有下载依赖，不是全工作区重建，也不代表 Runner 全门禁已通过。新完整 CI 必须待本批正常推送后给出真实结果。
