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
