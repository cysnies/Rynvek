# RYN-1 第二批：真正执行的 HTTP 世界事务

日期：2026-11-17（中国标准时间 UTC+8）。基线是 PR #1 已合并的 `330a01a553dfb504feb06c5e7bcdcaa85c9a1995`；正常 fetch、main 快进后创建 `fix/ryn-1-runtime-patch-chain`，不重写第一批历史。

## 本批实际交付，不夸大全部统一

原 HTTP 补丁路由把输入放进 CAS，追加原状态根并返回 202/计划 Tick，没有执行器消费。最小红测实际收到 **202 假成功**，之后才实施修复。本批选择“无活动对局宿主的 HTTP 创作世界”这一条小而完整的权威链，未同时重写实时比赛宿主或 Web 应用。

1. `0de5609`：`crates/rynvek-rts/src/runtime.rs` 新增真正拥有 ArenaWorld、World ID、修订及前驱根的 `ArenaRuntime`。引擎公开相同类型，`WorldRuntime::Arena` 使用它，不复制另一套模拟器。`command`、`step`、事务、规范根和 v2 检查点共用实现。
2. `43cb3b1`：内存/PG `append_commit` 保留 fencing，增加前驱修订 CAS、严格下一修订及不倒退 Tick；并发候选不能覆盖最新世界。
3. `fdf815f`：HTTP 创建、补丁、分叉都持有可恢复的完整检查点。服务器直接复用 `rynvek-world` 补丁类型，不再维护另一份仅校验的副本；实际应用并发布后返回 **200/status=applied** 和真实根。

## 权威状态与提交时序

- 权威：ArenaWorld 的规范领域状态、World ID、Tick、revision、parent root；文件头与 CMIT 校验身份/修订，Arena 重算十域状态根。观察与可视化只读访问 `world()`。
- 每个请求从最近已提交私有检查点恢复；不从 MatchConfig 模板推测当前状态。
- Class 1 在下一 Tick Phase 01 前边界应用；Class 2 在严格下一个 Coupling Epoch 的 Phase 01 前应用，再真实推进目标 Tick。每次最多一个 Epoch，避免任意长请求。没有只报告 planned tick 而不执行的后台队列。
- 事务在独立候选状态上推进和应用；失败时 Tick、修订、地图和资源均不发布。操作不能篡改时钟；终局不再推进 Tick 时显式拒绝，避免无进展循环。新增测试的早期实现曾在终局后挂起，安全停止该测试后增加了上述拒绝及回归，没有把中断算通过。
- 成功后写不可变输入和完整检查点，再由 Store 的原子 CAS 发布 CommitRow/WorldRow。CAS 失败可以留下不可达私有对象，但没有运行中内存副本先行更新，也不会覆盖另一个成功事务。尚未做对象回收。
- 目前每次 HTTP 事务都保存完整检查点；`journal_segment_cid` 仍指补丁输入，不将它谎称为完整七类 `.rjournal` WAL。当前恢复依据检查点，不承诺未知 WAL 重放。

## 安全与兼容

- 世界包、创世输入、补丁输入、完整快照使用独立的私有 CAS（磁盘模式位于 `blob_dir/world-checkpoints`），不与 Public Hub Blob 共用读取空间。即使知道完整快照或补丁 CID，公共下载路由仍返回 404。
- 世界权限检查仍先于解码和执行；无读权限返回与不存在相同的 404。活动对局注册或宿主租约存在时拒绝 HTTP 旁路编辑，不创建第二个权威世界。活动宿主队列接入是下一批工作，而不是取消实时编辑目标。
- 未支持的基因、规则、模板、参数及 RequiresForkRestart 明确拒绝，不能靠创建一个空分叉伪报完成。已支持的地形和资源操作真实执行。
- 显式 fork 会校验源检查点、重新绑定新 World ID/revision=0，保留来源关系后编码新检查点；不把源文件头原样放到另一个世界下。
- 无完整检查点的旧服务端世界、v1/错 ID/错 revision/错根文件显式拒绝，不猜测、不静默迁移。旧数据迁移工具仍未实现，部署前需保留旧数据并安排显式迁移。
- 默认无 blob_dir 的内存模式仍是临时开发模式；磁盘 CAS + 元数据存储才是持久化部署组合。宿主重建测试使用真实 FS CAS 和保留的内存元数据 Store，不等同真实 PostgreSQL 重启或断电演练。

