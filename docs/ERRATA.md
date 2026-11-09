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

## E-008：`NEUR` 分块 v1 补齐神经续跑语义

- **位置**：GAP-09 仅冻结「64-bit `spike_history_ring` + 可塑性偏移零游程编码 + Zstd」；M1 的 `NEUR` 为占位结构。
- **问题**：仅有脉冲历史与权重偏移不足以让 LIF / STDP 从快照**位级一致**地续跑（缺膜电位、不应期计数与资格迹）。
- **裁决**：冻结 `NEUR` v1：16 字节块头（`"NEUR"` | `version u16 = 1` | `reserved u16` | `body_len u32` | `row_count u32`）；
  每行 `dense_index u32 | neuron_index u16 | model_kind u8 | refractory_ticks u8 | spike_history_ring u64 | membrane_v_q8 i16 | ZRLE(weight_deltas) | ZRLE(eligibility)`；
  ZRLE = `total_len u16` 后接若干 `[zero_run u16 | literal_len u16 | i16 × literal_len]`。整个快照负载仍按规范做 Zstd level 3。
  `rynvek-neural::LifBrain::{to_neur_chunk, restore_from_neur}` 为唯一读写方，`tests/neur_snapshot_resume.rs` 验证「快照→恢复→续跑」与不中断运行逐 Tick 状态根一致。

## E-009：三种非 LIF 神经元模型与三种图存储布局的离散化

- **位置**：规范 05 只给出 LIF + 四因子 STDP + Dale 律的冻结源码；`GatedMLP` / `Izhikevich` / `CTRNN` 与 `TopologyStorageClass` 三种布局仅有名称与适用范围（GAP-08）。
- **裁决**（全部为 Q8.8 纯整数，中间量 `i32`，每个突触项先 `(w*x)>>8` 再求和，因此三种布局结果位级相同）：
  - `GatedMLP`：`h = hard_tanh(W_in·x)`，`g = hard_sigmoid(W_gate·x + b_gate)`（`clamp(z/4 + 0.5, 0, 1)`），隐藏层 `g ⊙ h`，读出 `hard_tanh(W_out·(g ⊙ h))`；
  - `Izhikevich`：RS 参数 `a=0.02, b=0.2, c=-65 mV, d=8`（Q8.8 原始值 `5 / 51 / -65·256 / 8·256`），每 Tick 两个半步积分，输入增益 16，峰值 30 mV 复位；
  - `CTRNN`：`s += inv_tau · (-s + W·σ(s) + I)`，`σ` 为 hard sigmoid，状态钳制在 `±8.0`；
  - 存储：`DenseTiledMatrix`（`16×16` 瓦片行主序，N ≤ 64）、`BlockSparseCSR`（`8×8` 块级 CSR，N ≤ 4096）、`PagedDynamicAdjacency`（每个突触后神经元固定 32 槽，空槽 `u16::MAX`），由 `select_storage_class` 按 GAP-08 阈值选择；超出容量报 `NeuralTopologyCapacityExceeded (0x4001)`。

## E-010：Typed SSA Rule IR 的 36 条指令表、规则图 JSON、`.rir` 布局与 WGSL 绑定

- **位置**：规范 08 只冻结四阶段管线、`.rir` 魔数 `RYIR`、「36 条指令」与错误码 `0x5001–0x5006`，未列出指令清单与编码。
- **裁决**：
  - **指令表**（操作码 = 下标）：`0–3` 四类常量；`4–12` `Add/Sub/Mul/Div/Neg/Abs/Min/Max/Clamp`；`13–18` 六种比较；`19–22` `And/Or/Not/Select`；`23–25` 三种 Cast；`26–27` `LoadComponent/EmitComponentWrite`；`28–29` `SampleField/EmitFieldDeposit`；`30–31` `NeighborCount/LoadNeighborComponent`；`32` `RandomQ8_8`；`33–35` `BoundedForBegin/BoundedForEnd/Phi`。TS（`OPCODES`）与 Rust（`Opcode::ALL`）两侧顺序一致并有测试锁定。
  - **类型与精确语义**：`Void/Bool/I32/Q8_8/Q32_32`，混合运算按 `I32 < Q8_8 < Q32_32` 提升，Pass 1 显式插入 Cast；禁止隐式窄化写入。`Q8_8` 加减乘饱和、除法向零截断、除零得 0；`I32→Q8_8` 先钳制到 `[-128, 127]`；`Q32_32` 乘法按 `rynvek-core` `i128 >> 32` 截断。**`Q32_32 Div` 在 Tier-0 中被拒绝（0x5001）**：WGSL 无 64 位整数，128 位除法模拟代价与验证成本过高，留给 Tier-1。
  - **随机数**：`RandomQ8_8` = Philox 输出字 0 的高 8 位（`[0,1)`），计数器为 `[sub_counter, pack_stream_and_phase(stream, 7), lo(serial), hi(serial)]`，规则阶段 ID 固定为 `7`。
  - **规则图 JSON**：`format = "rynvek.rule-graph.v1"`，`{name, reads[], writes[], nodes[]}`，节点字段顺序 `op, args, imm, component, channel, maxIters, stream`，`imm` 为十进制字符串；场写入以 `field.<channel>: Q8_8` 声明。规范文本为 2 空格缩进 + 末尾换行，TS `canonicalGraphJson` 与 Rust `serde_json::to_string_pretty` 逐字节一致（`tests/rules/forager_metabolism.graph.json` 双端校验）。
  - **`.rir`**：`"RYIR" | version u16 | …`（布局见 `rynvek-rules/src/rir.rs`），尾部 BLAKE3 完整性校验；解码后经 `to_graph` 重新走完整编译管线并要求结果相等，防止伪造字节码绕过静态验证。
  - **Gas 估算**：每条指令固定成本 × 所在嵌套循环上界之积；循环无提前退出，CPU 实测 Gas 与估算精确相等。
  - **WGSL**：工作组 `128×1×1`，入口 `rule_main`；`@group(0)` 参数与实体序号、`@group(1)` 组件输入、`@group(2)` 组件输出、`@group(3)` 场采样/沉积与 CSR 邻居表；`Q32_32` 以 `vec2<u32>` 模拟，Philox 前导与规范 02 逐字一致。
  - **ConflictPolicy 检查**：Pass 2 目前只做读写集与能力令牌（0x5002 / 0x5006）检查；同一组件的多写者冲突归约依赖 Phase 08 的 `ConflictPolicy`，而 GAP-04 要求的 8 种策略当前只实现了 E-007 中的 3 种，补齐后再在 Pass 2 挂接（M3 未改动 `rynvek-simulation`）。

