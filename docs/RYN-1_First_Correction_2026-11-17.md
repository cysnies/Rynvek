# RYN-1 首批根因纠偏记录

日期：2026-11-17（中国标准时间 UTC+8）。这是局部修复与回归记录，不是完整产品验收，也不是用户同意缩减原设计的 ERRATA。

## 版本和范围

- 基线：最新上传源码完整历史的 `01c1114c014b3bc492f1e384fb363613e336a753`，256 个原始提交；私有远端 `https://github.com/cysnies/Rynvek` 的 main 保留该基线。
- 独立开发目录：`/workspace/rynvek-dev-20261117`；分支 `fix/ryn-1-authoritative-state`。旧 `/workspace/rynvek` 的 `d1916bf` 工作树未覆盖。
- 仅处理基线编译断链、Arena 内的规范提交/快照、实际地形恢复、GPU 场多速率时钟及补丁假成功。不新增 Web 假界面，不声称两套 Runtime 已统一。
- 许可证和 CI 文件不变。唯一新增依赖是工作区已锁定并缓存的外部 `blake3`；未新增内部 crate 依赖边或改变 tier DAG。

## 先复现，再修复

1. 初次最小持久化回归被基线编译错误阻断：预算常量路径、无效快照错误码、缺失 blake3 依赖、哈希 serde/ZERO、注册表参数类型。恢复编译后，三个回归全部失败：Arena 与 WAL 根不同、改地形无法恢复、修改快照外层根仍被接受。
2. GPU 回归另暴露 CPU 能力协商和渲染 Provider 的失效接口。修复编译前置后，非零场在非场更新 Tick 的逐格对照实际失败，nutrient 镜像与 CPU 权威不符。
3. 世界补丁回归先暴露分区 const 求值编译错误；排除后，三个反例失败：未实现操作计数成功、无效后续操作留下前半段地形修改、非法生物质发放被接受。
4. 首次完整 GPU 测试 6 通过、1 失败：拾取成功，但默认实体颜色未使用共享阵营调色板。实现已对齐该调色板，未删改像素断言，复跑全部通过。

## 实现与兼容边界

### 规范根及格式

- `crates/rynvek-journal/src/manifest.rs` 提供唯一的规范根聚合函数，Arena 调用同一函数。保留现有 `RYNVEK_CANONICAL_WORLD_STATE_V2` 标签、tick 和十域顺序，不降低领域覆盖范围。
- 旧 CommitManifest 编码仍为 256B；新增规范编码为 388B：`RYC2` 前缀 4B + 原基础字段 256B + config/evolution/visibility/scheduled_events 四域 128B。显式选择算法，拒绝剥离扩展后静默解释成旧清单。
- 旧通用快照读写保留 v1；Arena 新快照为 `RYSS` format_version=2，加入 `CMIT`（规范清单）和 `MAPT`（实际地形、尺寸、种子）。结构读取器绑定外层根、世界、tick、revision；Arena 从还原状态重新算完整规范清单并核对。
- Arena 不把缺少规范绑定/实际地形的旧 v1 快照直接当作完整恢复，明确返回版本错误。旧快照迁移器尚未实现，使用前必须保留旧文件；不要宣称已经自动迁移存量世界。
- 重复分块被拒绝；快照保存同步文件、rename 后同步父目录。未做真实断电注入验证。
- ENTI 仍是旧工具兼容的计数注册表，不是 Arena 实体权威根。COMP 中的 Arena 序号与状态受到规范 entity_root 绑定，但尚未完成与通用 ECS 生命周期身份的统一。

### 场时钟

- `Engine::gpu_tick` 使用与 CPU 相同的 `MultiRateClock::is_field_step_due()`，真实场更新发生在 1、4、7……Tick，不是简单 tick%3==0。
- 用户编辑的脏态上传优先处理，即使是非场更新 Tick 也立即同步；上传后不重复计算。回读标签仍为真实权威 Tick。
- 40 Tick 用例含一次初始重同步，其余 13 次场更新全部核验、零分歧；不是把 39 改成 13 就算修复，新增逐 Tick 全格比较及非场 Tick 编辑回归。