## 实际验证：本批 115 个不同用例通过

与第一批存在重叠，不把两批数量简单相加。所有命令使用：

```sh
export CARGO_TARGET_DIR=/workspace/rynvek/target
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=2
cargo test -p rynvek-rts --lib
cargo test -p rynvek-world --lib
cargo test -p rynvek-engine --lib provider
cargo test -p rynvek-server --test http_api -- --nocapture
cargo test -p rynvek-server --test match_stream -- --nocapture
cargo test -p rynvek-server --test hub_verify rworld_publish_upload_and_fs_cas_roundtrip -- --nocapture
```

| 范围 | 实际结果与证据 |
| --- | --- |
| RTS --lib | 84 通过，含 3 个 Runtime 新用例：事务全回滚/时钟篡改、身份绑定及相同命令续跑、Phase 01/终局显式拒绝；已有四脑 WAL/快照仍通过 |
| world --lib | 16 通过，保留第一批原子性和假成功拒绝 |
| engine --lib provider | 2 通过，另 8 项未选中；Arena 分支使用共同 Runtime |
| server http_api | 8 通过，含 2 个新增端到端用例；实际 TCP HTTP 请求验证 200/applied、地形、独立引擎同初态/同补丁/同 Tick 同根、恢复续跑、并发 200/409、跳修订/时钟回退拒绝、旧世界/租约拒绝、公共 CID 下载隔离、分叉后继续应用 |
| server match_stream | 4 通过：真实本地 WebSocket/WebTransport 对局、错误令牌/陌生人拒绝、回放篡改拒绝；关键帧夹具验证开局敌方实体物理裁剪，客户端擅自 advance 被拒绝 |
| server hub_verify 选定用例 | 1 通过，另 2 项未选中：世界包发布、上传、FS CAS 回归 |

最终各组 0 失败、0 ignored。测试过程中出现的红测、一次 BlobId 类型别名编译错误及新增终局测试中断均已如实保留为开发过程，不列作通过。触及 Rust 文件格式与 `git diff --check` 通过；无新 Cargo 依赖边、许可证或 CI 改动，高置信度凭据模式检查未命中（不是完整安全审计）。

磁盘由约 19GB 降至约 16GB，复用既有缓存、离线且两个编译任务；没有删用户文件/旧 target/缓存，没有安装下载或全工作区构建。

未运行：真实 PostgreSQL、故障/断电、全工作区 CI/clippy、WASM、浏览器、硬件 GPU/性能基准、24h soak。本批没有新增 GPU 性能结论，第一批软件 GL 结果也不是实机证明。

## 剩余与最短可玩路线

1. 将当前 Runtime 接入既有 ArenaSession/MatchHost 的单一事件队列，统一命令、补丁、租约/fencing、迷雾观察和提交；活动宿主 HTTP 目前是明确拒绝，不是假称已接通。
2. 贯通 Server/Native/WASM 同记录的根一致性与重连恢复；完善事件 WAL 和迁移，检查点克隆的正确性路径再按实测优化，不直接当作实时热路径。
3. 挂载真实 Web Arena 页面和只读场景，复用现有框选/下令/建造及网络控制器，做真实新建局→操作→胜负→回放切片；浏览器验收交由父代理安排，本批不操作浏览器。
4. 神经能量/吸收分泌/生死繁殖/遗传谱系的因果闭环、开放规则包实际注入和三方真实共局仍必须继续完成。没有以本批服务端修复代替这些设计目标。