## E-011：Tier-1 QuickJS-NG 沙箱的 Gas 计量与确定性约定

- **位置**：GAP-06 / 规范 01 §1.1 冻结「QuickJS-NG 确定性沙箱、单 Tick `500,000 Gas`、堆 `64 MiB`」，未定义 Gas 单位。
- **裁决**：
  - **实现**：Rust 侧 `rquickjs 0.11`（内嵌 QuickJS-NG 0.11.0，MSRV 1.85）；TS 侧 `quickjs-emscripten-core 0.32.0` + `@jitl/quickjs-ng-wasmfile-release-sync 0.32.0`（QuickJS-NG 0.12.1 WASM），版本精确锁定。
  - **Gas** = 中断回调次数 × `10,000`（QuickJS-NG 每 `10,000` 次中断计数递减回调一次），超预算即不可捕获中断 → `0x5004`；`JS_SetMemoryLimit` 超限 → `0x5005`；脚本语法/运行时错误与缺少 `tick` → `0x5001`。
  - **确定性**：每 Tick 新建 Context（中断计数器挂在 Context 上，新建即复位，Gas 逐次相同、无跨 Tick 状态）；只注册 `BaseObjects/Eval/JSON/MapSet/TypedArrays/BigInt/RegExp`，无 `Date/Promise/WeakRef/Proxy`；`Math.random` 替换为抛错且不可改写；脚本一律严格模式求值；随机数只能来自宿主 `Rynvek.philox4x32_10`。
  - **跨宿主**：两端 QuickJS-NG 小版本不同，只保证**输出 JSON** 逐字节一致（`tests/rules/tier1_metabolism.*` 双端校验），Gas 数值在同一宿主内确定，跨宿主不承诺相等。
  - Tier-2 Wasmtime WIT 组件插件不在 M3 范围内。

## E-012：WGSL 校验使用 `naga 28` 而非 `wgpu 30` 配套的 `naga 30`

> **状态：已被 E-013 取代（M4）。** 工具链已上调至 Rust 1.87.0，`rynvek-rules` 的 WGSL 校验 dev 依赖已升级为与 `wgpu 30.0.1` 配套的 `naga 30`；规则 WGSL 的 GPU 实跑由 `rynvek-engine` 的差分测试覆盖。以下为 M3 时的原始裁决，仅作历史留档。

- **位置**：技术栈冻结 `wgpu 30.0.1`；工具链锁定 Rust 1.85.0。
- **问题**：`naga 30` 要求 Rust ≥ 1.87，无法在锁定工具链下编译；M3 环境也没有可用 GPU。
- **裁决**：`rynvek-rules` 仅以 dev-dependency 引入 `naga 28`（`wgsl-in`）对生成的 WGSL 做解析 + 全量校验（入口、工作组大小均断言）；GPU 端数值等价性由 `wgsl.rs` 中逐行移植的 `vec2<u32>` Q32 辅助函数与 CPU 精确语义的逐位比对测试保证。M4 引入 `wgpu 30` 时需先解决工具链版本冲突（升级到 1.87+ 或选用兼容版本），届时再补 GPU 实跑测试。

## E-013：工具链由 Rust 1.85.0 上调至 1.87.0，WGSL 校验升级到 `naga 30`

- **位置**：技术栈冻结 `wgpu 30.0.1`（规范 07 §7.1）；工作区原锁定 Rust 1.85.0（`rust-toolchain.toml`、`rust-version`）。
- **问题**：`wgpu 30.0.1` 及其依赖 `wgpu-core / wgpu-hal / naga 30` 声明 MSRV 1.87，在 1.85.0 下无法编译。规范同时要求 wgpu 精确为 30.0.1、严禁降级。
- **裁决**：
  - 选择能编译 `wgpu 30.0.1` 的**最低**稳定版本 **Rust 1.87.0**（`rust-toolchain.toml` 的 `channel` 与 workspace `rust-version` 同步），edition 仍为 2024；不使用更高版本，避免引入 1.88+ 的语言特性（如 `let` 链），已有代码按 1.87 语法改写。
  - workspace 依赖 `wgpu = "=30.0.1"` 精确锁定；`rynvek-rules` 的 dev 依赖 `naga` 由 28 升级为 30（与 wgpu 同一版本线，WGSL 前端行为一致）。
  - 原 E-002（`Ord::clamp` 非 `const fn`）的变通实现保持不变：1.87 下仍不可在 `const fn` 中调用 `clamp`。
  - E-012 由本条取代。

## E-014：GPU 计算层（`rynvek-wgpu`）中规范未给出的约定

- **位置**：规范 07 §7.1–7.2 只冻结了初始化序列、`Tier A–D` 名称、`32 MiB` 分段、`128 MiB` 上限、`3-Slot Readback Ring` 与错误码 `0x6001–0x6004`，以及目录树中的「BindGroup 0-3」。
- **裁决（最佳实践补全）**：
  - **档位判定阈值**：不满足计算最低要求（4 个绑定组、每阶段 ≥8 个存储缓冲、工作组 ≥128×1 与 8×8、存储绑定 ≥ 32 MiB）→ `TierD`；软件适配器（`DeviceType::Cpu/Other`）或 `max_buffer_size < 256 MiB` → `TierC`；独显且缓冲与存储绑定 ≥ 1 GiB、单工作组调用 ≥1024 → `TierA`；其余 `TierB`。
  - **BindGroup 职责**：0 `Frame`（Uniform 参数 + 帧常量，`@binding(15)` 固定为 `s_diagnostics` 4×`atomic<u32>` 诊断计数器）；1 `StateIn`（只读，`Committed[t-1]`）；2 `StateOut`（读写，`CandidateState[t]`）；3 `Auxiliary`（场采样/沉积、CSR 邻居、统计摘要）。`KernelLayout::validate` 在构建期拒绝越组的绑定类型。与 M3 规则 WGSL 的绑定一致。
  - **工作组尺寸**：线性内核 `128×1×1`，二维内核 `8×8×1`（规范值），由 `linear_workgroups / grid_workgroups` 统一计算派发数。
  - **原生后端回退**：规范要求 `BROWSER_WEBGPU | PRIMARY`；原生环境若 PRIMARY 无适配器（如无 Vulkan 驱动的 CI 容器），再以 `GL` 后端重试一次（浏览器目标不回退）。`WGPU_BACKEND` 环境变量可覆盖，空字符串视为未设置。
  - **软件适配器与测试跳过策略**：GPU 集成测试在无适配器时打印原因并跳过（保持验收绿色）；设置 `RYNVEK_REQUIRE_GPU=1` 时无适配器即失败。llvmpipe / SwiftShader 等软件适配器被判定为 `TierC`，但仍真实执行全部内核。
  - **非阻塞回读**：`ReadbackRing` 严格使用 `Slot[tick % 3]`；槽忙时**跳过**本 Tick 的回读（计数 `skipped`），已就绪未取走即被覆盖计 `dropped`，从不等待 Fence。DeviceLost 恢复第 5 步的校验使用独立的一次性回读 `OneShotReadback`（同样只轮询不阻塞），以 Philox 探针结果逐位比对 CPU 期望值。
  - **单体缓冲上限**：`create_storage_buffer` 对超过 `128 MiB` 的请求返回 `0x6003`；大于一个 Segment 的数据由 `SegmentedBufferPool` 以 `33,554,432 B` 切分。
  - **恢复状态机**：DeviceLost 后按 5 步执行——① 隔离（停止提交）② 释放旧资源 ③ 在**同一** `wgpu::Instance` 上重获设备（代数 +1）④ 重建管线并从 CPU 权威状态上传 ⑤ 校验并恢复；失败次数达到上限（缺省 3）即降级为 CPU Reference 并上报 `BACKEND_DEGRADED`。

