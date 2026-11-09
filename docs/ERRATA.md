# Rynvek 规范勘误（Errata）

本文件记录实现过程中发现的规范文档内部矛盾及其裁决。裁决原则：当规范的「冻结算法源码 + 外部权威标准」与「示例数值」冲突时，以前者为准，并在此留档。

## E-001：GV-003 Philox4x32-10 期望输出与 Random123 官方向量不一致

- **位置**：《开发规范文档》规范 12.3，`tests/golden_vectors/v1_baseline.json` 中的 `GV-003-PHILOX4X32_10_ORIGIN`。
- **问题**：规范给出的期望输出为 `[1717053460, 2979605061, 2685360627, 3691852427]`（`0x66582814 0xb1992a45 0xa00f59f3 0xdc0d328b`），而该向量的描述明确要求「与 D.E. Shaw Random123 官方标准严格一致」。
  Random123 官方 Known-Answer Test（`kat_vectors`）中 `philox4x32 10` 在全零计数器与全零密钥下的输出为
  `0x6627e8d5 0xe169c58d 0xbc57ac4c 0x9b00dbd8`，即十进制 `[1713891541, 3781805453, 3159862348, 2600524760]`。
- **验证**：规范 02 §2.2 冻结的 WGSL 轮函数（乘数 `0xD2511F53 / 0xCD9E8D57`，Weyl 增量 `0x9E3779B9 / 0xBB67AE85`，10 轮）
  逐位实现后得到的正是 Random123 官方值；同一实现也通过了 Random123 的另外两条官方向量（全 `0xFFFFFFFF`、圆周率位），
  而在 1–16 轮、密钥预先递增等变体中均无法得到规范给出的数值。规范示例数值属于笔误。
- **裁决**：保持规范冻结的算法不变，`v1_baseline.json` 中 GV-003 的 `expected.out_u32x4` 采用 Random123 官方值。
  其余字段、ID 与描述保持原样。

## E-002：Rust 1.85.0 中 `Ord::clamp` 不是 `const fn`

- **位置**：规范 02 §2.1 `Q8_8::mul` 与 `Q8_8::hard_tanh` 声明为 `const fn`，函数体调用了 `i32::clamp` / `i16::clamp`。
- **问题**：在锁定的工具链 Rust 1.85.0 中，`Ord::clamp` 不能在常量函数中调用，规范源码无法编译。
- **裁决**：保留 `const fn` 签名与语义，改用私有常量辅助函数 `const_clamp_i32` / `const_clamp_i16`（与 `Ord::clamp` 行为逐位一致）。

## E-003：`EntityRegistry::sparse_pages` 使用 `Box<[SparsePageEntry]>` 而非 `Box<[SparsePageEntry; PAGE_CAPACITY]>`

- **位置**：规范 04 §4.1 `EntityRegistry` 字段类型。
- **问题**：在 `#![deny(unsafe_code)]` 与 Rust 1.85.0 下，无法在不触碰栈的前提下安全构造 `Box<[T; 65536]>`（定长数组会先在栈上物化）。
- **裁决**：改为 `Vec<Box<[SparsePageEntry]>>`，构造时 `vec![EMPTY; PAGE_CAPACITY].into_boxed_slice()`，并断言长度恰为 `PAGE_CAPACITY`。语义与寻址公式与规范完全一致。

## E-004：SQLite 本地库补充 `local_world_commits` 表

- **位置**：规范 11.2 SQLite DDL 仅有 `local_worlds` / `local_snapshots` / `local_assets_lru` / `user_preferences`。
- **问题**：Postgres 侧有 `world_commits` 提交链；M1 的 `StorageEngine` 与崩溃恢复需要在本地同样持久化 revision→journal/snapshot CID 映射。
- **裁决**：保留规范 11.2 四表原文不变；迁移时额外 `CREATE TABLE IF NOT EXISTS local_world_commits (...)`，字段对齐 Postgres `world_commits` 的本地精简版。记入实现而非修改规范 SQL 文件正文。

## E-005：`profile.test` 启用 `opt-level = 1`

- **位置**：工作区根 `Cargo.toml`；项目文档第九章 M2 验收要求 `10,000 Ticks` 零误差。
- **问题**：debug 默认 `opt-level = 0` 下，五通道 `256×256` 反应-扩散 + 每 Tick BLAKE3 状态根在 10,000 Tick×2 次复跑中耗时不可接受（数分钟级）。
- **裁决**：在 `[profile.test]` 设置 `opt-level = 1`，保留调试符号与断言，使 M2 验收命令在约数十秒内完成；算法与位级结果不变。

## E-006：UniformGrid CSR 4-Pass 基数排序源码未完整给出

- **位置**：开发规范 §6.2 仅冻结 `SpatialCsrIndex` 字段与常量；目录树注明「4-Pass 基数排序」。
- **裁决**：实现 16-bit Morton 键、每趟 4-bit 的 LSD 基数排序，同键内按 `dense_index` 升序整理；CSR `row_offsets` 按行主序格子编号，`sorted_dense_indices` 与之对齐。邻居表取本格及 8 邻格，上限 `MAX_NEIGHBORS_PER_ENTITY = 32`。

## E-007：`ConflictPolicy` 枚举未在规范中给出完整源码

- **位置**：规范 03 / 目录树提及 ConflictPolicy 归约，无枚举定义。
- **裁决**：冻结最小可测策略集 `LowestDenseIndexWins` / `MaxAbsMagnitudeWins` / `SaturatingSum`，供 Phase 08 与规则管线后续挂接；语义在 `rynvek-simulation/src/conflict.rs` 注释中说明。
