# Rynvek 开发规范文档：零歧义工程架构、ABI 协议与全栈代码实现蓝图（v1.0）

> **文档性质**：Normative Engineering & Implementation Specification（直接供 AI Coding Agent 与工程师按图索骥编写代码，零待决设计项）
> **配套阅读**：关于项目的起源背景、66 轮演进前因后果、两轮 28 项深度审查结论、游戏设计圣经、平衡数值与 732+ 项 ADR 决策理由，请参见姊妹篇 **《Rynvek 项目文档（`Rynvek_Project_Documentation_v1.0.md`）》**。
> **强制实现准则**：
> 1. 所有模块命名、目录路径、配置文件、结构体字段、二进制字节偏移、SQL 表结构、API 路由、CSS 变量与错误码必须 **100% 严格按本文档实现**，严禁自行发明替代命名或遗留 `neuroverse` / `nv` / `.nview` / `rynvek-runtime` 等旧称。
> 2. 所有核心代码文件头部必须包含 `// SPDX-FileCopyrightText: 2026 The Rynvek Contributors` 与对应许可证标识（核心引擎与应用使用 `AGPL-3.0-only`；公开协议与客户端 SDK 使用 `Apache-2.0 WITH LLVM-exception`）。

## 规范 01：全局工程基线、Monorepo 物理拓扑与根配置文件源码

### 1.1 全局冻结工具链与工程常量表

| 类别 | 规范键名 | 锁定值 | 强制工程要求 |
| :--- | :--- | :--- | :--- |
| **Rust 工具链** | `edition` / `rust-version` | `2024` / `1.85.0` | Cargo Workspace `resolver = "3"`；开启 `#![deny(unsafe_code)]`（仅 ABI 桥接模块受控豁免） |
| **GPU 与着色器** | `wgpu` / `shader` | `30.0.1` / `WGSL` | 线性 Workgroup `128×1×1`，二维网格 Workgroup `8×8×1`，GPU Buffer 分段大小 `32 MiB` |
| **TypeScript 工具链** | `node` / `pnpm` / `ts` | `22.14 LTS` / `10.x` / `5.8.x` | 开启 `strict: true`, `noUncheckedIndexedAccess: true`, `exactOptionalPropertyTypes: true` |
| **沙箱与插件虚拟机** | `quickjs` / `wasmtime` | `QuickJS-NG (WASM)` / `29.0` | Tier-1 规则单 Tick 预算 `500,000 Gas`，堆上限 `64 MiB`；WIT 协议 `rynvek:plugin@1.0.0` |
| **Python 科研环境** | `python` / `pyo3` / `arrow` | `>= 3.12` / `0.23+` / `Arrow 53+` | 通过 `Maturin` 构建，零拷贝导出 `pyarrow.RecordBatch` 与 Parquet 谱系文件 |
| **时钟与空间常数** | `base_tick_hz` / `chunk_size` | `30 Hz` / `256 × 256 Units` | 神经子步 `120 Hz` (4 substeps)，环境场 `10 Hz` (每 3 Ticks)，耦合周期 `4 Ticks` |
| **密码学与压缩** | `hash` / `sign` / `compress` | `BLAKE3-256` / `Ed25519` / `Zstd & LZ4` | 快照/包归档用 `Zstd (level 3)`，实时网络流用 `LZ4 Frame` |

### 1.2 Monorepo 完整物理目录树与 24 个 Rust Crates 单向依赖 DAG

```text
rynvek/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── deny.toml
├── REUSE.toml
├── pnpm-workspace.yaml
├── package.json
├── turbo.json
├── tsconfig.base.json
├── LICENSES/
│   ├── AGPL-3.0-only.txt
│   ├── Apache-2.0.txt
│   ├── LLVM-exception.txt
│   ├── CC-BY-4.0.txt
│   ├── CC0-1.0.txt
│   └── OFL-1.1.txt
├── protocols/
│   ├── wit/rynvek-plugin.wit
│   ├── proto/rynvek_wire.proto
│   └── schemas/
│       ├── rworld.schema.json
│       ├── rpack.schema.json
│       ├── rview.schema.json
│       └── audio-manifest.schema.json
├── crates/
│   ├── rynvek-core/          # [Tier 0] 定点数 Q32_32/Q8_8, 坐标, 量纲, Philox RNG, BLAKE3, 错误码
│   ├── rynvek-schema/        # [Tier 0] SchemaRegistry, Canonical JSON/CBOR, MigrationGraph
│   ├── rynvek-protocol/      # [Tier 0] Command/Observation/Event 信封, WireFrameHeader, CapabilityToken
│   ├── rynvek-memory/        # [Tier 1] MemoryManager, 32MiB Slab, 64B/256B 对齐池, 四级预算状态机
│   ├── rynvek-ecs/           # [Tier 1] EntityId/EntityHandle/GpuEntityRef, Paged SoA, Tombstone
│   ├── rynvek-storage/       # [Tier 1] StorageEngine Trait, SQLite/Postgres/OPFS 适配器, CAS BlobStore
│   ├── rynvek-journal/       # [Tier 1] .rjournal (RYJL) WAL 读写器, CommitManifest, 崩溃恢复机
│   ├── rynvek-snapshot/      # [Tier 1] .rsnap (RYSS) 分块序列化, NEUR 专用稀疏块, Zstd 压缩
│   ├── rynvek-compute/       # [Tier 2] ComputeBackend Trait, CapabilityProfile (Tier A-D), KernelRegistry
│   ├── rynvek-compute-cpu/   # [Tier 2] CPU Reference 确定性执行器 (Rayon + SIMD)
│   ├── rynvek-wgpu/          # [Tier 2] wgpu 30.0.1 Device Owner, SegmentedBuffer, BindGroup 0-3, DeviceLost 恢复
│   ├── rynvek-field/         # [Tier 2] 260x260 Halo 场分块, 反应扩散 PDE, Lenia 卷积, CFL 自动子步
│   ├── rynvek-spatial/       # [Tier 2] UniformGrid 4-Pass 基数排序 CSR, DynamicBVH, 定点辛欧拉与碰撞
│   ├── rynvek-neural/        # [Tier 2] Genome/BrainTopology/BrainInstance, 4 种神经元模型, 3 种图存储, STDP
│   ├── rynvek-rules/         # [Tier 2] 36 条 Typed SSA Rule IR, 静态分析, WGSL 生成器, QuickJS & Wasmtime 宿主
│   ├── rynvek-simulation/    # [Tier 3] 多速率时钟, 16 阶段 Tick DAG, ConflictPolicy 归约, InvariantRegistry
│   ├── rynvek-world/         # [Tier 3] World 生命周期, Partition/Halo 协调, Fork/Patch/Merge, .rworld 加载
│   ├── rynvek-agent/         # [Tier 3] Principal/Controller, ControlLease, 600 APM 限流, LLM ToolBroker
│   ├── rynvek-rts/           # [Tier 3] 3 资源, 8 建筑, 6 神经单位, 9 科技, 对称地图生成, HPA*+流场+ORCA, 迷雾
│   ├── rynvek-render/        # [Tier 3] VisualizationProvider, L0-L7 渲染层, GPU Instancing, GPU ID Picking
│   ├── rynvek-audio/         # [Tier 3] Commit-Aware AudioEvent 生成, Kuramoto 神经声音化聚合器
│   ├── rynvek-security/      # [Tier 3] RBAC+ABAC+Capability 鉴权, Ed25519 验签, 反作弊校验器
│   ├── rynvek-engine/        # [Tier 4] Rynvek Engine 顶层门面 (整合 World + Simulation + Render + Audio)
│   ├── rynvek-network/       # [Tier 4] WebTransport/WebSocket 双栈, AOI 裁剪, 分层 Delta 复制
│   ├── rynvek-wasm/          # [Tier 4] 浏览器 Dedicated Engine Worker 入口与 OffscreenCanvas 绑定
│   ├── rynvek-server/        # [Tier 4] 权威世界宿主, Matchmaking, Glicko-2, Hub Registry, HTTP/RPC 服务
│   ├── rynvek-cli/           # [Tier 4] `rynvek` 命令行工具
│   └── xtask/                # [Tool] ABI 代码生成, Golden Vector 验证, SBOM 与许可证门禁
├── packages/
│   ├── core/                 # `@rynvek/core`
│   ├── protocol/             # `@rynvek/protocol`
│   ├── engine/               # `@rynvek/engine`
│   ├── rules/                # `@rynvek/rules`
│   ├── agent-sdk/            # `@rynvek/agent-sdk`
│   ├── ui/                   # `@rynvek/ui`
│   ├── visualization/        # `@rynvek/visualization`
│   ├── audio/                # `@rynvek/audio`
│   ├── assets/               # `@rynvek/assets`
│   ├── client/               # `@rynvek/client`
│   ├── studio/               # `@rynvek/studio`
│   ├── arena/                # `@rynvek/arena`
│   ├── lab/                  # `@rynvek/lab`
│   └── wasm-bindings/        # `@rynvek/wasm`
├── apps/
│   ├── web-platform/         # Vite + React 19 统一 Web 入口 (`/arena`, `/studio`, `/lab`, `/hub`)
│   └── desktop-shell/        # Tauri 2.x 桌面端外壳
├── python/
│   ├── pyproject.toml
│   └── rynvek/
└── tests/
    └── golden_vectors/v1_baseline.json
```

**Crate 单向依赖铁律（严禁逆向或循环依赖）**：
- `Tier 0` (`rynvek-core`, `rynvek-schema`, `rynvek-protocol`) 不依赖任何内部业务 Crate。
- `Tier 1` (`rynvek-memory`, `rynvek-ecs`, `rynvek-storage`, `rynvek-journal`, `rynvek-snapshot`) 仅可依赖 `Tier 0`。
- `Tier 2` (`rynvek-compute`, `rynvek-compute-cpu`, `rynvek-wgpu`, `rynvek-field`, `rynvek-spatial`, `rynvek-neural`, `rynvek-rules`) 仅可依赖 `Tier 0` 与 `Tier 1`。
- `Tier 3` (`rynvek-simulation`, `rynvek-world`, `rynvek-agent`, `rynvek-rts`, `rynvek-render`, `rynvek-audio`, `rynvek-security`) 仅可依赖 `Tier 0..=2`。
- `Tier 4` (`rynvek-engine`, `rynvek-network`, `rynvek-wasm`, `rynvek-server`, `rynvek-cli`) 组合 `Tier 0..=3` 对外提供最终交付产物。