## E-015：渲染层（`rynvek-render`）L0–L7 图层与神经 LOD 阈值

- **位置**：项目文档描述了世界视图的图层与「缩放决定神经细节」，但未给出图层编号与阈值。
- **裁决**：
  - **图层**：L0 Terrain、L1 FieldHeatmap、L2 Structures、L3 Organisms、L4 Bonds、L5 NeuralOverlay、L6 FogOfWar（不可拾取）、L7 Overlay；`RenderLayerMask(u8)` 位 `1 << n` 对应 Ln，`@rynvek/engine` 的 `RenderLayer` 与之一致。
  - **神经 LOD**（按 `px_per_cell`）：L0 `< 2`、L1 `[2, 8)`、L2 `[8, 32)`、L3 `[32, 128)`、L4 `≥ 128`；32×32 像素网格下 `px_per_cell = 32` 为 1:1。
  - **拾取**：额外渲染到 `R32Uint` ID 目标，`PickId = dense_index + 1`（0 表示未命中），经单像素非阻塞回读返回。
  - **色板**：背景 `#090B0A`、网格 `#303A34`、强调色 `#A3D65C`；阵营色采用色盲友好的 Okabe-Ito 色板；热力图由 `#0072B2` 渐变到 `#F0E442`。

## E-016：引擎门面、Worker 协议、许可证与依赖调整

- **位置**：规范 07 §7.1 第 1 条只给出 `ENGINE_INIT` 的 `postMessage` 形态；目录树列出 `rynvek-engine`、`rynvek-wasm` 与 `@rynvek/engine`，未给出消息清单与许可证归属。
- **裁决**：
  - **CPU 权威 / GPU 镜像**：`rynvek-simulation` 的 CPU Reference 是 D0 确定性权威，状态根只由 CPU 产生。GPU 维护五通道场的驻留镜像，每 Tick 推进后把行和摘要经回读环异步送回，与 CPU 摘要逐位比对；不一致即上报 `GPU_DIVERGENCE` 并从 CPU 重新上传。GPU 结果从不写回权威状态，因此 DeviceLost、跳过回读与降级均不改变状态根（测试逐 Tick 断言）。CPU 摘要历史保留 64 Tick，送达时已过期的回读计为 `stale_readbacks`。
  - **恢复起点**：在两个 Tick 之间侦测到的 DeviceLost，恢复后从 `last_tick + 1` 继续（`RECOVERED.resumeTick`）。
  - **消息协议**：`type` 为 SCREAMING_SNAKE_CASE、字段为 camelCase。命令 11 种：`ENGINE_INIT / RESIZE / STEP / START / PAUSE / SET_CAMERA / PICK_REQUEST / SET_LAYER_MASK / SET_HEATMAP_CHANNEL / SIMULATE_DEVICE_LOST / SHUTDOWN`；事件 10 种：`ENGINE_READY / TICK_COMMITTED / FRAME_RENDERED / PICK_RESULT / DEVICE_LOST / RECOVERY_PROGRESS / RECOVERED / BACKEND_DEGRADED / GPU_DIVERGENCE / ENGINE_ERROR`。共享夹具 `tests/engine/protocol_messages.json` 由 Rust（serde 往返）与 TS（运行时校验）双端断言。
  - **Worker 宿主语义**：`ENGINE_INIT` 在 Worker 生命周期内只允许一次（主线程 `EngineHost` 与 Worker `WorkerHost` 双端拒绝二次移交）；异步初始化期间到达的命令按序缓存（上限 256）并在 `ENGINE_READY` 后回放；协议违例与无法解析的消息以 `0x3003 SimulationInvariantViolated` 上报（规范未定义协议错误码，复用宿主不变量违例），非法尺寸 / `worldId` 用 `0x1004`。`gpuMode`：`auto` 无设备即 CPU 运行，`required` 无设备即 `0x6001` 并终止，`disabled` 不请求设备。
  - **帧调度**：Tick 只以整数推进，帧时间经固定步长累加器折算，单帧最多追赶 4 Tick，积压整体丢弃。
  - **浏览器重获设备**：`requestDevice` 在浏览器中只能异步完成。引擎在第 3 步挂起（`awaiting_external_reacquire`），由 `rynvek-wasm` 取出设备所有者、`await reacquire()` 后归还；Surface 由同一 Instance 创建，按设备代数重新配置。
  - **`unsafe_code`**：全工作区 `deny`；仅 `rynvek-wasm::bridge`（wasm-bindgen 导出符号）`allow`，模块内无手写 `unsafe`。
  - **许可证**：`@rynvek/engine` 与 `rynvek-wasm` 均为 `AGPL-3.0-only`。GAP-15 的 Apache-2.0 WITH LLVM-exception 客户端 SDK 清单不含二者，它们分发的是引擎本体。
  - **同层依赖**：`rynvek-engine`（T4）依赖 T0–T3；`rynvek-wasm`（T4）依赖 `rynvek-engine`（同层，单向）。wgpu 内核（T2）不依赖同为 T2 的 `rynvek-field / rynvek-neural`，CPU/GPU 差分测试放在组合二者的 `rynvek-engine` 中，DAG 保持严格单向。
  - **`rynvek-rules` 特性**：workspace 依赖改为 `default-features = false`，使引擎依赖树不引入 QuickJS（`rynvek-rules` 自身仍默认启用 `quickjs`，其测试不受影响）。
  - **wasm32 构建**：`rynvek-storage` 的 SQLite 适配（bundled libsqlite3，需要 libc）仅在非 wasm32 目标编译，浏览器持久化留待 OPFS。仅阻塞回读的测试辅助接口（`*_blocking`、规则 GPU 执行器）不在 wasm32 编译。`zstd-sys` 自带 wasm shim，但需要能产出 wasm32 的 C 编译器。本机没有 clang，验证时借用 `zig cc` 包装器（`CC_wasm32_unknown_unknown`），它不属于仓库依赖。