### 补丁

- `WorldPatch::apply_to_arena` 先预检全部操作，再修改权威状态；校验地形边界、种类、计数溢出以及正整数资源参数。
- 当前真实支持地形涂刷和 `economy.grant_biomass_milli`。其他参数、基因、规则、模板操作在接通前返回现有 `PluginCapabilityDenied`，不再伪造成功计数；能力仍保留在协议设计中，并非取消开放自定义目标。
- 地形变更失效派生流场缓存；原有 NavGrid/HPA 更新和建筑占地保留。
- 该直接调用入口不解决服务器排队补丁未执行、base revision/权限/边界调度的问题。后续应贯通 accepted→scheduled→applied/rejected，而非再加成功回执。

## 实际验证

环境：Rust/Cargo 1.87.0；离线缓存，两个编译任务，复用旧 target，无依赖安装或全量工作区构建。实测适配器为 **llvmpipe (LLVM 19.1.7, 256 bits) / Gl / Cpu，TierC**，即软件 GL 适配器；不是物理 GPU、浏览器 WebGPU 或性能认证。

下面各组共 **172 个不同用例通过，0 失败、0 ignored**；过滤执行的组明确列出过滤范围，重复回跑不重复计数：

| 命令参数（cargo test 之后） | 用例数 |
| --- | ---: |
| `-p rynvek-core -p rynvek-compute-cpu --lib` | 36 + 1 |
| `-p rynvek-journal -p rynvek-snapshot --lib` | 6 + 5 |
| `-p rynvek-rts --lib`（含四脑快照/WAL 崩溃恢复） | 81 |
| `-p rynvek-rts --test state_root_matrix --test ryn1_persistence` | 14 + 4 |
| `-p rynvek-engine --lib provider` | 2（另 8 项未选中） |
| `-p rynvek-engine --test engine_gpu`，设置 `RYNVEK_REQUIRE_GPU=1` | 7 |
| `-p rynvek-world --lib` | 16 |

复现环境变量：

```sh
export CARGO_TARGET_DIR=/workspace/rynvek/target
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=2
cargo test -p rynvek-journal -p rynvek-snapshot --lib
cargo test -p rynvek-rts --lib
cargo test -p rynvek-rts --test state_root_matrix --test ryn1_persistence
cargo test -p rynvek-engine --lib provider
RYNVEK_REQUIRE_GPU=1 cargo test -p rynvek-engine --test engine_gpu -- --nocapture
cargo test -p rynvek-world --lib
cargo test -p rynvek-core -p rynvek-compute-cpu --lib
```

额外检查：触及 Rust 文件 rustfmt 检查通过，`git diff --check` 通过；新增差异高置信度凭据模式检查未命中（不等同完整安全审计）。原有 TickCommit 命名、Contact 未读字段及分区未使用导入警告仍存在；未声称全工作区 clippy/CI 通过。磁盘复查剩余约 19GB；未删除旧 target 或其他用户数据。

## 未完成与下一批

- 当前仅统一 Arena 的规范根→WAL→快照，不是 SimulationEngine/Arena/Server/CAS/Worker 统一权威 Runtime。
- 完整七类 WAL 记录应用/显式拒绝、单调性及世界身份跨输入检查、旧快照迁移、真实 ECS 身份和故障注入仍需补验。
- 服务器补丁应用链、通用规则包实例化、三方控制真实共局、神经能量/生死繁殖/遗传闭环、真实可玩 Web Arena、硬件 GPU 与浏览器性能、数据库、24h soak 均未完成或未验证。
- 下一批先定义统一提交与领域推进接口，贯通 Arena 与 Server/Worker 的一个权威链路；接着把真实补丁事件和统一控制接进该链路，再交付可玩 Web 垂直切片。继续保留神经原生、开放自定义及高性能目标，不把本批通过当作产品完成。