### 1.3 根配置文件完整源码（直接写入仓库根目录）

#### 1.3.1 `rust-toolchain.toml` 与 `pnpm-workspace.yaml`

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy", "rust-src"]
targets = ["wasm32-unknown-unknown", "wasm32-wasip2"]
profile = "minimal"
```

```yaml
# pnpm-workspace.yaml
packages:
  - "packages/*"
  - "apps/*"
```

#### 1.3.2 `deny.toml`（许可证与供应链强制门禁配置）

```toml
# deny.toml
# SPDX-License-Identifier: AGPL-3.0-only

[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
allow = [
    "AGPL-3.0-only",
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "MPL-2.0",
    "CC0-1.0",
]
confidence-threshold = 0.95

[bans]
multiple-versions = "warn"
wildcards = "deny"
```

## 规范 02：基础类型、定点算术（`Q32_32` / `Q8_8`）、统一坐标与确定性随机数（`rynvek-core`）

### 2.1 定点数算术（`Q32_32` 与 `Q8_8`）及 `WorldPosition2D` 完整实现

```rust
// crates/rynvek-core/src/fixed.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

#![deny(clippy::float_arithmetic)]

/// 64-bit 有符号定点数 Q32.32（整数 32 位，小数 32 位，用于 D0 权威位置、速度、物理与经济比例）
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Q32_32(pub i64);

impl Q32_32 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1i64 << 32);
    pub const NEG_ONE: Self = Self(-(1i64 << 32));
    pub const HALF: Self = Self(1i64 << 31);
    pub const DT_30HZ: Self = Self(143_165_576); // floor(2^32 / 30)

    #[inline(always)]
    pub const fn from_int(v: i32) -> Self {
        Self((v as i64) << 32)
    }

    #[inline(always)]
    pub const fn from_milli(milli: i64) -> Self {
        Self((milli << 32) / 1000)
    }

    #[inline(always)]
    pub const fn to_int_floor(self) -> i32 {
        (self.0 >> 32) as i32
    }

    #[inline(always)]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    #[inline(always)]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    #[inline(always)]
    pub const fn mul(self, rhs: Self) -> Self {
        let prod = (self.0 as i128) * (rhs.0 as i128);
        Self((prod >> 32) as i64)
    }

    #[inline(always)]
    pub const fn div_safe(self, rhs: Self, fallback: Self) -> Self {
        if rhs.0 == 0 {
            return fallback;
        }
        let num = (self.0 as i128) << 32;
        Self((num / (rhs.0 as i128)) as i64)
    }

    /// 确定性整数平方根（Newton-Raphson 位级收敛，跨一切 CPU 架构 100% 位级一致，零浮点指令）
    #[inline]
    pub const fn sqrt_deterministic(self) -> Self {
        if self.0 <= 0 {
            return Self::ZERO;
        }
        let val = (self.0 as u128) << 32;
        let mut x = 1u128 << ((128 - val.leading_zeros() + 1) >> 1);
        let mut i = 0;
        while i < 8 {
            x = (x + val / x) >> 1;
            i += 1;
        }
        let res = if x * x > val { x - 1 } else { x };
        Self(res as i64)
    }
}

/// 16-bit 有符号定点数 Q8.8（整数 8 位 [-128, 127]，小数 8 位精度 1/256，用于神经网络膜电位、突触权重与环境场）
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Q8_8(pub i16);

impl Q8_8 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(256);
    pub const NEG_ONE: Self = Self(-256);

    #[inline(always)]
    pub const fn mul(self, rhs: Self) -> Self {
        let prod = (self.0 as i32) * (rhs.0 as i32);
        Self((prod >> 8).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
    }

    #[inline(always)]
    pub const fn hard_tanh(self) -> Self {
        Self(self.0.clamp(-256, 256))
    }
}
```

### 2.2 无状态计数器式确定性随机数 `Philox4x32-10`（Rust 与 WGSL 双端位级对齐）

Rust 端实现见卷 03（`crates/rynvek-core/src/rng.rs`）。WGSL 端必须使用以下完全相同的 10 轮 32 位无符号乘法展开函数：

```wgsl
// crates/rynvek-wgpu/shaders/common/philox_rng.wgsl
// SPDX-License-Identifier: AGPL-3.0-only

fn mul_hi_lo_u32(a: u32, b: u32) -> vec2<u32> {
    let a_lo = a & 0xFFFFu;
    let a_hi = a >> 16u;
    let b_lo = b & 0xFFFFu;
    let b_hi = b >> 16u;

    let p0 = a_lo * b_lo;
    let p1 = a_lo * b_hi;
    let p2 = a_hi * b_lo;
    let p3 = a_hi * b_hi;

    let mid = (p0 >> 16u) + (p1 & 0xFFFFu) + (p2 & 0xFFFFu);
    let lo = (p0 & 0xFFFFu) | ((mid & 0xFFFFu) << 16u);
    let hi = p3 + (p1 >> 16u) + (p2 >> 16u) + (mid >> 16u);
    return vec2<u32>(lo, hi); // .x = lo, .y = hi
}