## E-017：设计系统（`@rynvek/ui`）令牌取值、主题层与动作表归属

- **位置**：项目文档 6.1 与 GAP-14 只冻结了品牌三色（`#090B0A / #303A34 / #A3D65C`）、Geist Sans/Mono 字体、三主题名称（Neutral Dark / Scientific Light / Okabe-Ito）与五层隔离优先级 `Accessibility > Visualization > Faction > World Biome > Application UI`，未给出完整令牌表。
- **裁决（最佳实践补全）**：
  - **令牌清单**：应用层 `--rv-color-*`（bg / surface / surface-raised / overlay / border / border-strong / text / text-muted / text-inverse / accent / accent-hover / accent-text / focus-ring / success / warning / danger / info）及版式、间距、圆角、动效、层级令牌；世界层 `--rv-world-bg/grid/outline/fog`；阵营层 `--rv-faction-1..7`；可视化层 `--rv-heat-low/mid/high` 与 `--rv-neural-excitatory/inhibitory/spike/sync`。中文回退字体为 Noto Sans SC。
  - **取值**：品牌色以外的颜色按 WCAG 2.2 定（正文 ≥ 7:1，次要文字与强调文字 ≥ 4.5:1，强调色、焦点环与语义色 ≥ 3:1），单测逐主题审计。Scientific Light 中青柠在浅底上对比度不足，改用同色相深橄榄绿 `#4C7A1C`。阵营色在三主题中一致，取 Okabe-Ito 1..7，与 `rynvek-render::OKABE_ITO` 逐项相同（单测解析 Rust 源码比对）。
  - **热力图中点**：`--rv-heat-mid` 在三主题中都取 `#0072B2 → #F0E442` 的 8 位 sRGB 线性中点 `#78AB7A`，与 GPU 着色器 `mix(heat_low, heat_high, 0.5)` 一致，只用作图例刻度。
  - **层隔离**：低四层只能写本层命名空间的令牌，越界即抛错。无障碍层可以覆盖任意令牌，且总是最后应用。来自 `.rview` 等不可信来源的值经 `assertSafeCssValue` 校验，拒绝 `; { } < > \`、`/*`、`url(`、`expression(`、`@import`，长度上限 256。
  - **动作表**：规范 12.1 的 36 项绑定表（键盘组合、指针手势、手柄、编队数字组）放在 `@rynvek/ui`，供工具栏 `Kbd` 提示、命令面板与 Studio 共用。运行时全局路由器 `@rynvek/client::InputActionRouter` 属于 M6，届时直接包装 `matchKeyboard`。

## E-018：音频运行时（`@rynvek/audio`）中规范未给出的部分

- **位置**：规范 12.2 冻结了 `ProceduralPlaceholderSynth` 与 24 类配方源码；项目文档 6.2 只描述了五大总线、`AudioManifest` 的 `status: "placeholder"` → `.ogg` 热替换，以及 120 BPM 六轨自适应音乐。
- **逐字实现**：`placeholder_synth.ts` 只汉化了注释，代码逐字照搬规范。单测去掉注释后与规范代码块逐字比对，任何偏差都会失败。
- **已知行为（不修改）**：规范粉红噪声的“白噪声”源以样本下标为 LCG 种子，相邻样本相差 `1664525 / 2^32`，实为周期约 2580 样本的锯齿波，不是白噪声。经三极点漏积分后，48 kHz 下峰值约 4.69，超过 ±1。混入后再经包络（峰值 ≤ 0.35）与总线增益衰减，Web Audio 浮点管线在 destination 之前不会削波。按“不改变算法”的要求保留原样，单测锁定该确定性峰值。若后续规范修订，可改为 `white = lcg(state)` 的递推形式。
- **补全**：
  - **混音**：总线拓扑为 `bus GainNode → master → destination`。缺省电平：sfx_ui 0.8、sfx_world 0.7、sfx_combat 0.75、voice_alert 1.0、neural_sonify 0.5。电平变化用时间常数 15 ms 的 `setTargetAtTime` 平滑，避免爆音。
  - **静音循环**：`global.cycle_audio_mute`（动作 07）按 `none → sfx（仅保留 voice_alert）→ all → none` 循环，关键告警不会被一键静掉。
  - **`AudioManifest` v1**（`protocols/schemas/audio-manifest.schema.json`，Apache-2.0 WITH LLVM-exception）：
    - 结构为 `schema: "rynvek.audio-manifest.v1"`，`events` 由事件名映射到条目。
    - 条目共有字段：`bus`，可选 `gain / cooldown_ms / max_voices`。
    - `placeholder` 条目必须对应 24 类配方之一，且总线与配方一致。
    - `asset` 条目须给出相对 `.ogg` 路径（禁止 `..`、绝对路径与 URL），许可证限 `CC0-1.0 / CC-BY-4.0`。
  - **AudioDirector**：
    - 同一事件按总线设缺省冷却：UI 25 ms、世界 40 ms、战斗 20 ms、告警 1500 ms、神经 30 ms。
    - 同一事件最多 4 个并发发声；每条总线另有并发上限：8 / 16 / 24 / 2 / 12。
    - `.ogg` 未加载或加载失败时，回退到同名占位合成；`unlock()` 在用户手势中恢复 AudioContext。
  - **自适应音乐**：
    - 节拍：4/4 拍，一小节 2 秒；状态切换在下一小节线开始，以一拍（0.5 秒）线性淡变；调度提前量 50 ms。
    - 状态：项目文档列出的五个状态 `quiet_observation / base_building / mass_production / skirmish / final_alert` 逐级累加音轨：A → A B → A–D → A–E → A–F。
    - 场景数：D617–D637 摘要写作“6 场景”，但正文只列出五个状态。实现以正文为准，留待规范澄清。
- **测试**：`@rynvek/audio/testing` 提供录制型 `FakeAudioContext`，按 Web Audio 规范的线性、指数与 setTarget 插值公式求 AudioParam 值，包络与音乐淡变据此做确定性数值断言。

## E-019：`.rview` 视觉包容器、WGSL 安全审查与资产错误码

- **位置**：规范 10.1 只冻结了魔数 `RYVW`、Little-Endian 与 64 字节对齐文件头。项目文档只规定 TrustLevel 3 的 WGSL 片段着色器指令数 ≤ 512。规范 11 给出 `0x7001 / 0x7002 / 0x9003 / 0x9004`。
- **容器**（`@rynvek/assets`）：
  - **文件头**：沿用 `.rsnap` 的字段位置。
    - `format_version = 1`；`flags` 在 v1 必须为 0；`header_bytes = 64`。
    - `entry_count` 为 u16。
    - `package_key` 取 BLAKE3(包 ID) 的前 16 字节。
    - 三个 u64：`manifest_bytes`、`payload_bytes`、`total_bytes`。
    - 保留字段必须为 0。
  - **头后结构**：
    - 文件头之后是 32 字节 `content_hash = BLAKE3(manifest ‖ payload)`。
    - 然后是 UTF-8 清单 JSON，补零到 8 字节对齐，再接载荷区。
    - 条目按路径升序、8 字节对齐紧密排布，各带一个 BLAKE3。
  - **校验结果**：同一输入总是写出逐字节相同的文件。结构、边界、清单、截断或尾随数据错误报 0x7001；整包或条目摘要不符报 0x7002。
- **清单**（`protocols/schemas/rview.schema.json`）：
  - 字段：`schema: "rynvek.rview.v1"`、`id`（如 `rynvek.visual.industrial-default`）、`version`（semver）、`name`、`license`（`CC0-1.0 / CC-BY-4.0 / AGPL-3.0-only`）、`engine_compat`、`entries[]`。
  - 条目类型 `palette / theme / shader / sprite_atlas`，各有允许的媒体类型。
  - 内容格式：
    - 色板为 `{colors: {name: "#RRGGBB"}}`。
    - 主题为 `{tokens: {"--rv-*": value}}`。
    - 着色器为 WGSL 文本。
    - 图集为 PNG，或 `{image, frames}`。
- **依赖锁**：给定 `dependencies.lock` 时，包必须恰好出现一次，`kind = "rview"`、版本相同，整文件 BLAKE3 等于 `blake3_256`，否则报 0x9003。锁冲突属于“不是锁定的那个包”，不是存储损坏，因此不用 0x7002。
- **WGSL 审查**：浏览器端无法调用驱动编译器，改用可复现的保守估算，违规报 0x9004。
  - **指令数**：运算符、函数或构造器调用、赋值、下标访问与控制流语句各计 1 条。自定义函数按调用点展开计入入口函数；递归视为违规。
  - **结构要求**：源码 ≤ 64 KiB，恰好一个 `@fragment` 入口。
  - **TrustLevel ≥ 2 的禁用项**：`@compute / @vertex / @workgroup_size`、存储缓冲、原子操作、工作组变量与屏障、`textureStore` 与存储纹理、`enable` 扩展。这些能力会越过只读表现层的边界。
  - **预算**：TrustLevel 3 另外禁止一切循环（静态无法给出上界），预算 512；TrustLevel 2 预算 2048；TrustLevel 0/1 不限。

## E-020：可视化（`@rynvek/visualization`）的色标、时间序列与 `.rview` 主题桥接

- **神经 LOD**：阈值与可见图层沿用 E-015，TS 侧与 `rynvek-render::NeuralLod` 一致，单测解析 Rust 源码比对。UI 不加滞回，保证与 GPU 渲染层的判定逐帧一致。
- **色标**：热力图与 GPU 相同，在 8 位 sRGB 空间线性插值。阵营 `index` 映射到 `OKABE_ITO[1 + index % 7]`。突触颜色按权重符号取兴奋或抑制色，`|w| / w_max` 作不透明度。同步环颜色按 Kuramoto 序参量 `r = |mean(e^{iθ})|` 在网格色与 `--rv-neural-sync` 之间插值。GPU 色板按 `Rgba8::packed` 的小端 RGBA 打包；迷雾取世界背景色叠 `0xC0` 不透明度。
- **L4 时间序列**：定长环形缓冲按 Tick 追加，跳号以 NaN 补洞。降采样逐桶取 min/max，单个动作电位在任意缩放下都不会被抹掉，结果只取决于输入与桶数。
- **`.rview` → 主题层**：
  - 主题令牌按所属命名空间落到 world / faction / visualization 层；色板中名为令牌后缀的颜色（如 `faction-1`）同样落层。
  - 应用层令牌只接受 TrustLevel 0/1（官方皮肤），社区包与临时包改写应用 UI 有仿冒风险。
  - 未知令牌与不安全值逐个剔除并报告，不连累整层。

## E-021：Studio 创作态补丁的 CRDT 合并、安全分级映射与可停靠工作区

- **位置**：
  - 规范 10.2 给出 `LiveEditSafetyClass / HlcTimestamp / WorldPatch / WorldPatchOp` 的 Rust 定义。
  - 项目文档 GAP-25 所指的“规范 10.4（`AuthoringDocument`、三方语义合并器规则）”在开发规范中不存在。
  - `LiveEditSafetyClass` 未派生 serde，但 `WorldPatch` 派生了 `Serialize`，按原文无法编译。
- **裁决**：
  - **serde 形态**：`LiveEditSafetyClass` 按 serde 对单元变体的缺省形态序列化为变体名字符串，例如 `"EpochBoundarySafe"`；Rust 侧实现时补上 `serde::Serialize / Deserialize` 派生即可。其余字段遵循 serde 缺省：`[u8; N]` 为整数数组，`WorldPatchOp` 为外部标记。TS 序列化与 serde_json 逐字节一致（夹具 `packages/studio/test/fixtures/world_patch.json`）。补丁格式错误复用 `0x3003`，与 E-016 中 Worker 协议违例一致。
  - **AuthoringDocument（CRDT）**：
    - 结构：文档 = 只读基线 + 按 `patch_id` 去重的补丁集合。
    - 寄存器：世界参数键、地形格、基因组 slug、规则模块 ID、实体模板 ID 各为一个 LWW 寄存器。写入戳为 `(HLC, patch_id, 操作下标)`，戳大者胜。
    - 一致性：整合满足交换律、结合律与幂等性，单测覆盖 6 个并发补丁的全部 720 种到达顺序和 200 个随机补丁的多种乱序，状态哈希完全一致。
    - 原子性：补丁整合前整体校验，涂刷越界或分级声明不足时整个补丁被拒绝。不同作者写同一寄存器时给出胜者与败者，供 UI 提示。
  - **语义约定**：
    - `new_value_json == "null"` 表示删除参数。
    - 实体模板只能删除；规范操作集没有恢复操作，因此删除不可逆。新建的基因组和规则模块同理，没有删除操作。
  - **撤销 / 重做**：协作环境下不回滚历史，而是追加逆补丁。只回退仍由本人补丁胜出的寄存器，已被他人覆盖的跳过并报告；若某次编辑已被完全覆盖，就继续撤销更早的一次。地形逆操作压缩为行程矩形。撤销栈上限 256。
  - **安全分级映射**：
    - Class 0：`SetWorldParam` 顶层命名空间 `presentation / visual / ui / audio`。
    - Class 1：`field / economy / buildings / units`，以及 `PaintTerrainRect`、`RemoveEntityTemplate`。
    - Class 2：`neural / rules`，以及 `UpsertSeedGenome`、`UpdateRuleModule`（规范明确）。
    - Class 3：`world.*` 与所有未登记的命名空间（最保守）。
    - 补丁声明可以比所需更保守，不能更低。
  - **生效时机**：Class 0 在下一渲染帧；Class 1 在下一 Tick 的 Phase 01；Class 2 在严格晚于当前 Tick 的下一个 `tick % 4 == 0`；Class 3 产生 WorldFork 请求。同批次按 HLC 全序输出。
  - **HLC**：采用标准 Kulkarni 算法。远端时间领先本地 60 秒以上时拒绝；`logical_counter` 溢出 u32 时向墙钟借 1 毫秒，保持严格单调。
  - **Dockable Workspace**：
    - 布局模型：布局树由 split（row / column 加比例）与 tabs 组成，所有操作都是纯函数。规范化会删除空组、折叠单子分栏、合并同向嵌套分栏，并把比例归一（最小 0.1）。持久化 JSON 带版本号，并校验 ID 与面板唯一。
    - 缺省布局：包含项目文档 3.1 的五个面板。
    - React 视图交互：标签采用 roving tabindex，方向键切换、Delete 关闭；分隔条遵循 WAI-ARIA Window Splitter 模式，支持方向键与 Home/End；标签可拖放停靠到中心或四边。
    - 拖放状态：保存在组件内而非 DataTransfer，既便于无头测试，也能阻止从外部窗口拖入伪造面板。
  - **快捷键**：Studio 动作 30–36 由 `StudioCommands` 分派，播放频率缺省 30 Hz。在输入框内不带修饰键时不触发单字母快捷键。`Ctrl+S` 只在规则编辑器（`Code_Editor` 上下文）生效。
- **构建与测试约定**：
  - **测试**：依赖工作区包的 TS 包在 vitest 中把 `@rynvek/*` 别名到源码，`pnpm --filter <pkg> test` 无需预先构建。React 组件测试文件以 `// @vitest-environment jsdom` 声明环境。
  - **typecheck**：在 turbo 中先构建依赖包（`dependsOn: ["^build"]`）。
  - **许可证**：五个 M5 包均为 `AGPL-3.0-only`。`protocols/schemas/*.json` 随协议层采用 `Apache-2.0 WITH LLVM-exception`（REUSE.toml 批量声明）。

## E-022：竞技地图连通性形式化、地形与环境场开局补全（`rynvek-rts`）

- **背景**：地图生成 `generate_symmetric_arena_map`、四种标准尺寸与 180° 中心对称按规范原样实现、未作改动；但以下几点规范未定义。
- **裁决**：
  - **连通性**："双出生点之间至少 2 条宽度 ≥ 6 Tiles 的通路"按 Menger 定理形式化为 4-邻接可通行图上两出生点间的最小顶点割 `κ ≥ 12`，以单位容量最大流精确求解（至多 12 轮 BFS 增广）。采用 4-邻接，避免把对角墙缝计入宽度，偏保守。
  - **裂隙移速**：`AcidicFissure` 的移速系数规范未给出，取 1.00x；灼烧伤害 `1.5/Tick` 按规范。
  - **开局营养基底**：`NutrientMarsh` 为 `80.0`（即再生上限），`OpenSubstrate` 为 `20.0`，其余为 `0`。
  - **跨 Chunk 扩散**：复用 `rynvek-field` 的冻结五点拉普拉斯核。每步先填 Neumann Halo，再用相邻 Chunk 内区覆盖共享边；地图边界零通量。

## E-023：建筑视野与主脑自卫炮

- 规范只给出主脑视野 `24 Tiles`。其余建筑取 `8 Tiles`，脉冲防御塔取其射程 `12 Tiles`。
- 主脑自卫炮的射程 `10`、伤害 `20`、冷却 `15 Ticks` 按规范；伤害类型规范未给出，按"脉冲"取 `Kinetic`。

## E-024：伤害类型克制公式

- 规范只给出武器数值与伤害类型，没有克制系数。全部按整数 milli 运算：
  - **护盾系数**：Kinetic 100%、BioElectric 150%、Corrosive 50%。护盾耗尽后，余量按系数倒数折回原始伤害继续结算。
  - **护甲减免**：对生命值的伤害为 `max(原始 − 护甲 × 护甲系数, 原始 × 10%)`。护甲系数 Kinetic / BioElectric 为 1.0，Corrosive 为 0.5（腐蚀穿甲）。
  - **真实伤害**：环境危害场与裂隙灼烧绕过护盾与护甲。

## E-025：种子基因组初始权重、感知编码、调质作用与阵营参考系

- **W₀ 构造**：神经元模型、拓扑、`E₀` 与存储布局按规范冻结。W₀ 的具体数值规范未给出，按以下规则构造：
  - 先天行为偏置：每条偏置为一条"感知 → 专属隐藏神经元 → 动作"通路，权重 `±2.0`。有符号感知量拆成正负一对神经元。
  - 其余突触：由 Philox 抽位补齐（密钥取 `BLAKE3(genome_slug)` 前 8 字节），幅值 `[0.03, 0.125]`、随机符号。
- **16 维 Q8.8 感知编码**：
  - 场梯度：中心差分 × 8 后钳制。
  - 场强：营养 / 危害 / 能量按 `[0,100] → [0,1]` 映射，信号 B 按 `[-50,50] → [-1,1]` 映射。
  - 最近敌 / 友：取方向单位向量。
  - `HP_FRACTION`：当前生命 / 最大生命。
  - 通道 14 / 15：由 DA / NE 注入。
- **调质作用**：
  - LIF 脑把调质直接传入冻结核（NE 降阈值，DA 门控 STDP）。
  - ACh：感知增益 `1 + 0.5·ACh`。
  - 5-HT：`a_attack −= 0.5·max(5-HT, 0)`。
- **脑推理频率**：每个 Coupling Epoch（4 Ticks）推进脑 `BRAIN_STEPS_PER_EPOCH = 4` 步，读出取算术平均。
- **阵营参考系**：奇数号阵营使用 180° 旋转参考系，双方单位脑看到同一个以己方出生点为原点的世界，保证 PvP 绝对公平。

## E-026：PvE 虫潮未定细节

- **基因池**：规模 20。
- **初始配比**：Forager 30% / Striker 40% / Carapace 10% / Spitter 15% / Leviathan 5%。
- **预算花费**：按池内顺序循环购买，直到任何原型都买不起为止。
- **后代原型**：继承适应度较高一方的原型，并有 5% 概率跃迁为其他原型。
- **刷新锚点**："外围四个边缘裂隙"没有给坐标，取四条边中点，向内按切比雪夫环找最近的可通行非裂隙 Tile，平局按 `(y, x)` 升序打破。
- **波次预算**：`B_w = ⌊150·w^1.35⌋` 用大整数精确判定 `B^20 ≤ 150^20·w^27`，零浮点。

## E-027：信标场注入与调质广播

- **场注入**：半径 `R` 内的格钉向抛物目标 `s·(R²−d²)/R²`，仅当 `|target| ≥ |v|` 时覆盖。多个信标按实体序号依次施加。
- **调质广播**：单位收到的调质为半径内各广播源向量之和，逐分量钳制到 `[-1,1]`；求和与遍历顺序无关。
- **Leviathan 光环**：在半径 `8.0` 内把自身收到的调质再转发一跳，不递归。

## E-028：导航代价模型

- **地形代价**：`1000 / 移速系数`，即开阔 1000、玄武岩 909、沼泽 1176；裂隙取 4000，引导寻路绕行灼烧。墙体与建筑占格不可通行。
- **单步代价**：两格代价均值 × 方向系数（正交 1000‰ / 对角 1414‰），保证双向对称。对角移动禁止切角。
- **启发式**：Octile 距离 × 909，可采纳。
- **出队顺序**：按 `(f, h, tile_index)` 全序出队，保证路径选择与平台无关。

## E-029：演化算子参数

- **突变**：`mutation_rate` 解释为每个非零突触发生扰动的概率，缺省 0.05。高斯噪声用 Irwin–Hall 近似（12 个 16 位均匀数）。`σ = 0.08` 记为 Q8.8 的 `20`，突变后权重钳制到 `±8.0`。
- **拓扑突变**：以 `mutation_rate` 的概率新增一条突触（幅值 8..32）或剪除幅值最小的一条。
- **交叉**：只在同一原型之间进行。
- **随机源**：Philox 流 `EVOLUTION_RNG_STREAM`，密钥为 `(world_seed, tick, 实体序号)`，回放逐位一致。

## E-030：控制等级定义、拒绝原因 → 错误码映射与严格 Schema

- **五级控制**：规范只冻结了名称，各级定义如下：

  | 等级 | 名称 | 允许的工具 / 命令 |
  | --- | --- | --- |
  | 0 | `Spectate` | 仅观战 |
  | 1 | `Perceive` | `query_visible_clusters`、`sample_regional_fields` |
  | 2 | `Modulate` | `issue_strategic_intent`、`configure_signal_beacon`，以及对应的 `issue_order` / `configure_beacon` 命令 |
  | 3 | `Produce` | `queue_vat_production`、`trigger_crucible_breeding`，以及建造、研发、生产等其余命令 |
  | 4 | `Sovereign` | 全部命令，含 `demolish` 与 `surrender`；仅人类 Principal 可持有 |

- **租约**：人类申请租约时无条件抢占非人类持有者。Fencing Token 全局单调递增，旧 Token 一律以 `0x8001` 拒绝。
- **RTS 拒绝原因 → 错误码**：

  | 拒绝原因 | 错误码 |
  | --- | --- |
  | 实体不存在或类型不符 | `0x2002` |
  | 非本阵营实体、前置未满足、站点忙、对局已结束 | `0x8001` |
  | 队列已满 | `0x8004` |
  | 资源或人口不足 | `0x8005` |
  | 落点非法 | `0x8006` |
  | 参数越界 | `0x5001` |

- **ToolBroker 错误码**：
  - 未知工具、Schema 不符、未知协议、地图外坐标：`0x5001`。
  - 控制等级不足：`0x5006`。
  - 调用间隔或 APM 超限：`0x8004`。
  - 规划超时：`0x8003`，并丢弃计划、不产生副作用。
- **严格 Schema**：工具参数 Schema 按严格模式拒绝未声明的属性（Draft 2020-12 缺省允许），让 LLM 幻觉出的字段尽早暴露。
- **鉴权失败不记录**：鉴权失败的调用不写入 `AgentEpisodicMemory`，防止伪造 Principal 污染他人记忆。
- **浮点量化**：工具参数中的浮点数在边界处按"四舍五入、远离零"一次性量化为 milli 单位。

## E-031：经济结算与 Tick 内推进顺序

- **Tick 顺序**：
  1. 经济
  2. 施工
  3. 环境场（每 3 Tick）
  4. 神经（每 4 Tick）
  5. 运动
  6. 采集、战斗与环境伤害
  7. 死亡结算
  8. 迷雾（每 4 Tick）、PvE、胜负裁决
- **速率拆分**：每秒速率用累计取整差分拆到各 Tick，任意连续 30 Tick 之和精确等于标称值。
- **供电**：先结算产出，再按实体序号依次扣除耗电；余额不足的建筑本 Tick 掉电停工（Brownout）。施工中的建筑既不耗电也不产出。
- **反应堆**：须先扣到本 Tick 的 Biomass 份额才发电。
- **萃取器**：每 Tick 至多从覆盖格抽取 `0.4` 营养，按 1:1 计入生物质账本。
- **精炼厂**：以 60 Tick 为一周期，周期末余额足够时合成 Neurogel。
- **孵化槽**：入队时扣费，供电时倒计时，完成后在占地外圈最近的可通行格孵化。
- **施工**：无需工蜂，按 `build_ticks` 自动推进，生命值从 10% 线性增长到满值。
- **精英繁育**：规范未给出成本与耗时，取 `50 Flux + 50 Neurogel`、`600 Ticks`。
- **人口上限**：只来自已建成的孵化槽（每座 `+24`）。开局人口为 `5/0`（3 Forager + 1 Striker），须先建孵化槽才能生产。

## E-032：战斗细节

- **索敌**：取射程内、直线弹道不被墙体阻挡的最近敌方，按 `(距离², 序号)` 排序。
- **开火门控**：攻击读出 `< −0.5` 时不开火。
- **射速加成**：`field.signal_b` 正值按 `+20%·S_B/50` 提升射速，老兵再 `+20%`。
- **闪避**：闪避率为老兵 20% + 目标处于友方 Weaver 光环 15% − 攻击者处于 Weaver 光环 15%，钳制到 `[0, 90%]`，由 Philox 流 `0x0603` 判定。
- **特殊武器**：
  - Carapace：对 ±45° 扇形内的敌方造成伤害。
  - Spitter：命中格沉积 `15.0` 危害场。
  - 防御塔：每发消耗 `4 Flux`。
- **环境**：危害场每 Tick 造成 `0.08×F` 真实伤害。能量场 `≥ 20` 处护盾再生 `+25%`。
- **护盾再生**：每秒恢复最大护盾的 2% × (1 + 防御读出)。
- **残骸**：生物质造价的 75%（研发后 90%）化为死亡格营养。

## E-033：超时积分平局与突然死亡

- **积分**：只计已完工建筑与存活单位；`Cost` 取三资源之和，全部以 milli 计算。
- **突然死亡**：分差 `< 1%`（相对较高分）时进入 `3,600 Ticks` 加赛，期间主脑每秒流失 `20 HP`。
- **加赛裁决**：加赛中主脑被毁按歼灭判负；加赛结束再按积分裁决，仍相等则平局。

## E-034：观测裁剪

- **可见性**：己方全量可见。敌方单位只在所在 Tile 当前可见时出现。敌方建筑在任一占地格可见时出现，否则以静态 Ghost 快照出现。
- **隐藏信息**：敌方的指令与调质状态不下发。
- **区域场采样**：`sample_regional_fields` 只统计本阵营已探索的格，防止借环境场反推迷雾中的敌方信标。
- **聚合可见性**：观测中的可见性以 `4×4` Tile 聚合格下发，取值 0 = 未探索、1 = 可见、2 = 已探索。

## E-035：Arena 线上协议 `rynvek-arena/1`

- **消息格式**：规范未定义客户端 ↔ 权威服务端的对局消息，采用 JSON，以 `type` 为标签、snake_case 命名。
  - 客户端 → 服务端：`hello`、`acquire_lease`、`release_lease`、`command`、`tool_call`、`advance`。
  - 服务端 → 客户端：`welcome`、`lease`、`lease_revoked`、`ack`、`tool_result`、`observation`、`error`。
- **地形下发**：`welcome` 携带 4 bit 打包、Base64 编码的地形，以及 BLAKE3 `tiles_hash`，客户端解包后校验。`Vec2` 以 Q32.32 原始整数对下发。
- **人类接管**：人类申请租约即接管 Agent（旧 Token 立即失效并收到 `lease_revoked`）。人类交还（动作 27）后，被接管的 Agent 自动按原等级恢复租约。
- **命令通道**：人类、脚本与 Agent 走同一契约，每条 `command` 都要求租约与足够等级，并计入阵营 600 APM。
- **`advance`**：只用于本地练习、PvE 与测试，单次至多 `900 Ticks`。排位服务器自行计时。
- **Agent 租约**：时长 `39,600 Ticks`，覆盖整局加突然死亡。
- **建造请求**：规范 9.1 写明 `queue_vat_production` "或发起建筑建造请求"，但 Schema 没有坐标字段。约定当 `unit_archetype` 为 `bld.*` 时，`vat_entity_id` 解释为占地左下角 `"x,y"`，`count` 必须为 1，`genome_slug` 忽略。
- **参考实现**：`crates/rynvek-engine/examples/arena_server.rs`，stdio JSON Lines 服务端。

## E-036：`@rynvek/client` / `@rynvek/arena` 前端约定与闭环夹具

- **建造预览**：在客户端复现服务端的四条落点规则。客户端只有 `4×4` 聚合可见性，所以预览可能比权威层乐观、不会更严，最终以 `0x8006` 为准。
- **建筑朝向**：规范中的建筑均为正方形占地，朝向只影响外观。
- **确定性选址**：按切比雪夫环由内向外、环内 `(y, x)` 升序扫描。
- **右键情境指令**（动作 19）：
  1. 1 Tile 内有可见敌方 → `AttackMove`；
  2. 选中的全是 Forager 且目标为沼泽 → `Harvest`；
  3. 其余 → `Move`。
- **输入**：文本框内不触发不带修饰键的单字符快捷键。
- **事件过滤**：只下发本阵营事件与全局事件（虫潮、突然死亡、终局）。
- **小地图**：阵营色取 Okabe-Ito 第 `i+1` 色，野生虫潮取第 7 色。
- **闭环测试**：`tests/arena/opening.transcript.json` 记录 3 个席位（人类 p0、Agent p0、Agent p1）共 26 步、截至 Tick 2431 的完整收发。
  - `@rynvek/arena` 用夹具驱动前端，断言发出的每条消息与夹具一致；设置 `RYNVEK_ARENA_SERVER` 时改连真实 `arena_server` 进程。
  - `rynvek-engine/tests/arena_loop.rs` 用同一夹具回放权威会话，逐步比对输出。JS 会把 `1.0` 序列化为 `1`，比较前先对 JSON 数值做语义归一。
  - 夹具用 `RYNVEK_ARENA_RECORD=1` 重新录制。
- **已知玩法问题（不改规范数值）**：
  - 采集工蜂的神经脑会在 2.5 Tile 采集半径内游走，可能踏入相邻裂隙被灼烧致死。闭环场景因此选取 4 Tile 内无裂隙的沼泽作为采集目标。
  - 是否让采集工蜂的运动读出在 Harvest 指令下回避裂隙，留待平衡迭代裁决。
- **许可证**：`@rynvek/client` 与 `@rynvek/arena` 为 `AGPL-3.0-only`；`protocols/schemas/agent-tools.v1.json` 随协议层采用 Apache-2.0 WITH LLVM-exception。