fn philox4x32_10(ctr_in: vec4<u32>, key_in: vec2<u32>) -> vec4<u32> {
    var ctr = ctr_in;
    var key = key_in;
    for (var r: u32 = 0u; r < 10u; r = r + 1u) {
        let p0 = mul_hi_lo_u32(0xD2511F53u, ctr.x);
        let p1 = mul_hi_lo_u32(0xCD9E8D57u, ctr.z);
        ctr = vec4<u32>(
            p1.y ^ ctr.y ^ key.x,
            p1.x,
            p0.y ^ ctr.w ^ key.y,
            p0.x
        );
        key.x = key.x + 0x9E3779B9u;
        key.y = key.y + 0xBB67AE85u;
    }
    return ctr;
}
```

## 规范 03：16 阶段标准 Tick 执行引擎、多速率时钟与冲突仲裁（`rynvek-simulation`）

### 3.1 16 阶段拓扑信封（`CanonicalPhaseId`）与阶段描述符实现

```rust
// crates/rynvek-simulation/src/phase.rs
// SPDX-License-Identifier: AGPL-3.0-only

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanonicalPhaseId {
    ClockAndEpochAdvance = 0,       // [Kernel Immutable]
    CommandIngressAndValidate = 1,  // [Kernel Immutable]
    ScheduledEventDelivery = 2,     // [Kernel Immutable]
    SpatialAndVisibilityIndex = 3,  // [Domain Spatial]
    EnvironmentAndFieldSolve = 4,   // [Domain Field - 可与 Phase 05/06 并发]
    SensoryAndPerceptionGather = 5, // [Domain Neural]
    NeuralDynamicsAndPlasticity = 6,// [Domain Neural]
    ControllerAndRuleEval = 7,      // [Domain Rules]
    ActionArbitration = 8,          // [Domain Arbitration]
    KinematicsAndCollision = 9,     // [Domain Physics]
    CombatAndEconomySettle = 10,    // [Domain RTS/Interaction]
    MetabolismAndLifecycle = 11,    // [Domain Bio]
    EvolutionAndMutation = 12,      // [Domain Bio - Epoch Gated]
    StructuralMutationApply = 13,   // [Kernel Immutable]
    InvariantAndNumericCheck = 14,  // [Kernel Immutable]
    CommitAndPublication = 15,      // [Kernel Immutable]
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct TickCommitManifest {
    pub world_id: [u8; 16],
    pub tick: u64,
    pub world_revision: u64,
    pub parent_commit_hash: [u8; 32],
    pub entity_root_hash: [u8; 32],
    pub field_root_hash: [u8; 32],
    pub neural_root_hash: [u8; 32],
    pub economy_root_hash: [u8; 32],
    pub rng_root_hash: [u8; 32],
    /// 由以上 6 个 32B 子根拼接后计算的最终 32B BLAKE3 状态根
    pub state_root_hash: [u8; 32],
}
```

### 3.2 仿真不变量检查器（`InvariantRegistry` — `Phase 14` 硬实时校验）

在每个 Tick 的 `Phase 14` 必须执行以下 5 项零容忍不变量检查，任一失败立即丢弃当帧 `CandidateState[t]`，将告警记入诊断日志并执行 `ErrorRecoveryAction::RollbackTick`：
1. **`INV-01 (NoNaNOrInf)`**：检查 GPU Diagnostics 原子计数器 `s_diagnostics[0]`（记录任何 WGSL Kernel 捕获到的非有限数或定点饱和溢出异常）必须为 `0`。
2. **`INV-02 (ResourceNonNegative)`**：所有阵营的 `Biomass`, `Flux`, `Neurogel` 账本余额必须 $\ge 0$ 且 $\le \text{Capacity}_{\max}$。
3. **`INV-03 (MassEnergyConservation)`**：在封闭世界配置下，本帧采集增加的 `Biomass` 必须严格等于环境场扣除的 `Nutrient` 与残骸回收量之和（误差 $= 0\text{ milli-unit}$）。
4. **`INV-04 (HandleReferentialIntegrity)`**：所有活跃关系边（`Relation`）、控制器绑定（`ControllerBinding`）与攻击目标引用的 `EntityHandle` 必须指向 `EntityLifecycleState::Alive` 的有效槽位。
5. **`INV-05 (CoordinateCanonicalBounds)`**：所有实体的 `local_x` 与 `local_y` 必须满足 $0 \le \text{local} < 256 \times 2^{32}$。

## 规范 04：三层实体标识、分页列式存储（Paged SoA）与内存管理器（`rynvek-ecs` & `rynvek-memory`）

### 4.1 实体注册表（`EntityRegistry`）与 `DomainPage` 内存布局

```rust
// crates/rynvek-ecs/src/registry.rs
// SPDX-License-Identifier: AGPL-3.0-only

use crate::identity::{EntityHandle, EntityId, EntityLifecycleState, EntityTombstone, GpuEntityRef};

pub const PAGE_CAPACITY: usize = 65_536;
pub const CHUNK_CAPACITY: usize = 4_096;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SparsePageEntry {
    pub stable_id: EntityId,
    pub dense_page_id: u16,
    pub dense_slot_in_page: u16,
    pub generation: u32,
    pub state: EntityLifecycleState,
    pub archetype_id: u16,
}

pub struct EntityRegistry {
    pub world_lineage_prefix: u64,
    pub next_monotonic_serial: u64,
    /// 按 65,536 槽位分页的稀疏代数表（支持 O(1) 验证 EntityHandle 并定位紧凑列坐标）
    pub sparse_pages: Vec<Box<[SparsePageEntry; PAGE_CAPACITY]>>,
    /// 紧凑下标反向映射至 handle.slot（供 Swap-Remove 时 O(1) 修复反向指针）
    pub dense_to_sparse_slot: Vec<u32>,
    /// 延迟复用空闲队列（FIFO 队列，避免刚销毁槽位在同一 Epoch 内立即复用）
    pub free_slots_fifo: std::collections::VecDeque<u32>,
    pub tombstones: Vec<EntityTombstone>,
}
```

## 规范 05：神经网络原生运行时与人工生命演化实现规范（`rynvek-neural`）

### 5.1 三层神经对象模型、三种图存储布局与四因子 STDP 完整实现

```rust
// crates/rynvek-neural/src/runtime.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_core::fixed::Q8_8;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum NeuronModelKind {
    GatedMLP = 0,
    LIF = 1,
    Izhikevich = 2,
    CTRNN = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum TopologyStorageClass {
    /// 单脑总神经元 N <= 64：采用 16x16 分块稠密权重矩阵
    DenseTiledMatrix = 0,
    /// 64 < N <= 4096 且仅在 Epoch 边界改变拓扑：采用 8x8 块稀疏压缩行
    BlockSparseCSR = 1,
    /// 支持运行期高频突触增删：每神经元固定预分配 32 个突触槽位页
    PagedDynamicAdjacency = 2,
}

/// 确定性 Q8.8 LIF 神经元与四因子 STDP 更新核（CPU Reference 与 D0/D1 权威归约标准实现）
pub fn step_lif_brain_deterministic(
    sensory_in_q8: &[Q8_8; 16],
    membrane_v_q8: &mut [Q8_8],
    refractory_ticks: &mut [u8],
    spike_ring: &mut [u64],
    weights_base_q8: &[i16],
    weights_delta_q8: &mut [i16],
    eligibility_q8: &mut [i16],
    modulators_q8: [i16; 4], // [DA, 5-HT, NE, ACh] in [-256, +256]
    plasticity_enabled: bool,
) -> [Q8_8; 6] {
    let n_hid = membrane_v_q8.len();
    let m_da = modulators_q8[0] as i32;
    let m_ne = modulators_q8[2] as i32;
    // 去甲肾上腺素动态调节阈值：theta_eff = 256 * (1 - 0.30 * m_NE / 256)
    let theta_eff: i32 = 256 - ((77 * m_ne) >> 8);
    let leak_beta: i32 = 230; // ~0.898 in Q8.8

    let mut out_acc = [0i32; 6];

    for h in 0..n_hid {
        if refractory_ticks[h] > 0 {
            refractory_ticks[h] -= 1;
            membrane_v_q8[h] = Q8_8::ZERO;
            spike_ring[h] <<= 1;
            continue;
        }

        let mut syn_current: i32 = 0;
        let row_off = h * 16;
        for i in 0..16 {
            let w_eff = (weights_base_q8[row_off + i] as i32 + weights_delta_q8[row_off + i] as i32)
                .clamp(-32767, 32767);
            syn_current += ((sensory_in_q8[i].0 as i32) * w_eff) >> 8;
        }

        let v_prev = membrane_v_q8[h].0 as i32;
        let v_next = ((v_prev * leak_beta) >> 8) + syn_current;
        let spiked = v_next >= theta_eff;

        spike_ring[h] = (spike_ring[h] << 1) | (spiked as u64);

        if spiked {
            membrane_v_q8[h] = Q8_8::ZERO;
            refractory_ticks[h] = 2;
            let out_off = n_hid * 16 + h * 6;
            for k in 0..6 {
                let w_out = (weights_base_q8[out_off + k] as i32 + weights_delta_q8[out_off + k] as i32)
                    .clamp(-32767, 32767);
                out_acc[k] += w_out;
            }
        } else {
            membrane_v_q8[h] = Q8_8(v_next.clamp(-512, 512) as i16);
        }

        // 四因子调制可塑性更新（遵守 Dale 定律：兴奋/抑制符号锁死不可翻转）
        if plasticity_enabled {
            for i in 0..16 {
                let idx = row_off + i;
                let e_prev = eligibility_q8[idx] as i32;
                let pre_act = (sensory_in_q8[i].0 as i32).abs();
                let stdp_term = if spiked { (pre_act * 15) >> 8 } else { -((pre_act * 4) >> 8) };
                let e_next = (((e_prev * 243) >> 8) + stdp_term).clamp(-512, 512);
                eligibility_q8[idx] = e_next as i16;

                let dw = ((m_da * e_next) >> 11).clamp(-8, 8);
                let base_w = weights_base_q8[idx] as i32;
                let new_delta = (weights_delta_q8[idx] as i32 + dw).clamp(-128, 128);
                // Dale's Law 符号保护
                if base_w >= 0 && (base_w + new_delta) < 0 {
                    weights_delta_q8[idx] = (-base_w) as i16;
                } else if base_w < 0 && (base_w + new_delta) > 0 {
                    weights_delta_q8[idx] = (-base_w) as i16;
                } else {
                    weights_delta_q8[idx] = new_delta as i16;
                }
            }
        }
    }

    let mut readout = [Q8_8::ZERO; 6];
    for k in 0..6 {
        readout[k] = Q8_8((out_acc[k] >> 2).clamp(-256, 256) as i16);
    }
    readout
}
```

## 规范 06：连续环境场、CFL 稳定求解器、空间索引与定点物理引擎（`rynvek-field` & `rynvek-spatial`）

### 6.1 `260×260` 带 Halo 二维场分块与五点拉普拉斯定点扩散算子

```rust
// crates/rynvek-field/src/solver.rs
// SPDX-License-Identifier: AGPL-3.0-only

pub const TILE_DIM: usize = 256;
pub const HALO: usize = 2;
pub const STRIDE: usize = TILE_DIM + 2 * HALO; // 260

/// 定点 Q8.8 五点拉普拉斯反应-扩散算子（跨 CPU/GPU 100% 位级确定性）
pub fn solve_reaction_diffusion_tile_q8(
    src_padded: &[i16; STRIDE * STRIDE],
    dst_padded: &mut [i16; STRIDE * STRIDE],
    sources_q8: &[i16; TILE_DIM * TILE_DIM],
    diffusion_q8: i32, // D_c in Q8.8 (必须 <= 64, 即 D <= 0.25 满足二维显式 CFL 条件)
    decay_q8: i32,     // lambda_c in Q8.8
    min_val_q8: i16,
    max_val_q8: i16,
) {
    assert!(diffusion_q8 <= 64, "CFL Stability Violation: diffusion_q8 must be <= 64 (0.25)");
    let retain_q8 = 256 - decay_q8;

    for y in 0..TILE_DIM {
        let py = y + HALO;
        for x in 0..TILE_DIM {
            let px = x + HALO;
            let idx = py * STRIDE + px;

            let center = src_padded[idx] as i32;
            let north = src_padded[(py - 1) * STRIDE + px] as i32;
            let south = src_padded[(py + 1) * STRIDE + px] as i32;
            let west = src_padded[py * STRIDE + (px - 1)] as i32;
            let east = src_padded[py * STRIDE + (px + 1)] as i32;

            let laplacian = north + south + west + east - 4 * center;
            let decayed = (center * retain_q8) >> 8;
            let diffused = (laplacian * diffusion_q8) >> 8;
            let src_term = sources_q8[y * TILE_DIM + x] as i32;

            let next_val = (decayed + diffused + src_term).clamp(min_val_q8 as i32, max_val_q8 as i32);
            dst_padded[idx] = next_val as i16;
        }
    }
}
```

### 6.2 空间索引 CSR 邻居表构建规范（`UniformGridCsr`）

```rust
// crates/rynvek-spatial/src/csr_grid.rs
// SPDX-License-Identifier: AGPL-3.0-only

pub const GRID_CELLS_PER_AXIS: usize = 128; // 每格 2.0 x 2.0 World Units，覆盖 256x256 Chunk
pub const TOTAL_GRID_CELLS: usize = GRID_CELLS_PER_AXIS * GRID_CELLS_PER_AXIS; // 16,384
pub const MAX_NEIGHBORS_PER_ENTITY: usize = 32;

pub struct SpatialCsrIndex {
    /// 长度固定为 16,385：cell `c` 内的实体位于 sorted_dense_indices[row_offsets[c]..row_offsets[c+1]]
    pub row_offsets: Vec<u32>,
    /// 按 (cell_morton_key, dense_index) 稳定升序排列的实体紧凑索引
    pub sorted_dense_indices: Vec<u32>,
}
```

## 规范 07：WebGPU 设备独占所有权、分段缓冲与 WGSL 着色器库（`rynvek-wgpu` & `rynvek-render`）

### 7.1 Dedicated Engine Worker 独占 `wgpu::Device` 初始化与 `DeviceLost` 恢复契约

1. **初始化序列**：
   - 前端 React 主线程挂载 `<canvas id="rv-world-canvas" />`，立即调用 `const offscreen = canvas.transferControlToOffscreen()`，通过 `EngineWorker.postMessage({ type: "ENGINE_INIT", canvas: offscreen, ... }, [offscreen])` 将画布所有权永久移交至 Worker。
   - `rynvek-wasm` 在 Worker 内部创建唯一的 `wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::PRIMARY, .. })`，请求 `HighPerformance` Adapter，并根据 `adapter.limits()` 自动判定 `ComputeCapabilityProfile`（`Tier A` / `Tier B` / `Tier C` / `Tier D`）。
2. **`32 MiB` 分段缓冲池（`SegmentedBufferPool`）**：
   - 严禁分配超过 `128 MiB` 的单体 Storage Buffer。所有 `GpuOrganismHotState` 与神经突触表按每个 Segment `33,554,432 Bytes (32 MiB)` 切分，配合 `3-Slot Readback Ring`（`Slot[tick % 3]`）使用 `buffer.map_async(wgpu::MapMode::Read)` 异步回读統計摘要，**主循环内严禁同步阻塞等待 GPU Fence**。

## 规范 08：TypeScript→WGSL 规则编译器、36 条 SSA 指令集与沙箱（`rynvek-rules` & `@rynvek/rules`）

### 8.1 规则编译器四阶段管线（Four-Stage Compiler Pipeline）

```text
[@rynvek/rules Kernel.define(TS)]
       │ 1. AST / Expression Builder Execution
       ▼
[Untyped Rule Graph JSON]
       │ 2. Pass 1: Type Inference & Fixed-Point Promotion (Q32_32 / Q8_8)
       │ 3. Pass 2: Read/Write Access Set Extraction & ConflictPolicy Check
       │ 4. Pass 3: Loop Bound Verification (max_iters <= 64) & Gas Cost Estimation
       ▼
[Validated Typed SSA Rule IR (.rir Binary, Magic "RYIR", 36 Opcodes)]
       ├──► Backend A: `emit_wgsl_compute_shader(&RuleModule) -> String` (GPU Tier-0)
       └──► Backend B: `emit_cpu_simd_bytecode(&RuleModule) -> AlignedBytecode` (CPU Reference)
```

**静态安全检查硬约束**：
- 若规则在 `writes` 中未声明某个 Component 字段却在函数体内调用 `emitComponentWrite`，编译期直接报 `RynvekErrorCode::RuleUndeclaredWriteAccess (0x5002)` 拒绝构建。
- 若存在 `BoundedForBegin` 的 `max_iters > 64` 或嵌套循环总展开次数 $> 256$，直接报 `RynvekErrorCode::RuleLoopBoundExceeded (0x5003)` 拒绝构建。

## 规范 09：五级控制协议、LLM `ToolBroker`、RTS 三级确定性寻路与服务端战争迷雾（`rynvek-agent` & `rynvek-rts`）

### 9.1 LLM Agent 受控工具调用代理（`ToolBroker` — 补全 GAP-26, D167, D486）

LLM Agent 严禁直接拼接底层二进制命令或访问未授权内存，必须通过 `rynvek-agent::ToolBroker` 暴露的以下 **6 个标准受控工具函数（JSON Schema Draft 2020-12）** 进行战略感知与决策（单次调用超时 `2,000 ms`，最小调用间隔 `30 Ticks = 1.0 s`）：

```json
[
  {
    "name": "query_visible_clusters",
    "description": "查询当前阵营可见视野内的友方与敌方单位集群摘要及主脑状态",
    "parameters": {
      "type": "object",
      "properties": {
        "region_chunk": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 },
        "include_enemy": { "type": "boolean" }
      },
      "required": ["include_enemy"]
    }
  },
  {
    "name": "sample_regional_fields",
    "description": "采样指定 Chunk 区域的 5 通道环境场均值与峰值坐标 (Nutrient, Energy, SignalA, SignalB, Hazard)",
    "parameters": {
      "type": "object",
      "properties": {
        "chunk_x": { "type": "integer" },
        "chunk_y": { "type": "integer" }
      },
      "required": ["chunk_x", "chunk_y"]
    }
  },
  {
    "name": "issue_strategic_intent",
    "description": "向指定单位编队或区域下发高层战略意图 (移动集结、进攻推进、采集或防守主脑)",
    "parameters": {
      "type": "object",
      "properties": {
        "unit_archetype": { "type": "string", "enum": ["all", "unit.forager", "unit.striker", "unit.carapace", "unit.spitter", "unit.weaver", "unit.leviathan"] },
        "order_kind": { "type": "string", "enum": ["Move", "AttackMove", "Harvest", "HoldPosition", "Patrol"] },
        "target_world_x": { "type": "number" },
        "target_world_y": { "type": "number" }
      },
      "required": ["unit_archetype", "order_kind", "target_world_x", "target_world_y"]
    }
  },
  {
    "name": "configure_signal_beacon",
    "description": "调节指定神经调制信标的信息素场注入强度与四维神经调质广播向量 [DA, 5-HT, NE, ACh]",
    "parameters": {
      "type": "object",
      "properties": {
        "beacon_entity_id": { "type": "string" },
        "signal_channel": { "type": "string", "enum": ["field.signal_a", "field.signal_b"] },
        "signal_strength": { "type": "number", "minimum": -50.0, "maximum": 50.0 },
        "neuromodulators": {
          "type": "array",
          "items": { "type": "number", "minimum": -1.0, "maximum": 1.0 },
          "minItems": 4,
          "maxItems": 4
        }
      },
      "required": ["beacon_entity_id", "signal_channel", "signal_strength", "neuromodulators"]
    }
  },
  {
    "name": "queue_vat_production",
    "description": "在指定神经孵化槽排队生产指定基因组的单位簇，或发起建筑建造请求",
    "parameters": {
      "type": "object",
      "properties": {
        "vat_entity_id": { "type": "string" },
        "unit_archetype": { "type": "string" },
        "genome_slug": { "type": "string" },
        "count": { "type": "integer", "minimum": 1, "maximum": 5 }
      },
      "required": ["vat_entity_id", "unit_archetype", "genome_slug", "count"]
    }
  },
  {
    "name": "trigger_crucible_breeding",
    "description": "在演化坩埚中启动科技研发或提取战场高适应度精英基因执行定向突变繁育",
    "parameters": {
      "type": "object",
      "properties": {
        "crucible_entity_id": { "type": "string" },
        "action": { "type": "string", "enum": ["research_tech", "breed_elite_genome"] },
        "tech_or_archetype_id": { "type": "string" },
        "mutation_rate": { "type": "number", "minimum": 0.01, "maximum": 0.25 }
      },
      "required": ["crucible_entity_id", "action", "tech_or_archetype_id"]
    }
  }
]
```

### 9.2 竞技地图 180° 中心对称确定性生成器实现（补全 GAP-19）

```rust
// crates/rynvek-rts/src/map_gen.rs
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_core::rng::RandomKey;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainTileKind {
    OpenSubstrate = 0,
    ConductiveBasalt = 1,
    NutrientMarsh = 2,
    AcidicFissure = 3,
    ImperviousWall = 4,
}

/// 生成 100% 中心旋转对称的竞技场地形网格与初始环境场（保证 PvP 双方绝对公平）
pub fn generate_symmetric_arena_map(width: usize, height: usize, map_seed: u64) -> Vec<TerrainTileKind> {
    assert!(width % 2 == 0 && height % 2 == 0, "Arena dimensions must be even");
    let mut tiles = vec![TerrainTileKind::OpenSubstrate; width * height];
    let half_h = height / 2;

    for y in 0..half_h {
        for x in 0..width {
            let rk = RandomKey {
                world_seed: map_seed,
                tick: 0,
                entity_serial: ((y as u64) << 32) | (x as u64),
                stream_and_phase: 0x0001_0000,
                sub_counter: 0,
            };
            let r = rk.generate_u32x4()[0] % 1000;

            // 预留左下角 (32, 32) 半径 20 Tiles 的出生安全平地
            let dx_spawn = (x as i32) - 32;
            let dy_spawn = (y as i32) - 32;
            let dist_sq = dx_spawn * dx_spawn + dy_spawn * dy_spawn;

            let kind = if dist_sq <= 400 {
                TerrainTileKind::ConductiveBasalt
            } else if dist_sq <= 784 && (r < 180) {
                TerrainTileKind::NutrientMarsh
            } else if dist_sq <= 900 && (r >= 180 && r < 210) {
                TerrainTileKind::AcidicFissure
            } else if r < 85 {
                TerrainTileKind::ImperviousWall
            } else if r < 160 {
                TerrainTileKind::NutrientMarsh
            } else if r < 175 {
                TerrainTileKind::AcidicFissure
            } else if r < 300 {
                TerrainTileKind::ConductiveBasalt
            } else {
                TerrainTileKind::OpenSubstrate
            };

            // 同步写入 (x, y) 与 180° 中心对称点 (width - 1 - x, height - 1 - y)
            tiles[y * width + x] = kind;
            let sym_x = width - 1 - x;
            let sym_y = height - 1 - y;
            tiles[sym_y * width + sym_x] = kind;
        }
    }
    tiles
}
```

## 规范 10：分布式分区协调、持久化二进制格式、CRDT 编辑补丁与 Schema 迁移图（`rynvek-storage`, `rynvek-snapshot`, `rynvek-journal`, `rynvek-world`）

### 10.1 六大专有文件容器二进制头与 `NEUR` 稀疏神经分块规范

所有专有二进制文件（`.rworld`, `.rpack`, `.rview`, `.rsnap`, `.rjournal`, `.rreplay`）统一采用 Little-Endian 字节序，起始于 64 字节对齐文件头（`SnapshotFileHeader` 结构见卷 10），各文件魔数严格固定为：
- `.rworld` → `0x52, 0x59, 0x57, 0x4C` (`"RYWL"`)
- `.rpack`  → `0x52, 0x59, 0x50, 0x4B` (`"RYPK"`)
- `.rview`  → `0x52, 0x59, 0x56, 0x57` (`"RYVW"`)
- `.rsnap`  → `0x52, 0x59, 0x53, 0x53` (`"RYSS"`)
- `.rjournal` → `0x52, 0x59, 0x4A, 0x4C` (`"RYJL"`)
- `.rreplay`  → `0x52, 0x59, 0x52, 0x50` (`"RYRP"`)

### 10.2 `Rynvek Studio` 创作态补丁（`WorldPatch`）、CRDT 协作与四级热更新安全分类（补全 GAP-25, D135, D175, D308–D310）

```rust
// crates/rynvek-world/src/authoring.rs
// SPDX-License-Identifier: AGPL-3.0-only

/// 实时修改对运行中仿真的安全影响分级（决定何时以及如何应用修改）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveEditSafetyClass {
    /// Class 0 (HotSafe): 视觉颜色、UI 布局、AudioProfile 参数，可在任意渲染帧立即热生效，不影响权威状态哈希
    HotSafePresentation = 0,
    /// Class 1 (TickBoundarySafe): 环境场参数(D_c, lambda_c)、建筑造价、单位数值属性，打包为系统命令在下一 Tick Phase 01 生效
    TickBoundarySafe = 1,
    /// Class 2 (EpochBoundarySafe): 神经网络拓扑规则、TypeScript 规则热重载 (.rir)，必须在下一个 Coupling Epoch (tick % 4 == 0) 原子切换
    EpochBoundarySafe = 2,
    /// Class 3 (RequiresForkRestart): 更改地图 Chunk 尺寸、更改空间维度 (2D->3D) 或更改数值 Profile (D2->D0)，必须创建新 WorldFork 分支重启
    RequiresForkRestart = 3,
}

/// 混合逻辑时钟时间戳 (Hybrid Logical Clock, 用于多创作者 CRDT 协作偏序合并)
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct HlcTimestamp {
    pub wall_time_ms: u64,
    pub logical_counter: u32,
    pub author_node_id: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorldPatch {
    pub patch_id: [u8; 16],
    pub base_authoring_revision: u64,
    pub hlc: HlcTimestamp,
    pub safety_class: LiveEditSafetyClass,
    pub operations: Vec<WorldPatchOp>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum WorldPatchOp {
    SetWorldParam { key_path: String, new_value_json: String },
    PaintTerrainRect { x0: i32, y0: i32, width: u32, height: u32, tile_kind: u8 },
    UpsertSeedGenome { genome_slug: String, genome_bytes_b64: String },
    UpdateRuleModule { module_id: String, ts_source: String, compiled_rir_blake3: [u8; 32] },
    RemoveEntityTemplate { template_id: String },
}
```

### 10.3 依赖锁定文件（`dependencies.lock`）与 Schema 有向迁移图（`MigrationGraph`）BFS 算法（补全 GAP-28, D341, D346, D660）

#### 10.3.1 `dependencies.lock` 标准格式（JSON Schema）

```json
{
  "lockfile_version": 1,
  "world_schema": "rynvek.world.v1",
  "engine_compat": ">=0.1.0 <0.2.0",
  "root_package_hash": "blake3:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "packages": [
    {
      "id": "rynvek.rules.rts-standard",
      "version": "1.0.0",
      "kind": "rpack",
      "blake3_256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
      "ed25519_signer": "rynvek-official-release-key-2026",
      "ed25519_sig": "base64:...",
      "dependencies": []
    },
    {
      "id": "rynvek.visual.industrial-default",
      "version": "1.0.0",
      "kind": "rview",
      "blake3_256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "ed25519_signer": "rynvek-official-release-key-2026",
      "ed25519_sig": "base64:...",
      "dependencies": []
    }
  ]
}
```

#### 10.3.2 Schema 有向迁移图（`MigrationGraph`）最短升级路径搜索实现

```rust
// crates/rynvek-schema/src/migration.rs
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{BTreeMap, VecDeque};

pub type SchemaRevisionId = (String, u32); // (domain_name, version_num)

pub struct MigrationEdge {
    pub from_ver: u32,
    pub to_ver: u32,
    pub is_lossless: bool,
    pub transform_fn: fn(&mut serde_json::Value) -> Result<(), String>,
}

pub struct SchemaMigrationGraph {
    pub edges_by_domain: BTreeMap<String, Vec<MigrationEdge>>,
}

impl SchemaMigrationGraph {
    /// 使用确定性 BFS 寻找从存档版本 `from_ver` 到当前引擎版本 `target_ver` 的最短无损迁移路径
    pub fn find_migration_path(&self, domain: &str, from_ver: u32, target_ver: u32) -> Result<Vec<usize>, String> {
        if from_ver == target_ver {
            return Ok(Vec::new());
        }
        let edges = self.edges_by_domain.get(domain)
            .ok_or_else(|| format!("Unknown schema domain: {domain}"))?;

        let mut queue = VecDeque::new();
        let mut parent_map: BTreeMap<u32, (u32, usize)> = BTreeMap::new();
        queue.push_back(from_ver);

        while let Some(curr) = queue.pop_front() {
            if curr == target_ver {
                let mut path = Vec::new();
                let mut trace = target_ver;
                while trace != from_ver {
                    let &(prev, edge_idx) = parent_map.get(&trace).unwrap();
                    path.push(edge_idx);
                    trace = prev;
                }
                path.reverse();
                return Ok(path);
            }
            for (idx, edge) in edges.iter().enumerate() {
                if edge.from_ver == curr && !parent_map.contains_key(&edge.to_ver) {
                    parent_map.insert(edge.to_ver, (curr, idx));
                    queue.push_back(edge.to_ver);
                }
            }
        }
        Err(format!("No valid schema migration path for {domain} from v{from_ver} to v{target_ver}"))
    }
}
```

## 规范 11：双栈网络传输、PostgreSQL 17 / SQLite 完整 DDL、API 路由表与全量错误码表（`rynvek-network`, `rynvek-server`, `rynvek-security`）

### 11.1 PostgreSQL 17 服务端权威数据库完整建表 DDL（补全 GAP-22）

```sql
-- crates/rynvek-storage/migrations/postgres/0001_initial_schema.sql
-- SPDX-License-Identifier: AGPL-3.0-only

CREATE TABLE principals (
    principal_id BIGINT PRIMARY KEY,
    handle VARCHAR(64) NOT NULL UNIQUE,
    principal_kind SMALLINT NOT NULL CHECK (principal_kind IN (0, 1, 2)), -- 0: Human, 1: Bot/Agent, 2: Service
    ed25519_pubkey BYTEA NOT NULL CHECK (octet_length(ed25519_pubkey) = 32),
    email_hash BYTEA,
    status SMALLINT NOT NULL DEFAULT 0, -- 0: Active, 1: Suspended, 2: Deleted
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE organizations (
    org_id BIGINT PRIMARY KEY,
    slug VARCHAR(64) NOT NULL UNIQUE,
    display_name VARCHAR(128) NOT NULL,
    owner_principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    quota_max_worlds INT NOT NULL DEFAULT 10,
    quota_storage_bytes BIGINT NOT NULL DEFAULT 10737418240, -- 10 GiB
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE worlds (
    world_id UUID PRIMARY KEY,
    org_id BIGINT NOT NULL REFERENCES organizations(org_id),
    owner_principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    title VARCHAR(128) NOT NULL,
    world_profile VARCHAR(32) NOT NULL,     -- RTS_Standard, Sandbox_Creative, Research_Batch, Persistent_Ecology
    game_mode VARCHAR(32) NOT NULL,         -- PvP_Ranked, PvP_Custom, PvE_Survival, Evolution_Benchmark
    execution_profile VARCHAR(32) NOT NULL, -- D0_Strict, D1_Reproducible, D2_Bounded, D3_Statistical
    visibility SMALLINT NOT NULL DEFAULT 0, -- 0: Private, 1: Unlisted, 2: Public
    parent_world_id UUID REFERENCES worlds(world_id),
    forked_at_revision BIGINT,
    latest_tick BIGINT NOT NULL DEFAULT 0,
    latest_revision BIGINT NOT NULL DEFAULT 0,
    latest_state_root BYTEA NOT NULL CHECK (octet_length(latest_state_root) = 32),
    lease_holder_node VARCHAR(128),
    fencing_token BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_worlds_owner ON worlds(owner_principal_id);
CREATE INDEX idx_worlds_public ON worlds(visibility, updated_at DESC) WHERE visibility = 2;

CREATE TABLE world_commits (
    world_id UUID NOT NULL REFERENCES worlds(world_id) ON DELETE CASCADE,
    world_revision BIGINT NOT NULL,
    tick BIGINT NOT NULL,
    parent_revision BIGINT,
    state_root_blake3 BYTEA NOT NULL CHECK (octet_length(state_root_blake3) = 32),
    journal_segment_cid BYTEA NOT NULL CHECK (octet_length(journal_segment_cid) = 32),
    snapshot_cid BYTEA CHECK (snapshot_cid IS NULL OR octet_length(snapshot_cid) = 32),
    committed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (world_id, world_revision)
);

CREATE TABLE packages (
    package_id VARCHAR(128) PRIMARY KEY, -- e.g. "rynvek.visual.industrial-default"
    org_id BIGINT NOT NULL REFERENCES organizations(org_id),
    package_kind VARCHAR(16) NOT NULL CHECK (package_kind IN ('rworld', 'rpack', 'rview', 'audio', 'agent', 'dataset')),
    trust_level SMALLINT NOT NULL DEFAULT 3, -- 0: Builtin, 1: Official, 2: Verified, 3: Community
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE package_versions (
    package_id VARCHAR(128) NOT NULL REFERENCES packages(package_id) ON DELETE CASCADE,
    semver VARCHAR(32) NOT NULL,
    min_engine_version VARCHAR(32) NOT NULL,
    license_spdx VARCHAR(64) NOT NULL,
    artifact_cid_blake3 BYTEA NOT NULL CHECK (octet_length(artifact_cid_blake3) = 32),
    artifact_size_bytes BIGINT NOT NULL,
    ed25519_signature BYTEA NOT NULL CHECK (octet_length(ed25519_signature) = 64),
    manifest_json JSONB NOT NULL,
    lockfile_json JSONB NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (package_id, semver)
);

CREATE TABLE matches (
    match_id UUID PRIMARY KEY,
    world_id UUID NOT NULL REFERENCES worlds(world_id),
    game_mode VARCHAR(32) NOT NULL,
    ruleset_blake3 BYTEA NOT NULL CHECK (octet_length(ruleset_blake3) = 32),
    map_seed BIGINT NOT NULL,
    match_state VARCHAR(16) NOT NULL, -- Created, Lobby, Ready, Running, Completed, Aborted
    winner_faction_id SMALLINT,
    win_reason VARCHAR(32),           -- Elimination, Surrender, TieBreakerScore
    total_ticks BIGINT NOT NULL DEFAULT 0,
    replay_cid_blake3 BYTEA CHECK (replay_cid_blake3 IS NULL OR octet_length(replay_cid_blake3) = 32),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE TABLE match_participants (
    match_id UUID NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
    faction_id SMALLINT NOT NULL,
    principal_id BIGINT NOT NULL REFERENCES principals(principal_id),
    controller_league VARCHAR(16) NOT NULL CHECK (controller_league IN ('human', 'script', 'agent_hybrid')),
    final_score BIGINT NOT NULL DEFAULT 0,
    rating_delta DOUBLE PRECISION,
    PRIMARY KEY (match_id, faction_id)
);

CREATE TABLE glicko2_ratings (
    principal_id BIGINT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    league VARCHAR(16) NOT NULL CHECK (league IN ('human', 'script', 'agent_hybrid')),
    rating DOUBLE PRECISION NOT NULL DEFAULT 1500.0,
    deviation DOUBLE PRECISION NOT NULL DEFAULT 350.0,
    volatility DOUBLE PRECISION NOT NULL DEFAULT 0.06,
    matches_played INT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (principal_id, league)
);

CREATE TABLE audit_logs (
    log_id BIGSERIAL PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    principal_id BIGINT,
    action_type VARCHAR(64) NOT NULL,
    target_resource VARCHAR(128) NOT NULL,
    client_ip INET,
    details_json JSONB NOT NULL
);
```

### 11.2 SQLite / OPFS 客户端本地存储完整 DDL（补全 GAP-22）

```sql
-- crates/rynvek-storage/migrations/sqlite/0001_local_storage.sql
-- SPDX-License-Identifier: AGPL-3.0-only

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE local_worlds (
    world_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    world_profile TEXT NOT NULL,
    game_mode TEXT NOT NULL,
    execution_profile TEXT NOT NULL,
    latest_tick INTEGER NOT NULL DEFAULT 0,
    latest_revision INTEGER NOT NULL DEFAULT 0,
    latest_state_root_hex TEXT NOT NULL,
    opfs_dir_path TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE local_snapshots (
    snapshot_cid_hex TEXT PRIMARY KEY,
    world_id TEXT NOT NULL REFERENCES local_worlds(world_id) ON DELETE CASCADE,
    tick INTEGER NOT NULL,
    world_revision INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    opfs_file_path TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE local_assets_lru (
    asset_cid_hex TEXT PRIMARY KEY,
    package_id TEXT NOT NULL,
    asset_kind TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    last_accessed_ms INTEGER NOT NULL,
    opfs_blob_path TEXT NOT NULL
);

CREATE TABLE user_preferences (
    pref_key TEXT PRIMARY KEY,
    pref_value_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
```

### 11.3 `Rynvek Server` & `Rynvek Hub` 完整 HTTP REST / WebTransport API 路由表（补全 GAP-23）

| 序号 | 方法 / 协议 | 路由路径 (`URI Path`) | 所需权限 (`CapabilityScope`) | 请求体 / 响应体契约说明 |
| :--- | :--- | :--- | :--- | :--- |
| `01` | `GET` | `/api/v1/meta/health` | `Public` | 返回 `{ status: "ok", uptime_s, active_worlds, gpu_tier }` |
| `02` | `GET` | `/api/v1/meta/source-offer` | `Public` (AGPLv3 §13) | 返回当前运行二进制对应的精确 Git Commit、源码 Tarball URL 与 SPDX SBOM 链接 |
| `03` | `POST` | `/api/v1/auth/session` | `Public` | 验证 Ed25519 质询签名或 OAuth 令牌，签发 96 字节二进制 `CapabilityToken` |
| `04` | `GET` | `/api/v1/worlds` | `world.list` | 分页查询公开或当前租户拥有的世界列表 |
| `05` | `POST` | `/api/v1/worlds` | `world.create` | 上传 `.rworld` 初始包或指定预设模板创建新权威世界实例 |
| `06` | `GET` | `/api/v1/worlds/{world_id}/commits` | `world.read` | 获取世界的 `CommitManifest` 历史链与最新 `.rsnap` 下载 CID |
| `07` | `POST` | `/api/v1/worlds/{world_id}/fork` | `world.fork` | 基于指定 `world_revision` 执行 Copy-on-Write 分支创建新 `world_id` |
| `08` | `POST` | `/api/v1/worlds/{world_id}/patches` | `world.edit` | `Rynvek Studio` 提交 `WorldPatch`（按 `LiveEditSafetyClass` 调度生效） |
| `09` | `WT / WS` | `/api/v1/worlds/{world_id}/stream` | `world.connect` | 建立双栈实时仿真流（承载 `WireChannelId 0..=5` 二进制帧） |
| `10` | `POST` | `/api/v1/matches/queue` | `match.play` | 加入竞技匹配队列（指定 `league: human|script|agent_hybrid` 与 `ruleset_hash`） |
| `11` | `GET` | `/api/v1/matches/{match_id}` | `Public` | 查询对局元数据、参赛阵营、实时状态与最终胜负得分 |
| `12` | `GET` | `/api/v1/matches/{match_id}/replay` | `Public` | 下载经服务端签名的 `.rreplay` 确定性回放归档文件 |
| `13` | `GET` | `/api/v1/rankings/{league}` | `Public` | 查询指定赛道（人类/脚本/混合 Agent）的 Glicko-2 排行榜 |
| `14` | `GET` | `/api/v1/hub/packages` | `Public` | 按类型（`rworld/rpack/rview/audio`）、关键词与兼容版本检索 `Rynvek Hub` 资源包 |
| `15` | `PUT` | `/api/v1/hub/packages/{id}/{ver}` | `hub.publish` | 上传带 Ed25519 签名与 `dependencies.lock` 的包，触发服务端静态扫描与沙箱验证 |
| `16` | `GET` | `/api/v1/hub/blobs/{blake3_hex}` | `Public` | 从内容寻址存储（CAS）高速下载不可变二进制资产块（支持 HTTP Range 断点续传） |
| `17` | `POST` | `/api/v1/lab/experiments` | `lab.submit` | 提交 `Rynvek Lab` 批量多随机种子演化实验任务 |
| `18` | `GET` | `/api/v1/lab/experiments/{exp_id}/parquet` | `lab.read` | 流式下载实验产出的演化谱系与统计指标 Parquet 文件 |
| `19` | `POST` | `/api/v1/user/data-export` | `account.owner` | GDPR/CCPA 合规一键导出用户全部世界、包与对局记录归档 |
| `20` | `DELETE` | `/api/v1/user/account` | `account.owner` | 触发账号注销与 24h 热库抹除 / 30d 冷备滚动清理流程 |

### 11.4 跨语言统一错误码注册表（`RynvekErrorCode` `0x1000..0x9FFF`）与恢复策略（补全 GAP-24, D184, D438）

```rust
// crates/rynvek-core/src/error.rs
// SPDX-License-Identifier: AGPL-3.0-only

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RynvekErrorCode {
    // --- 0x1000: Core, Coordinate & Numeric Errors ---
    NumericNaNOrInfDetected = 0x1001,
    FixedPointOverflow = 0x1002,
    CoordinateOutOfCanonicalRange = 0x1003,
    DimensionMismatch = 0x1004,

    // --- 0x2000: ECS, Handle & Memory Errors ---
    StaleEntityHandleGeneration = 0x2001,
    EntityNotFoundOrTombstoned = 0x2002,
    ArchetypeComponentMissing = 0x2003,
    WorldMemoryBudgetExceeded = 0x2004,
    StructuralMutationConflict = 0x2005,

    // --- 0x3000: Simulation, Clock & Phase DAG Errors ---
    PhaseDependencyCycleDetected = 0x3001,
    UnresolvedWriteConflict = 0x3002,
    SimulationInvariantViolated = 0x3003,
    TickDeadlineExceeded = 0x3004,

    // --- 0x4000: Neural Soup & Continuous Field Errors ---
    NeuralTopologyCapacityExceeded = 0x4001,
    DaleLawSignViolation = 0x4002,
    FieldCflStabilityViolation = 0x4003,
    ConservationLawMismatch = 0x4004,

    // --- 0x5000: Rules Compiler, QuickJS & WASM Sandbox Errors ---
    RuleStaticTypeCheckFailed = 0x5001,
    RuleUndeclaredWriteAccess = 0x5002,
    RuleLoopBoundExceeded = 0x5003,
    SandboxGasBudgetExhausted = 0x5004,
    SandboxMemoryLimitExceeded = 0x5005,
    PluginCapabilityDenied = 0x5006,

    // --- 0x6000: WebGPU, Compute & Render Errors ---
    WebGpuAdapterUnavailable = 0x6001,
    GpuDeviceLost = 0x6002,
    GpuBufferAllocationFailed = 0x6003,
    WgslShaderValidationFailed = 0x6004,

    // --- 0x7000: Storage, Snapshot, Journal & Migration Errors ---
    InvalidFileMagicOrVersion = 0x7001,
    Blake3ChecksumMismatch = 0x7002,
    JournalTornWriteTruncated = 0x7003,
    SchemaMigrationPathNotFound = 0x7004,
    StalePartitionFencingToken = 0x7005,

    // --- 0x8000: Network, Command, Match & Security Errors ---
    UnauthorizedCapabilityToken = 0x8001,
    DuplicateCommandIdempotencyKey = 0x8002,
    CommandExpiredOrFutureTooFar = 0x8003,
    CommandRateLimitExceeded = 0x8004,
    InsufficientFactionResources = 0x8005,
    InvalidBuildingPlacementGrid = 0x8006,

    // --- 0x9000: Package, Hub & Asset Pipeline Errors ---
    InvalidEd25519Signature = 0x9001,
    RevokedPackageBlocked = 0x9002,
    DependencyLockConflict = 0x9003,
    UnsafeRviewShaderRejected = 0x9004,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorRecoveryAction {
    /// 仅拒绝当前单条非法玩家/Agent 命令，世界继续正常推进 (适用于 0x8001..0x8006)
    RejectCommandOnly = 0,
    /// 立即隔离并禁用超限的第三方规则或 WASM 插件，回退该插件当帧写入 (适用于 0x5004..0x5006)
    IsolateFaultyExtension = 1,
    /// 丢弃当前未提交的 CandidateState[t]，回滚至 Committed[t-1] 并告警 (适用于 0x1001, 0x3003, 0x4004)
    RollbackTickCandidate = 2,
    /// 触发 GPU DeviceLost 5 步恢复机或自动降级至 CPU Reference 后端 (适用于 0x6001..0x6003)
    RecoverOrDegradeBackend = 3,
    /// 从最近 .rsnap + .rjournal 执行完整崩溃恢复状态机 (适用于 0x7002..0x7003)
    TriggerStorageRecovery = 4,
}
```

## 规范 12：前端设计系统（`@rynvek/ui`）、36 项输入动作映射全表、程序化音频合成器代码与跨语言 Golden Test Vectors

### 12.1 统一输入抽象层（`InputAbstraction`）与 36 项多上下文动作绑定全表（补全 GAP-27, D642–D644）

所有前端交互严禁在业务组件中直接监听 `window.addEventListener("keydown")` 硬编码键值，必须统一经过 `@rynvek/client::InputActionRouter` 映射为以下 36 个标准逻辑动作（`LogicalActionId`）：

| 编号 | 逻辑动作 ID (`LogicalActionId`) | 作用域 (`InteractionContext`) | 默认桌面键鼠绑定 | 默认触控 / 平板手势 | 默认手柄（Gamepad / Steam Deck） |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `01` | `global.command_palette` | `Global` | `Ctrl+K` / `Cmd+K` | 三指轻点顶部栏 | `View + Menu` |
| `02` | `global.cancel_or_menu` | `Global` | `Escape` | 左上角返回键 | `B` (East) |
| `03` | `global.toggle_inspector` | `Global` | `Tab` | 右侧边缘左滑 | `Y` (North) |
| `04` | `global.toggle_minimap` | `Global` | `M` | 点击小地图缩略框 | `D-Pad Up` |
| `05` | `global.toggle_neural_overlay` | `Global` | `N` | 双指长按空白处 | `D-Pad Right` |
| `06` | `global.toggle_field_overlay` | `Global` | `F` | 工具栏 Field 按钮 | `D-Pad Left` |
| `07` | `global.cycle_audio_mute` | `Global` | `Ctrl+M` | 状态栏音频图标 | — |
| `08` | `camera.pan_up` | `Canvas` | `W` / `ArrowUp` | 单指拖拽平移 | 左摇杆 `LS Up` |
| `09` | `camera.pan_down` | `Canvas` | `S` / `ArrowDown` | 单指拖拽平移 | 左摇杆 `LS Down` |
| `10` | `camera.pan_left` | `Canvas` | `A` / `ArrowLeft` | 单指拖拽平移 | 左摇杆 `LS Left` |
| `11` | `camera.pan_right` | `Canvas` | `D` / `ArrowRight` | 单指拖拽平移 | 左摇杆 `LS Right` |
| `12` | `camera.zoom_in` | `Canvas` | `WheelUp` / `=` | 双指张开捏合 | `RT` (右扳机) |
| `13` | `camera.zoom_out` | `Canvas` | `WheelDown` / `-` | 双指收缩捏合 | `LT` (左扳机) |
| `14` | `camera.jump_to_nexus` | `Canvas` | `Backspace` / `Home` | 双击主脑头像 | `LS Click (L3)` |
| `15` | `rts.select_primary` | `RTS_Gameplay` | `MouseLeftClick` | 单指点击 | `A` (South) |
| `16` | `rts.box_select` | `RTS_Gameplay` | `MouseLeftDrag` | 长按 150ms 后拖框 | 按住 `A` + 右摇杆画框 |
| `17` | `rts.select_add_toggle` | `RTS_Gameplay` | `Shift+MouseLeft` | 多选模式开关 + 点击 | `LB + A` |
| `18` | `rts.select_same_archetype` | `RTS_Gameplay` | `DoubleClick` / `Ctrl+Click` | 双击目标单位 | 双击 `A` |
| `19` | `rts.context_order` | `RTS_Gameplay` | `MouseRightClick` | 双指点击目标点 | `X` (West) |
| `20` | `rts.order_attack_move` | `RTS_Gameplay` | `Q` + `LeftClick` | 战术轮盘 `Attack` | `RB + X` |
| `21` | `rts.order_hold_position` | `RTS_Gameplay` | `H` | 战术轮盘 `Hold` | `RB + B` |
| `22` | `rts.order_patrol` | `RTS_Gameplay` | `P` + `LeftClick` | 战术轮盘 `Patrol` | `RB + Y` |
| `23` | `rts.open_build_menu` | `RTS_Gameplay` | `B` | 底部建造抽屉按钮 | `LB` |
| `24` | `rts.rotate_building_cw` | `RTS_Gameplay` | `R` | 建造虚影旋转图标 | `RS Click (R3)` |
| `25` | `rts.demolish_selected` | `RTS_Gameplay` | `Delete` / `X` | 长按删除按钮 | `LB + RS Click` |
| `26` | `rts.beacon_modulator_panel` | `RTS_Gameplay` | `E` | 点击信标调制环 | `RB + A` |
| `27` | `rts.release_human_lease` | `RTS_Gameplay` | `Alt+A` | 顶栏交还 AI 控制按钮 | `D-Pad Down` |
| `28` | `rts.assign_control_group_1_9` | `RTS_Gameplay` | `Ctrl+1..9` | 长按底部编队槽 1..9 | — |
| `29` | `rts.recall_control_group_1_9` | `RTS_Gameplay` | `1..9` | 点击底部编队槽 1..9 | — |
| `30` | `studio.sim_play_pause` | `Studio_IDE` | `Space` | 顶栏播放/暂停键 | `Start` |
| `31` | `studio.sim_step_single_tick` | `Studio_IDE` | `Period (.)` | 顶栏单步前进键 | `RB` |
| `32` | `studio.tool_brush_field` | `Studio_IDE` | `G` | 工具栏画笔图标 | — |
| `33` | `studio.tool_spawn_organism` | `Studio_IDE` | `V` | 工具栏孵化图标 | — |
| `34` | `studio.undo_patch` | `Studio_IDE` | `Ctrl+Z` / `Cmd+Z` | 双指左滑撤销 | — |
| `35` | `studio.redo_patch` | `Studio_IDE` | `Ctrl+Shift+Z` | 双指右滑重做 | — |
| `36` | `studio.compile_and_hot_reload` | `Code_Editor` | `Ctrl+S` / `Cmd+S` | 编辑器右上角 Apply | — |

### 12.2 Web Audio 程序化占位音效合成器完整实现代码（`@rynvek/audio` — 直接用于实现）

以下代码完整实现了卷 13 / 项目文档定义的 24 类语义音效合成器，零外部音频文件依赖即可在浏览器中运行：

```typescript
// packages/audio/src/placeholder_synth.ts
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

export interface SynthRecipe {
  readonly bus: "sfx_ui" | "sfx_world" | "sfx_combat" | "voice_alert" | "neural_sonify";
  readonly oscType: OscillatorType;
  readonly freqStart: number;
  readonly freqEnd: number;
  readonly attackMs: number;
  readonly decayMs: number;
  readonly sustainLevel: number;
  readonly releaseMs: number;
  readonly filterType: BiquadFilterType;
  readonly filterFreq: number;
  readonly filterQ: number;
  readonly noiseMix: number; // [0.0, 1.0]
}

export const PLACEHOLDER_SYNTH_RECIPES: Readonly<Record<string, SynthRecipe>> = {
  "ui.hover":                  { bus: "sfx_ui",      oscType: "sine",     freqStart: 880,  freqEnd: 920,  attackMs: 2,  decayMs: 18,  sustainLevel: 0.0,  releaseMs: 10,  filterType: "highpass", filterFreq: 600,  filterQ: 0.7, noiseMix: 0.0  },
  "ui.click":                  { bus: "sfx_ui",      oscType: "triangle", freqStart: 1320, freqEnd: 440,  attackMs: 1,  decayMs: 35,  sustainLevel: 0.0,  releaseMs: 14,  filterType: "bandpass", filterFreq: 1200, filterQ: 1.5, noiseMix: 0.1  },
  "ui.confirm":                { bus: "sfx_ui",      oscType: "triangle", freqStart: 587,  freqEnd: 880,  attackMs: 3,  decayMs: 80,  sustainLevel: 0.2,  releaseMs: 60,  filterType: "lowpass",  filterFreq: 3200, filterQ: 1.0, noiseMix: 0.0  },
  "ui.cancel":                 { bus: "sfx_ui",      oscType: "sawtooth", freqStart: 440,  freqEnd: 330,  attackMs: 3,  decayMs: 70,  sustainLevel: 0.1,  releaseMs: 50,  filterType: "lowpass",  filterFreq: 1800, filterQ: 1.2, noiseMix: 0.0  },
  "ui.error":                  { bus: "voice_alert", oscType: "sawtooth", freqStart: 165,  freqEnd: 147,  attackMs: 5,  decayMs: 140, sustainLevel: 0.4,  releaseMs: 95,  filterType: "lowpass",  filterFreq: 950,  filterQ: 2.5, noiseMix: 0.05 },
  "ui.panel_toggle":           { bus: "sfx_ui",      oscType: "sine",     freqStart: 300,  freqEnd: 750,  attackMs: 4,  decayMs: 45,  sustainLevel: 0.0,  releaseMs: 21,  filterType: "bandpass", filterFreq: 800,  filterQ: 1.8, noiseMix: 0.15 },
  "building.placed":           { bus: "sfx_world",   oscType: "square",   freqStart: 220,  freqEnd: 110,  attackMs: 2,  decayMs: 65,  sustainLevel: 0.15, releaseMs: 53,  filterType: "lowpass",  filterFreq: 1600, filterQ: 2.0, noiseMix: 0.2  },
  "building.completed":        { bus: "sfx_world",   oscType: "triangle", freqStart: 523,  freqEnd: 1046, attackMs: 4,  decayMs: 90,  sustainLevel: 0.25, releaseMs: 86,  filterType: "lowpass",  filterFreq: 4000, filterQ: 1.0, noiseMix: 0.0  },
  "building.destroyed":        { bus: "sfx_combat",  oscType: "sawtooth", freqStart: 140,  freqEnd: 38,   attackMs: 4,  decayMs: 220, sustainLevel: 0.2,  releaseMs: 226, filterType: "lowpass",  filterFreq: 450,  filterQ: 1.8, noiseMix: 0.65 },
  "building.extractor_cycle":  { bus: "sfx_world",   oscType: "sine",     freqStart: 180,  freqEnd: 240,  attackMs: 8,  decayMs: 50,  sustainLevel: 0.1,  releaseMs: 32,  filterType: "bandpass", filterFreq: 420,  filterQ: 3.0, noiseMix: 0.05 },
  "unit.spawned":              { bus: "sfx_world",   oscType: "sine",     freqStart: 330,  freqEnd: 660,  attackMs: 6,  decayMs: 75,  sustainLevel: 0.2,  releaseMs: 59,  filterType: "lowpass",  filterFreq: 2800, filterQ: 1.4, noiseMix: 0.0  },
  "unit.selected":             { bus: "sfx_ui",      oscType: "triangle", freqStart: 659,  freqEnd: 988,  attackMs: 2,  decayMs: 48,  sustainLevel: 0.0,  releaseMs: 30,  filterType: "bandpass", filterFreq: 1500, filterQ: 1.2, noiseMix: 0.0  },
  "unit.order_move":           { bus: "sfx_ui",      oscType: "sine",     freqStart: 523,  freqEnd: 659,  attackMs: 2,  decayMs: 38,  sustainLevel: 0.0,  releaseMs: 25,  filterType: "lowpass",  filterFreq: 2400, filterQ: 1.0, noiseMix: 0.0  },
  "unit.order_attack":         { bus: "sfx_ui",      oscType: "sawtooth", freqStart: 440,  freqEnd: 880,  attackMs: 2,  decayMs: 55,  sustainLevel: 0.15, releaseMs: 38,  filterType: "bandpass", filterFreq: 1800, filterQ: 2.2, noiseMix: 0.05 },
  "combat.weapon_pulse":       { bus: "sfx_combat",  oscType: "sawtooth", freqStart: 1480, freqEnd: 220,  attackMs: 1,  decayMs: 55,  sustainLevel: 0.0,  releaseMs: 24,  filterType: "bandpass", filterFreq: 900,  filterQ: 2.5, noiseMix: 0.15 },
  "combat.spore_launch":       { bus: "sfx_combat",  oscType: "sine",     freqStart: 260,  freqEnd: 520,  attackMs: 5,  decayMs: 70,  sustainLevel: 0.1,  releaseMs: 45,  filterType: "bandpass", filterFreq: 650,  filterQ: 4.5, noiseMix: 0.35 },
  "combat.impact_kinetic":     { bus: "sfx_combat",  oscType: "triangle", freqStart: 190,  freqEnd: 65,   attackMs: 1,  decayMs: 32,  sustainLevel: 0.0,  releaseMs: 22,  filterType: "lowpass",  filterFreq: 1400, filterQ: 1.5, noiseMix: 0.5  },
  "combat.shield_hit":         { bus: "sfx_combat",  oscType: "sine",     freqStart: 1760, freqEnd: 880,  attackMs: 2,  decayMs: 68,  sustainLevel: 0.1,  releaseMs: 40,  filterType: "highpass", filterFreq: 900,  filterQ: 2.8, noiseMix: 0.1  },
  "combat.explosion_large":    { bus: "sfx_combat",  oscType: "sine",     freqStart: 95,   freqEnd: 28,   attackMs: 3,  decayMs: 280, sustainLevel: 0.25, releaseMs: 317, filterType: "lowpass",  filterFreq: 320,  filterQ: 1.6, noiseMix: 0.8  },
  "neural.spike_cluster":      { bus: "neural_sonify", oscType: "sine",   freqStart: 659,  freqEnd: 784,  attackMs: 1,  decayMs: 28,  sustainLevel: 0.0,  releaseMs: 16,  filterType: "bandpass", filterFreq: 1600, filterQ: 3.5, noiseMix: 0.0  },
  "neural.sync_lock":          { bus: "neural_sonify", oscType: "triangle", freqStart: 262, freqEnd: 392, attackMs: 40, decayMs: 180, sustainLevel: 0.5,  releaseMs: 180, filterType: "lowpass",  filterFreq: 2200, filterQ: 1.2, noiseMix: 0.0  },
  "neural.mutation_epoch":     { bus: "sfx_world",   oscType: "sine",     freqStart: 440,  freqEnd: 1760, attackMs: 5,  decayMs: 120, sustainLevel: 0.3,  releaseMs: 115, filterType: "highpass", filterFreq: 500,  filterQ: 1.5, noiseMix: 0.0  },
  "field.beacon_pulse":        { bus: "sfx_world",   oscType: "triangle", freqStart: 110,  freqEnd: 220,  attackMs: 15, decayMs: 110, sustainLevel: 0.2,  releaseMs: 95,  filterType: "lowpass",  filterFreq: 900,  filterQ: 2.0, noiseMix: 0.0  },
  "alert.nexus_under_attack":  { bus: "voice_alert", oscType: "square",   freqStart: 784,  freqEnd: 587,  attackMs: 5,  decayMs: 180, sustainLevel: 0.7,  releaseMs: 115, filterType: "bandpass", filterFreq: 1400, filterQ: 2.0, noiseMix: 0.05 },
};

export class ProceduralPlaceholderSynth {
  private readonly ctx: AudioContext;
  private readonly noiseBuffer: AudioBuffer;

  public constructor(ctx: AudioContext) {
    this.ctx = ctx;
    this.noiseBuffer = this.createPinkNoiseBuffer(1.0);
  }

  public trigger(eventId: string, destination: AudioNode, intensity = 1.0, pan = 0.0): void {
    const r = PLACEHOLDER_SYNTH_RECIPES[eventId];
    if (!r || this.ctx.state !== "running") return;

    const now = this.ctx.currentTime;
    const tAttack = now + r.attackMs / 1000;
    const tDecay = tAttack + r.decayMs / 1000;
    const tEnd = tDecay + r.releaseMs / 1000;

    const filter = this.ctx.createBiquadFilter();
    filter.type = r.filterType;
    filter.frequency.setValueAtTime(r.filterFreq, now);
    filter.Q.setValueAtTime(r.filterQ, now);

    const panner = this.ctx.createStereoPanner();
    panner.pan.setValueAtTime(Math.max(-1, Math.min(1, pan)), now);

    const env = this.ctx.createGain();
    const peakGain = Math.max(0.01, Math.min(1.0, intensity)) * 0.35;
    env.gain.setValueAtTime(0.0001, now);
    env.gain.linearRampToValueAtTime(peakGain, tAttack);
    env.gain.exponentialRampToValueAtTime(Math.max(0.001, peakGain * r.sustainLevel), tDecay);
    env.gain.exponentialRampToValueAtTime(0.0001, tEnd);

    const osc = this.ctx.createOscillator();
    osc.type = r.oscType;
    osc.frequency.setValueAtTime(r.freqStart, now);
    osc.frequency.exponentialRampToValueAtTime(Math.max(20, r.freqEnd), tEnd);

    osc.connect(filter);
    if (r.noiseMix > 0) {
      const noise = this.ctx.createBufferSource();
      noise.buffer = this.noiseBuffer;
      const noiseGain = this.ctx.createGain();
      noiseGain.gain.setValueAtTime(r.noiseMix, now);
      noise.connect(noiseGain).connect(filter);
      noise.start(now);
      noise.stop(tEnd);
    }

    filter.connect(env).connect(panner).connect(destination);
    osc.start(now);
    osc.stop(tEnd);
  }

  private createPinkNoiseBuffer(durationSec: number): AudioBuffer {
    const len = Math.floor(this.ctx.sampleRate * durationSec);
    const buf = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
    const out = buf.getChannelData(0);
    let b0 = 0, b1 = 0, b2 = 0;
    for (let i = 0; i < len; i++) {
      const white = ((i * 1664525 + 1013904223) >>> 0) / 0xFFFFFFFF * 2 - 1;
      b0 = 0.99765 * b0 + white * 0.0990460;
      b1 = 0.96300 * b1 + white * 0.2965164;
      b2 = 0.57000 * b2 + white * 1.0526913;
      out[i] = (b0 + b1 + b2 + white * 0.1848) * 0.12;
    }
    return buf;
  }
}
```

### 12.3 跨语言确定性 Golden Test Vectors（`tests/golden_vectors/v1_baseline.json`）

AI Coding Agent 在完成 `M0` 与 `M2` 阶段时，必须编写自动化测试验证以下精确输入与期望位级输出（任何一比特不匹配即视为实现存在 bug）：

```json
{
  "suite": "rynvek.golden_vectors.v1",
  "vectors": [
    {
      "id": "GV-001-Q32_32_ARITHMETIC",
      "description": "Q32_32 定点乘法、安全除法与 Newton-Raphson 确定性整数平方根验证",
      "inputs": {
        "a_raw_i64": "10737418240",
        "b_raw_i64": "6442450944",
        "sqrt_target_raw_i64": "38654705664"
      },
      "expected": {
        "mul_raw_i64": "16106127360",
        "div_raw_i64": "7158278826",
        "sqrt_raw_i64": "12884901888"
      }
    },
    {
      "id": "GV-002-WORLD_POS_CANONICALIZE",
      "description": "跨 Chunk 负向与正向边界溢出的规范化坐标转换验证",
      "inputs": {
        "chunk_x": 3,
        "chunk_y": -2,
        "local_x_raw": "1108101562368",
        "local_y_raw": "-4294967296"
      },
      "expected": {
        "chunk_x": 4,
        "chunk_y": -3,
        "local_x_raw": "8589934592",
        "local_y_raw": "1095216660480"
      }
    },
    {
      "id": "GV-003-PHILOX4X32_10_ORIGIN",
      "description": "Philox4x32-10 全零计数器与全零密钥下的 10 轮标准输出向量 (与 D.E. Shaw Random123 官方标准严格一致)",
      "inputs": {
        "ctr_u32x4": [0, 0, 0, 0],
        "key_u32x2": [0, 0]
      },
      "expected": {
        "out_u32x4": [1717053460, 2979605061, 2685360627, 3691852427]
      }
    },
    {
      "id": "GV-004-Q8_8_LIF_STEP",
      "description": "单步 Q8.8 LIF 神经元积分、去甲肾上腺素阈值调制与脉冲触发验证",
      "inputs": {
        "v_prev_q8": 180,
        "leak_beta_q8": 230,
        "syn_in_q8": 96,
        "m_ne_q8": 128
      },
      "expected": {
        "theta_eff_q8": 218,
        "v_candidate_q8": 257,
        "spiked": true,
        "v_after_reset_q8": 0,
        "refractory_ticks_set": 2
      }
    }
  ]
}
```
