# Rynvek 项目文档：项目全貌、演进前因后果、游戏设计圣经与全量冻结架构决策总册（v1.0）

> **文档定位**：本文档为 Rynvek 项目的 **「项目全貌与决策总册（Project Overview, Rationale & Frozen ADR Bible）」**，与姊妹篇 **《Rynvek 开发规范文档（`Rynvek_Development_Specification_v1.0.md`）》** 共同构成交付给 AI Coding Agent 及研发团队的完整工程基线。
> **阅读与执行契约**：
>
> 1. **本文档（项目文档）** 负责完整阐述项目的起源背景、66 轮设计演进的前因后果、两轮共 28 项深度架构审查与遗漏补全结论、8 大产品矩阵、L1–L4 四层神经生态原理、完整的 `Rynvek Arena` 游戏设计圣经与数值平衡体系、视觉与音频艺术方向、AGPL-3.0 法律合规边界，以及全部 732+ 项冻结架构决策（ADR Register）与 M0–M8 研发路线图。
>
> 2. **姊妹篇（开发规范文档）** 负责提供 100% 零歧义的代码级实现蓝图（含 Monorepo 配置文件、24 个 Rust Crates 与 14 个 TS Packages 精确结构体/接口/着色器源码、二进制文件字节偏移、PostgreSQL/SQLite DDL、REST/WebTransport 路由表、错误码全表、Web Audio 合成器代码与跨语言 Golden Test Vectors）。
>
> 3. **Agent 自治铁律**：两份文档已封闭全部设计自由度，AI Coding Agent 在开发过程中应严格遵循两份文档中的冻结决策与精确参数，无需进行任何额外的架构设计或数值猜测。

## 第一章：项目起源、核心愿景与 66 轮设计演进前因后果

### 1.1 起源背景：从「百万级神经网络自然选择（Neural Soup）」到原生策略游戏

Rynvek 的起点源自对人工生命（Artificial Life）与开放式神经演化（Open-ended Neuroevolution）前沿研究 —— **Neural Soup（神经汤）** 的深度剖析与工程化延展：

1. **Neural Soup 的核心启示**：

   * 在传统强化学习或游戏 AI 中，神经网络通常作为外部训练好的静态控制器挂载在预设单位上，环境仅是被动背景，且往往依赖人工设计的显式奖励函数（Fitness Function）。

   * 而在 **Neural Soup** 范式中，数十万至上百万个微型独立神经网络（合计可达百亿级有效突触参数吞吐）共同栖息在一个支持反应-扩散与化学信号传播的 **共享连续环境场（Shared Field Tensor）** 中。

   * 每个粒子/细胞拥有独立的能量池（Energy）、化学物质合成/分解规则（Chemistry）以及可变异的神经网络基因（Genome）。系统 **不设任何人工显式适应度函数**，仅依靠严格的热力学能量守恒：能够感知局部营养梯度、释放/解读同伴化学信号、协同捕食或防御并维持能量正平衡的个体得以存活并触发有丝分裂（Mitosis），在复制过程中对突触权重与网络拓扑施加随机突变；能量耗尽的个体则死亡分解，其残骸重新化为环境场中的基础营养物。

   * 实验证明，仅凭“共享连续场 + 局部神经计算 + 能量代谢守恒 + 自然选择”，系统即可自发涌现出三层宏观秩序：**第一层自组织（趋化聚集、涡旋巡游）→ 第二层演化适应（捕食者-猎物军备竞赛、投毒与抗性）→ 第三层多细胞式协作（外层防御刺胞、内层储能核心、通道信号中继）**。

2. **核心痛点与 Rynvek 的破局定位**：

   * 单纯的人工生命模拟（如 Conway's Game of Life、Lenia、Particle Life）虽然具有极强的科学观赏性，但缺乏人类玩家与高层 AI Agent 的战略博弈目标，容易停留在“屏幕保护程序式”的被动观察；而传统 RTS 游戏（如《StarCraft》《Command & Conquer》或可编程 RTS《Screeps》）的单位行为完全由硬编码状态机或确定性脚本驱动，缺乏底层生物演化的涌现惊喜。

   * **Rynvek 的核心创见**：将 **Neural Soup 作为整个游戏世界的物理与生物计算底层（Substrate）**，在其上构建支持 **人类玩家、确定性脚本、强化学习模型（RL Policy）与大语言模型智能体（LLM Agent）同台竞技与协作的神经网络原生策略游戏（Neural-Native RTS）与开放涌现世界平台**。

   * 在 Rynvek 中，**神经网络不是 AI 插件，而是核心游戏机制本身**。玩家在宏观上像《Mindustry》一样建造模块化工业与生物神经设施、管理资源与能量通量管网；在中观上通过发射引导信息素场、广播四维神经调质（Neuromodulators）与运行演化坩埚（Mutation Crucible）来“牧养、调制和繁育”成千上万具有自主神经网络的生命体群落，支持公平的 PvP 竞技与自适应演化的 PvE 生存挑战。

### 1.2 历史设计演进的六个关键阶段（前因后果全景）

为了理解为什么 Rynvek 最终锁定了当前的架构，以下完整梳理项目在历史讨论中经历的六次关键架构跃迁：

| 演进阶段 | 对应历史轮次 | 核心议题与关键架构转折点 | 为什么做出该转折（前因后果与取舍理由） | 
| ----- | ----- | ----- | ----- | 
| **阶段一：神经原生 RTS 四层生态立项** | Turn 01 – Turn 08 | 确立 `L1 (生态物理场) → L2 (神经原生生命体) → L3 (战略调制层) → L4 (人类/Agent/脚本统一 API)` 四层架构；决定“先做 Arena 竞技闭环，再扩展 Persistent World”。 | 若允许人类或脚本在每毫秒直接改写百万个细胞的每一个神经元权重，不仅网络带宽与操作复杂度爆炸，还会彻底杀死底层人工生命的涌现感。因此发明了 **L3 战略调制层**（通过环境场注入、神经调质广播与精英基因筛选间接引导群体），并强制人类与所有 AI 共享同一套速率受限的 L4 控制契约。 | 
| **阶段二：从单一游戏升级为开放规则引擎** | Turn 09 – Turn 11 | 借鉴康威生命游戏多变体与《Factorio》Mod 生态，确立 **稳定内核 `K0–K3` + 可替换世界规则层 `W0–W7` + 扩展包生态 `X`** 的解耦体系。 | 如果把物理、化学或某一种神经元模型写死在引擎底层，项目就只能玩一种规则。将类型/能力系统（K0）、执行后端（K1）、混合存储（K2）、调度编译（K3）固化为内核，而把空间拓扑（W0）、场动力学（W1）、神经计算（W2）、演化遗传（W3）、多细胞组织（W4）、控制协议（W5）、游戏模式（W6）、可视化（W7）全部模块化，使同一引擎既能跑 Conway/Lenia，也能跑硬核竞技 RTS。 | 
| **阶段三：Web-First 技术栈选型与放弃 Bevy** | Turn 12 – Turn 19 | 放弃当时尚未稳定且 WebGPU/多世界自定义调度受限的通用游戏引擎 Bevy，正式锁定 **Rust 2024 + `wgpu 30.0.1` + WebGPU/WGSL + React/TypeScript + WebAssembly** 自研引擎路线。 | 项目要求 Web 端开箱即玩（零安装门槛），同时在百万级神经计算与大规模实例渲染上榨干硬件性能，且服务端需要运行完全相同代码的 Headless 权威仿真。直接使用 Rust + `wgpu` 构建专用 Runtime，可同时编译为浏览器 WASM+WebGPU、桌面 Native（Vulkan/Metal/DX12）与 Linux 权威服务端。 | 
| **阶段四：规则语言务实转向与深层架构审计** | Turn 20 – Turn 49 | 1\. 冻结自有代码全部采用 **`AGPL-3.0-only`**（Turn 20）；  2\. **否定自研文本 DSL**，改用开发者熟悉的 **TypeScript（编写规则与声明式 GPU 算子）+ Python（科研与 AI 训练）+ WIT/WASM（原生插件）**（Turn 30）；  3\. 完成 `v0.4` 至 `v1.7`（`E001–E040`, `D041–D592`）共十轮深水区工程设计。 | 重新发明一门新语法会让 IDE 补全、类型检查和社区学习成本变得极高。复用 TypeScript AST/Builder 生成内部 `Typed SSA Rule IR (.rir)` 再降级到 WGSL 和 CPU SIMD，既保留了主流语言生态，又实现了 GPU 安全加速。 | 
| **阶段五：视觉美术（shadcn × Mindustry）与工业电子音频定型** | Turn 50 – Turn 55 | 1\. 确立应用层 UI 采用自建 **shadcn/ui 风格（Neutral Dark + Industrial Lime `--rv-*`）**，世界画面采用 **Mindustry 风格俯视角几何工业像素 + 多尺度神经科学可视化**（`D593–D616`）；  2\. 确立轻快工业电子与有机演化结合的自适应多轨音乐，前期全部采用 **Web Audio 程序化占位符合成器**（`D617–D637`）；  3\. 完成首轮终局审计（`D638–D675`）。 | 传统奇幻/写实 RTS 的重度贴图与复杂光影会掩盖底层神经活动和连续场信息流；采用 Mindustry 式的清晰几何轮廓与 shadcn 式的高信噪比暗色工具界面，能让玩家一眼看清资源链、场梯度与神经脉冲同步态。 | 
| **阶段六：全球商标重名排查与 Rynvek 品牌终局迁移** | Turn 56 – Turn 66 | 排查发现原暂定名 `Neuroverse` 在美国已存在注册商标且有多个同名 AI/脑科学项目，候选名 `Zivren` 亦存在商业电商占用；最终选定具备高独占性与硬朗工业计算质感的原创主品牌 **`Rynvek`**（`ADR-BRAND-001`），并在 Turn 66 将核心引擎正式定名为 **`Rynvek Engine`**，视觉包扩展名定为 **`.rview`**。 | 保留 `Neural Soup`、`ECS`、`World` 等通用学术与技术术语不变，将所有产品品牌、Crate (`rynvek-*`)、NPM Scope (`@rynvek/*`)、Python 包 (`rynvek`)、CLI (`rynvek`)、环境变量 (`RYNVEK_*`) 与文件扩展名 (`.rworld`, `.rpack`, `.rview`, `.rsnap`, `.rjournal`, `.rreplay`) 彻底统一至 Rynvek 命名空间。 | 

## 第二章：两轮全量架构深度审查与 28 项遗漏/冲突终局裁决

面向「直接交由 AI Coding Agent 零决策自主实现」的最高标准，我们在全面梳理 66 轮历史记录的基础上执行了 **两轮深度架构审计**：

* **第一轮审计（`GAP-01` 至 `GAP-18`）**：重点解决历史讨论中明确留待“未来做完 Spike 原型再二选一”的架构悬留项、跨轮次演进造成的数据结构同名异构冲突、以及 T66 更名后的全栈命名对齐。

* **第二轮审计（`GAP-19` 至 `GAP-28`）**：站在 AI Coding Agent 逐文件编写代码的角度，全面排查在实现地图生成、科技演化树、PvE 虫潮、数据库建表、服务端 REST/RPC 接口、错误码处理、编辑器实时补丁、LLM 工具调用与依赖锁定迁移时可能遇到的任何规范空白，并依据行业最佳实践予以 100% 封闭。

### 2.1 第一轮审计：18 项核心架构冲突与原型悬留项终局裁决（`GAP-01` – `GAP-18`）

| 编号 | 历史遗留问题 / 冲突点 | 终局冻结工程决策（Normative Resolution） | 
| ----- | ----- | ----- | 
| **GAP-01** | T66 更名 `Rynvek Engine` 与 `.rview` 后，早期 Crate 名、WIT 包名、CLI 命令、二进制魔数与 CSS 变量未同步。 | 全局统一为 `rynvek-engine` Crate、`rynvek` CLI、`package rynvek:plugin@1.0.0;`、魔数 `RYWL`/`RYPK`/`RYVW`/`RYSS`/`RYJL`/`RYRP` 及 `--rv-*` CSS 变量前缀。 | 
| **GAP-02** | T54 §1 遗留“由 Rust/wgpu 独占 `GPUDevice` 还是由浏览器 TS 层拥有 Device，建议先做 Spike”。 | **冻结为 Dedicated Engine Worker (`rynvek-wasm` + `rynvek-wgpu`) 独占 `wgpu::Device` 与 `wgpu::Queue`**。React 主线程仅通过 `canvas.transferControlToOffscreen()` 移交画布，严禁在 TS 层创建第二套 GPU 上下文。 | 
| **GAP-03** | 确定性等级（4 级 vs 3 级）冲突，且浮点 GPU 神经汤与 PvP 严格确定性存在矛盾。 | **冻结 D0–D3 四级确定性规范 + 双层混合权威执行契约**：宏观 RTS 权威层强制运行于 `D0`（`Q32.32` 64 位定点算术）；微观神经汤在 PvP 下运行于 `INT16 Q8.8` / `INT8 Q1.7` 定点量化模式（或由权威服务端统一计算），每 `4 Ticks`（1 个 Coupling Epoch）通过确定性量化归约桥向宏观层提交 6 维动作读出。 | 
| **GAP-04** | `EntityId`、`EntityHandle`、`FieldDescriptor`、`PhaseDescriptor`、`ConflictPolicy` 在不同轮次定义了多个不兼容版本。 | **冻结唯一标准内存布局**：`EntityId(u128)`（高 64 位谱系前缀 + 低 64 位单调序号）、`EntityHandle { slot: u32, generation: u32 }`、`GpuEntityRef { dense_index: u32, generation: u32 }`，以及 8 种策略的 `ConflictPolicy` 枚举。 | 
| **GAP-05** | Tick 内部阶段数量在 T26（12 阶段）、T40（15 阶段）、T46（动态 DAG）之间不一致。 | **冻结为 16 阶段标准拓扑信封（`Phase 00`–`Phase 15`）**：其中 `00, 01, 02, 13, 14, 15` 为不可变内核信封阶段，`03–12` 为可按 WorldProfile 裁剪或同级并发调度的标准领域阶段。 | 
| **GAP-06** | TypeScript 规则执行引擎在 Deno、V8、QuickJS 与 WGSL 编译器之间边界模糊。 | **冻结三层规则执行架构**：Tier-0 采用 `@rynvek/rules` 声明式 AST Builder 生成 36 条指令的 `Typed SSA Rule IR (.rir)` 并转译为 WGSL 与 CPU SIMD；Tier-1 采用编译进 WASM 的 **QuickJS-NG 确定性沙箱**（单 Tick 限额 `500,000 Gas`）；Tier-2 采用 Wasmtime WIT 组件插件。 | 
| **GAP-07** | T54 指出 D639（游戏设计圣经）与 D640（数值平衡规范）仅有目录而无具体数值。 | 在本文档第五章完整冻结 **3 种战略资源、5 层连续环境场、8 种工业建筑、6 种神经原生单位、L3 四维调质方程与战斗克制公式** 的全部精确数值。 | 
| **GAP-08** | T54 §3.1 指出神经网络图存储需支持多种拓扑但未定具体物理布局。 | 冻结 `NeuralGraphStorage` 下辖三种标准布局：**`DenseTiledMatrix`（**$N \le 64$**）**、**`BlockSparseCSR`（**$64 < N \le 4096$ **慢变拓扑）** 与 **`PagedDynamicAdjacency`（每神经元 32 槽位动态突触页）**。 | 
| **GAP-09** | T54 §10 指出普通 ECS 快照无法保存脉冲延迟队列与稀疏突触状态。 | 冻结 `.rsnap` 快照中的专用 **`NEUR` 二进制分块（Tag `"NEUR"`）**，使用 64-bit `spike_history_ring` 位图替代庞大脉冲队列，并对后天可塑性偏移 $\delta w_{ji}$ 采用零游程编码 + Zstd 压缩。 | 
| **GAP-10** | T48 默认跨分区每 Tick 使用 2PC 两阶段提交，T54 §3 指出这会严重阻塞帧率。 | **废除每 Tick 全局 2PC，冻结三级分区一致性协议**：每 Tick 仅做 `16.0 Units` Halo 镜像异步交换；跨界移动通过带 `FencingToken` 与 `2.0 Units` 滞回区的 `EntityHandoff` 在 `Tick + 1` 原子生效；Bounded 2PC 仅用于低频管理事务。 | 
| **GAP-11** | T54 §3.8 指出大规模 RTS 单位寻路与局部避障尚未选定确定性算法组合。 | **冻结三级确定性导航架构**：宏观 `16×16` 扇区 **HPA\*** + 中观多单位共享 **Eikonal 向量流场** + 微观 `Q32.32` 定点 **2-Iteration ORCA 互惠避障**。 | 
| **GAP-12** | 战争迷雾（Fog of War）服务端安全裁剪与已探索区域记忆显示机制未具体化。 | **冻结服务端 `VisibilityGrid` 物理裁剪协议**：服务端按 `64×64` 视野网格执行对称阴影投射，非可见区敌方移动单位在网络序列化阶段直接剔除（零字节下发），离开视野时仅下发 `EntityVisibilityLost` 静态幽灵快照。 | 
| **GAP-13** | T52 决定前期音效全部使用程序化占位符，但缺少每个音效的具体合成参数。 | 冻结 **24 个标准语义音频事件的 Web Audio 合成器精确 DSP 参数表**（波形、起止频率、ADSR 包络毫秒值、滤波器类型与 Q 值）。 | 
| **GAP-14** | UI 主题、世界主题、阵营色与无障碍主题的覆盖关系及完整色板未完全锁定。 | 冻结 **五层主题隔离架构**（Accessibility > Visualization > Faction > World Biome > Application UI）及完整的 `--rv-*` Neutral Dark / Scientific Light / Okabe-Ito 色盲安全令牌表。 | 
| **GAP-15** | 核心代码采用 `AGPL-3.0-only` 时，需防止第三方创作者的规则脚本与 Agent 被传染。 | **冻结双层许可证矩阵**：引擎与应用采用 `AGPL-3.0-only`；公开协议（`protocols/*`）与客户端 SDK（`@rynvek/protocol`, `@rynvek/rules`, `@rynvek/agent-sdk`, Python `rynvek`）采用 **`Apache-2.0 WITH LLVM-exception`** 并附接口豁免声明。 | 
| **GAP-16** | 序列化格式在 JSON、TOML、Protobuf、rkyv 与自定义二进制之间职责重叠。 | **冻结分层序列化标准**：源码清单用 `TOML 1.0`、构建签名清单用 `Canonical JSON (RFC 8785)`、网络控制信令用 `Protobuf v3`、高频状态 Delta 用 `16B 对齐列式二进制 + LZ4`、快照与归档用 `64B Header + rkyv/SoA Chunk + Zstd + BLAKE3`。 | 
| **GAP-17** | T54 (D669) 要求建立三档硬件性能预算，但未给出具体量化数字。 | 冻结 **Minimum / Recommended / High-End 三档硬件量化性能预算表** 与 **6 级自动优雅降级状态机**。 | 
| **GAP-18** | 732+ 项历史决策散落各轮，缺乏统一索引与演进替代状态说明。 | 在本文档第八章建立完整的 **Rynvek 全量架构决策统一注册表（Unified ADR Register）**。 | 

### 2.2 第二轮审计：面向 AI Coding Agent 零决策实现的 10 项深层工程补全（`GAP-19` – `GAP-28`）

| 编号 | 二次深层审查发现的 Agent 实现空白点 | 最佳实践终局补全方案（已完整写入本项目文档与开发规范文档） | 
| ----- | ----- | ----- | 
| **GAP-19** | **地图程序化生成、5 种地形参数与开局初始状态（Starter Kit & Seed Genomes）未定义**：Agent 实现 `Rynvek Arena` 时不知地图如何生成、有哪些地形、开局给多少建筑/单位/资源。 | 在本文档 **5.1 节与 5.7 节** 完整定义：4 种标准地图尺寸、5 种地形 Tile（通行率、导电性、营养再生率、热液加成、视野阻挡）、**180° 中心旋转对称确定性地图生成算法**、开局标准资产配额（`1 Nexus + 3 Foragers + 1 Striker`，`500 Biomass / 300 Flux / 100 Neurogel`）及 6 大初始单位 `SeedGenome` 的精确超参数。 | 
| **GAP-20** | **科技与演化升级树（Tech & Evolution Tree）缺少具体节点数据**：Agent 无法凭空编写科技升级面板与属性加成逻辑。 | 在本文档 **5.6 节** 完整冻结 **三阶工业-生物协同演化树（Tier 1–3 共 9 个标准科技/演化节点）**，明确每个节点的 ID、前置建筑、资源消耗、研发 Tick 数及对单位/建筑/脑容量上限的精确数学修正。 | 
| **GAP-21** | **PvE 自适应虫潮生成方程与 PvP 超时积分裁决公式缺失**：Agent 实现比赛结算与 PvE 模式时缺乏数学公式。 | 在本文档 **5.7 节** 冻结 **PvE 自适应自然选择波次公式**（每 1,800 Ticks 一波，波次预算 $B_w = \lfloor 150 \cdot w^{1.35} \rfloor$，且由上一波存活/高伤害前 10% 基因变异繁衍）及 **PvP 36,000 Ticks 超时 Tie-Breaker 积分公式**。 | 
| **GAP-22** | **服务端 PostgreSQL 17 与客户端 SQLite / OPFS 缺少精确 `CREATE TABLE` DDL**：Agent 实现存储层时会自行猜测表名和字段类型。 | 在《开发规范文档》**规范 11.3** 完整给出 **PostgreSQL 17 服务端 10 张核心表与 SQLite 本地 4 张核心表的完整 SQL DDL 建表语句与索引定义**。 | 
| **GAP-23** | **`Rynvek Server` 与 `Rynvek Hub` 缺少完整 HTTP REST / WebTransport 路由表与 Token 二进制结构**。 | 在《开发规范文档》**规范 11.4** 完整给出 **22 个标准服务端 API 端点路由、请求/响应负载结构与 96 字节 `CapabilityToken` 二进制签名结构**。 | 
| **GAP-24** | **跨语言错误码注册表（`RynvekErrorCode`）未枚举具体数值与恢复策略**。 | 在《开发规范文档》**规范 11.5** 完整定义 **`0x1000`–`0x9FFF` 九大域标准错误码枚举、触发条件与强制故障恢复动作（`ErrorRecoveryAction`）**。 | 
| **GAP-25** | **`Rynvek Studio` 创作态补丁（`WorldPatch`）、CRDT 协作与热更新安全分级（`LiveEditSafetyClass`）缺少代码结构**。 | 在《开发规范文档》**规范 10.4** 完整定义 `AuthoringDocument`、`WorldPatchOp`、三方语义合并器规则与四级热更新安全分类。 | 
| **GAP-26** | **LLM Agent 的 `ToolBroker` 受控函数签名与 `AgentEpisodicMemory` 结构未具象化**。 | 在《开发规范文档》**规范 09.2** 完整给出 **LLM `ToolBroker` 的 6 个标准工具 JSON Schema、速率/超时配额及外部记忆归档结构**。 | 
| **GAP-27** | **统一输入抽象层（`InputAbstraction`）的 36 个逻辑动作枚举与四套上下文键位映射未列全**。 | 在《开发规范文档》**规范 12.2** 完整列出 **全部 36 个 `LogicalActionId` 在 RTS、Studio、Neural Editor、Code Editor 下的默认键鼠/触控/手柄映射全表**。 | 
| **GAP-28** | **依赖锁定文件（`dependencies.lock`）格式与 Schema 有向迁移图（`MigrationGraph`）最短路径算法未给出实现**。 | 在《开发规范文档》**规范 10.5** 完整给出 `dependencies.lock` 标准结构与 `MigrationGraph::plan_upgrade_path` 的完整可编译 Rust 算法实现。 | 

## 第三章：Rynvek 产品体系、运行模式与用户生态矩阵

### 3.1 八大核心产品组件与职责边界（D638, ADR-BRAND-001, T64, T66）

```
                                 ┌──────────────────────────────┐
                                 │            RYNVEK            │
                                 │  "Rules. Life. Emergence."   │
                                 └──────────────┬───────────────┘
                 ┌──────────────────────────────┼──────────────────────────────┐
                 ▼                              ▼                              ▼
   ┌───────────────────────────┐  ┌───────────────────────────┐  ┌───────────────────────────┐
   │      End-User Apps        │  │    Core Engine & SDK      │  │   Server & Cloud Infra    │
   ├───────────────────────────┤  ├───────────────────────────┤  ├───────────────────────────┤
   │ • Rynvek Arena (RTS Game) │  │ • Rynvek Engine (Core)    │  │ • Rynvek Server (Host)    │
   │ • Rynvek Studio (IDE)     │  │ • Rynvek SDK (TS/Py/Rust) │  │ • Rynvek Hub (Registry)   │
   │ • Rynvek Lab (Research)   │  │ • `rynvek` CLI Tool       │  │ • Rynvek Network (Fed/ID) │
   └───────────────────────────┘  └───────────────────────────┘  └───────────────────────────┘

```

1. **`Rynvek Engine`**：整个生态的唯一权威仿真、神经计算、连续场求解、规则执行与 WebGPU 渲染内核（由原 `Rynvek Runtime` 正式定名）。它既可编译为 WASM 运行于浏览器 Dedicated Worker 中，也可编译为原生机器码运行于桌面端与无头服务端。

2. **`Rynvek Arena`**：面向玩家与 AI 竞技者的策略游戏应用。提供 Mindustry 风格的网格基地建设、神经原生单位集群指挥、L3 信息素与神经调质控制面板、天梯匹配大厅、实时观战与确定性回放分析。

3. **`Rynvek Studio`**：面向世界创作者与 Mod 开发者的集成创作环境（IDE）。采用可停靠多面板工作区（Dockable Workspace），内置世界地形与场编辑器、神经网络拓扑与脉冲检视器、TypeScript 规则代码编辑器、时间线单步调试器与 `.rview` 视觉包预览器。

4. **`Rynvek Lab`**：面向科研人员与强化学习开发者的实验平台。支持声明式定义多随机种子批量实验、参数网格扫描（Grid Sweep）、消融实验（Ablation Study）、演化谱系树可视化，以及零拷贝导出 Apache Arrow / Parquet 标准数据集。

5. **`Rynvek Server`**：独立部署的权威无头服务端程序，承载持续运行的多分区开放世界（World Host）、竞技对局权威裁决（Match Authority）与反作弊验证。

6. **`Rynvek SDK`**：统一的多语言开发套件，包含 `@rynvek/*` TypeScript 包、Python `rynvek` 科研与 Gymnasium/PettingZoo 接口包、`rynvek:plugin@1.0.0` WIT 原生插件接口及统一命令行工具 `rynvek`。

7. **`Rynvek Hub`**：去中心化友好的内容注册表与分发服务，管理 `.rworld`（世界包）、`.rpack`（规则/扩展包）、`.rview`（视觉包）、Audio Profile 与公开研究数据集的 Ed25519 签名验证、依赖锁定与分发。

8. **`Rynvek Network`**：负责跨节点统一身份认证（`Principal` / `Account`）、全球 Glicko-2 分赛道天梯（Human / Script / Agent Leagues）与跨服务器联邦互联服务。

### 3.2 三维正交运行配置矩阵（`WorldProfile` × `GameMode` × `ExecutionProfile` — D641）

任何世界实例在启动时必须显式指定以下三个正交维度的配置组合，严禁在代码中通过零散的布尔标志（如 `is_pvp`、`is_debug`）硬编码分支逻辑：

| 正交维度 | 标准配置枚举值 | 核心运行约束与启用模块 | 
| ----- | ----- | ----- | 
| **维度一：`WorldProfile`** | `RTS_Standard` | 启用三资源经济、8 类工业建筑、6 类标准神经单位、科技演化树、战争迷雾与 36,000 Ticks 时长上限。 | 
|  | `Sandbox_Creative` | 开放上帝画笔（实时涂抹环境场、注入任意基因组、无建造冷却与无限资源）、支持任意 Tick 暂停/单步/历史分支回退。 | 
|  | `Research_Batch` | 关闭 WebGPU 渲染与 Web Audio，启用无头最大吞吐量推进、周期性谱系快照与 Arrow/Parquet 指标采集。 | 
|  | `Persistent_Ecology` | 开启多分区（Multi-Partition）动态负载、四级多保真度仿真（`LOD0–LOD3`）、7×24 小时自动 WAL 归档与快照轮转。 | 
| **维度二：`GameMode`** | `PvP_Ranked` | 强制 `Server-Authoritative`、强制认证赛季标准基因池与 Ruleset Hash、强制服务端战争迷雾裁剪、强制 `600 APM` 公平限流。 | 
|  | `PvP_Custom` | 允许房间宿主自定义初始资源、地图种子、已签名的社区 `.rpack` 规则包与人类/Agent 混合编队。 | 
|  | `PvE_Survival` | 1–4 名玩家或 Agent 协同建设基地，抵御每 `1,800 Ticks` 演化升级一次的野生神经生物潮（Wild Swarm Waves）。 | 
|  | `Evolution_Benchmark` | 无人工干预或固定基准控制器下的纯自然选择演化对照实验模式。 | 
| **维度三：`ExecutionProfile`** | `D0_Strict` | 宏观状态 100% 采用 `Q32.32` 定点整数运算；微观神经与场采用 `INT16 Q8.8` 定点量化并在 Coupling Epoch 归约，跨平台 0 bit 误差。 | 
|  | `D1_Reproducible` | 固定同款硬件与驱动配置下的确定性 `f32` 执行，适用于受控科研实验与单机回放。 | 
|  | `D2_Bounded` | 允许跨厂商 GPU 高吞吐 `f32`/`f16` 近似计算（单步容差 $\le 10^{-5}$），每 4 Ticks 量化同步。 | 
|  | `D3_Statistical` | 仅要求宏观统计分布一致的极限吞吐模式，用于百万级粒子长期生态演化探索。 | 

## 第四章：核心概念架构：L1–L4 四层神经生态与 K0–K3 / W0–W7 引擎分层

### 4.1 为什么采用「稳定内核 K0–K3 + 可编程世界层 W0–W7」？

为了让 Rynvek 既具备工业级 RTS 的确定性与高性能，又具备康威生命游戏般的无限规则可扩展性，系统严格遵循 **“机制与策略分离（Separation of Mechanism and Policy）”** 原则：

```
┌────────────────────────────────────────────────────────────────────────────┐
│ X: 开放内容与社区扩展生态 (.rworld, .rpack, .rview, Rynvek Hub, Lockfile)  │
├────────────────────────────────────────────────────────────────────────────┤
│ W7: 表现与分析解耦层 (VisualizationProvider, L0-L7 Render, Audio, Lab)     │
│ W6: 游戏模式与结算层 (PvP Matchmaking, PvE Wave Spawner, Victory/TieRule)  │
│ W5: 战略控制协议层 [L4/L3] (Principal, ControlLease, Beacon Modulators)    │
│ W4: 多细胞组织与单位层 [L2+] (Adhesion Bonds, Gap Junctions, Unit Cluster) │
│ W3: 遗传、可塑性与自然选择层 [L2] (Genome, Baldwin Assimilation, Crucible) │
│ W2: 生命体与神经计算层 [L2] (LIF / Izhikevich / CTRNN / GatedMLP, STDP)    │
│ W1: 环境动力学与物理化学层 [L1] (5-Channel PDE Fields, Lenia, Kinematics)  │
│ W0: 空间拓扑与多速率时间层 [L1] (Lattice2D/Continuous2D, Chunk+Q32.32, Clock)│
├════════════════════════════════════════════════════════════════════════════┤
│ K3: 规则编译与 16 阶段 Tick 调度内核 (Typed SSA Rule IR, Phase DAG, Commit)│
│ K2: 混合列式状态存储与持久化内核 (3-Tier EntityId, Paged SoA, .rsnap/WAL)  │
│ K1: 异构计算与硬件抽象内核 (CPU Reference + Dedicated Worker wgpu Backend) │
│ K0: 基础类型、量纲、确定性 RNG (Philox4x32-10) 与 Capability 安全内核      │
└────────────────────────────────────────────────────────────────────────────┘

```

**核心架构铁律**：

1. **单向依赖不可逆**：上层模块（`W*`）只能通过下层暴露的受控 Trait 与 Schema 注册表调用内核（`K0–K3`），内核绝不硬编码任何特定游戏单位或特定化学场的名字。

2. **表現层绝对只读**：`W7`（WebGPU 渲染器、UI Inspector、Web Audio 音频运行时）仅消费 `Phase 15` 发布的只读状态快照与派生事件，**在物理内存与类型签名上完全没有修改世界权威状态的权限**。

## 第五章：Rynvek Arena 游戏设计圣经与完整数值平衡规范（Game Design Bible & Balance Spec）

### 5.1 核心玩法循环、开局初始状态（Starter Kit）与 6 大预置种子基因组（补全 GAP-19, D639）

#### 5.1.1 核心玩法循环（Core Gameplay Loop）

`Rynvek Arena` 的一局标准对战分为四个紧密咬合的阶段循环：

1. **基地锚定与环境汲取（0:00 – 3:00）**：玩家围绕初始 `Nexus Core` 建造 `Biomass Harvester`（汲取富营养沼泽 `NutrientMarsh` 中的生物质）与 `Flux Reactor`（建立在热液裂隙 `AcidicFissure` 上获取额外 `+50%` 通量能加成），通过 `Spawning Vat` 孵化初始 `Forager` 工蜂簇采集远端资源并建立早期 `Striker` 防线。

2. **神经精炼与信号场布局（3:00 – 8:00）**：建造 `Synapse Refinery` 将生物质与通量能精炼为高阶 `Neurogel`；在前线与资源走廊部署 `Signal Beacon`（神经调制信标），向环境中发射 `Signal A`（趋化引导场）与 `Signal B`（战斗激发场），并通过广播四维神经调质 $\mathbf{m} = [\text{DA}, \text{5-HT}, \text{NE}, \text{ACh}]$ 调控群落行为倾向。

3. **协同演化与多兵种合成（8:00 – 15:00）**：建造 `Mutation Crucible`（演化坩埚），研发三阶生物-工业科技（解锁 `Carapace` 重甲盾簇、`Spitter` 腐蚀孢子簇、`Weaver` 突触编织者），并提取战场高适应度存活个体的基因执行定向交叉与突变，培育适应当前战局（如高抗腐、高脉冲同步或高机动偷袭）的专属强化菌株。

4. **母体决战与主脑攻坚（15:00 – 20:00）**：孵化旗舰级 `Leviathan`（巨型神经母体），利用移动调制光环与现场分裂能力配合大军突破敌方 `Pulse Turret` 防线，摧毁敌方 `Nexus Core` 取得胜利（或通过 20 分钟超时积分裁决获胜）。

#### 5.1.2 标准开局初始配额（Starter Kit）与 6 大预置种子基因组（Standard Seed Genome Pool）

在 `PvP_Ranked` 与标准 `PvE_Survival` 开局第 `0` Tick，每个阵营的出生点（Spawn Anchor）固定初始化以下资产与资源，确保开局零随机不公：

* **初始资源账本**：`Biomass = 500.0` (`500,000 milli`), `Flux = 300.0` (`300,000 milli`), `Neurogel = 100.0` (`100,000 milli`)。

* **初始实体**：

  * `1 × bld.nexus_core`（位于阵营出生锚点中心 $[G_x, G_y]$，占地 $4 \times 4$ Tiles）；

  * `3 × unit.forager`（生成于主脑核心正前方距离 $3.5\text{ Units}$ 的扇形弧线上）；

  * `1 × unit.striker`（生成于主脑核心外侧巡逻位，距离 $4.5\text{ Units}$）。

* **赛季认证初始种子基因池（6 大标准 `SeedGenome` 参数表）**：

| 种子基因 ID (`genome_slug`) | 绑定单位类型 | 神经元模型 (`NeuronModel`) | 拓扑结构 ($N_{\text{in}}\text{-}N_{\text{hid}}\text{-}N_{\text{out}}$) | 初始非零突触数 ($E_0$) | 默认先天行为偏置（通过初始权重矩阵 $\mathbf{W}_0$ 固化） | 
| ----- | ----- | ----- | ----- | ----- | ----- | 
| `seed.forager.v1` | `unit.forager` | `GatedMLP` | `16 - 24 - 6` ($N=46$) | `216` (`DenseTiled`) | 强正权重连接 $\nabla F_{\text{nutrient}} \to \mathbf{f}_{\text{move}}$ 及 $a_{\text{harvest}}$；强负权重规避 $F_{\text{hazard}}$ 与敌方近邻。 | 
| `seed.striker.v1` | `unit.striker` | `LIF` (SNN) | `16 - 32 - 6` ($N=54$) | `288` (`BlockSparse`) | 对最近敌方相对向量与 $F_{\text{signal\_b}}$ 高增益响应，当敌方进入射程 $6.5\text{ Units}$ 时高频触发 $a_{\text{attack}}$。 | 
| `seed.carapace.v1` | `unit.carapace` | `CTRNN` | `16 - 32 - 6` ($N=54$) | `320` (`DenseTiled`) | 慢时间常数（$\tau \in [4, 12]$）高稳定性状态；强趋向友方群落质心前方阻挡敌火，高 $a_{\text{defense}}$ 护盾再生偏置。 | 
| `seed.spitter.v1` | `unit.spitter` | `LIF` (SNN) | `16 - 40 - 6` ($N=62$) | `360` (`BlockSparse`) | 保持与敌方距离在 $[7.5, 11.0]\text{ Units}$ 的风筝（Kiting）环路权重；优先攻击静止高体积目标（建筑与重甲）。 | 
| `seed.weaver.v1` | `unit.weaver` | `CTRNN` | `16 - 48 - 6` ($N=70$) | `448` (`BlockSparse`) | 内置双耦合振荡环路（Kuramoto 同步种子），自动跟随受损友方簇并激活修复与相位同步输出 $a_{\text{emit}}$。 | 
| `seed.leviathan.v1` | `unit.leviathan` | `Izhikevich` | `16 - 106 - 6` ($N=128$) | `1024` (`PagedDynamic`) | 多簇模块化脑：子簇 A 负责主炮集火，子簇 B 负责护盾与局部调质广播，子簇 C 在富营养区触发现场增殖分裂。 | 

### 5.2 三大核心战略资源与五层连续环境场规范（D639, D640）

#### 5.2.1 三大战略资源（`Biomass`, `Flux`, `Neurogel`）

| 资源 ID | 名称 | 内部精度单位 | 视觉色值 | 产出途径与消耗守恒机制 | 
| ----- | ----- | ----- | ----- | ----- | 
| `res.biomass` | **生物质（Biomass）** | `i64 milli-unit` | `#82B96B` | 由 `Biomass Harvester` 或 `Forager` 从 `field.nutrient` 按 `1:1` 守恒抽取；单位死亡时体内剩余能量的 `75%` 重新化为死亡网格的 `field.nutrient`。用于一切建造与基础躯干孵化。 | 
| `res.flux` | **通量能（Flux）** | `i64 milli-unit` | `#A3D65C` | 由 `Nexus Core` (`+30/s`) 与 `Flux Reactor` (`+60/s`，裂隙上 `+90/s`) 持续产出，受阵营总电容上限约束。用于建筑运转、信标调制广播与单位孵化电力。 | 
| `res.neurogel` | **神经胶质（Neurogel）** | `i64 milli-unit` | `#5CE1E6` | 由 `Synapse Refinery` 每 `60 Ticks` (2.0s) 消耗 `10 Biomass + 50 Flux` 合成 `5 Neurogel`。用于孵化高阶神经单位、研发科技与运行 `Mutation Crucible`。 | 

#### 5.2.2 五层标准连续环境场（`256 × 256` Cells/Chunk, 10Hz 更新）

| 通道 | 场标识符 | 定点类型与量程 | 扩散率 $D$ | 衰减率 $\lambda$ | 生态物理与战略控制作用 | 
| ----- | ----- | ----- | ----- | ----- | ----- | 
| `Ch 0` | `field.nutrient` | `Q8.8` `[0.0, 100.0]` | `0.020` | `0.000` | 生态营养基底；在 `NutrientMarsh` 地形上以 `+0.05/Tick` 自然再生（上限 `80.0`）。 | 
| `Ch 1` | `field.energy` | `Q8.8` `[0.0, 100.0]` | `0.150` | `0.010` | 反应堆辐射场；处于 $F_{\text{energy}} \ge 20.0$ 网格的友方单位内部能量恢复 `+25%`。 | 
| `Ch 2` | `field.signal_a` | `Q8.8` `[-50.0, +50.0]` | `0.250` | `0.050` | 趋化引导场 A（正值吸引群落沿梯度 $\nabla S_A$ 集结或迁移，负值形成禁行排斥区）。 | 
| `Ch 3` | `field.signal_b` | `Q8.8` `[-50.0, +50.0]` | `0.250` | `0.050` | 战斗激发场 B（正值提升区域内友方单位攻击欲望与射速偏置，负值强制脱离接触撤退）。 | 
| `Ch 4` | `field.hazard` | `Q8.8` `[0.0, 100.0]` | `0.050` | `0.005` | 腐蚀/熵增危害场；每 Tick 对场内非腐蚀抗性实体造成 $0.08 \times F_{\text{hazard}}$ 点真实伤害。 | 

### 5.3 八种工业网格建筑与六种神经原生单位完整平衡表（D640）

#### 5.3.1 八种工业建筑平衡表

| 建筑 ID | 名称 | 尺寸 | 造价 (`Biomass`/`Neurogel`) | 建造耗时 (Ticks) | `HP` / `Armor` | `Flux` 收支 (/s) | 核心产出、容量与战术参数 | 
| ----- | ----- | ----- | ----- | ----- | ----- | ----- | ----- | 
| `bld.nexus_core` | 神经主脑核心 | `4×4` | 开局自带 | `0` | `4000` / `15` | `+30.0` | 提供存储上限 `2000/1000/500`；视野 `24 Tiles`；自带脉冲自卫炮（射程 `10`，伤害 `20`，冷却 `15 Ticks`）。 | 
| `bld.biomass_harvester` | 生物质萃取器 | `2×2` | `120` / `0` | `150` (5s) | `450` / `3` | `-6.0` | 每 Tick 从覆盖的 $2\times 2$ 网格抽取最多 `0.4` 营养转化为 `Biomass`（最高 `+12.0 Biomass/s`）。 | 
| `bld.flux_reactor` | 通量反应堆 | `3×3` | `200` / `20` | `240` (8s) | `750` / `5` | `+60.0` | 消耗 `1.5 Biomass/s` 发电 `+60 Flux/s`（在 `AcidicFissure` 热液裂隙上发电 `+90 Flux/s`），增加电容上限 `+500`。 | 
| `bld.synapse_refinery` | 突触精炼厂 | `3×3` | `250` / `0` | `300` (10s) | `800` / `5` | `-25.0` | 每 `60 Ticks` 消耗 `10 Biomass + 50 Flux` 产出 `5.0 Neurogel`（即 `+2.5 Neurogel/s`），增加胶质上限 `+250`。 | 
| `bld.spawning_vat` | 神经孵化槽 | `3×3` | `180` / `40` | `210` (7s) | `900` / `6` | `-15.0` | 按选定 `GenomeId` 孵化单位；生产队列上限 `5`；每座孵化槽提供阵营神经人口容量上限 `+24 Supply`。 | 
| `bld.signal_beacon` | 神经调制信标 | `2×2` | `100` / `30` | `120` (4s) | `400` / `2` | `-12.0` | 作用半径 `16 Tiles`；持续注入 `signal_a` 或 `signal_b` 并广播四维调质向量 $\mathbf{m} \in [-1, 1]^4$；提供反隐脉冲侦测半径 `14 Tiles`。 | 
| `bld.pulse_turret` | 脉冲防御塔 | `2×2` | `150` / `15` | `150` (5s) | `650` / `8` | `-10.0` | 射程 `12 Tiles`，冷却 `15 Ticks` (2.0 发/秒)，单发消耗 `4 Flux`，造成 `28` 点 `Kinetic` 伤害。 | 
| `bld.mutation_crucible` | 演化坩埚 | `3×3` | `300` / `120` | `360` (12s) | `1000` / `8` | `-40.0` | 研发 Tier 2 / Tier 3 演化科技；执行精英基因提取、双亲交叉（Crossover）与受控拓扑突变。 | 

#### 5.3.2 六种神经原生单位集群平衡表

| 单位 ID | 名称 | 人口占用 (`Supply`) | 造价 (`Bio`/`Gel`/`Flux`) | 孵化 Ticks | `HP` / `Shield` / `Armor` | 半径 / 移速 (Units/s) | 视野 (Tiles) | 武器伤害类型 / 单发伤害 / 射程 / 冷却 (Ticks) | 
| ----- | ----- | ----- | ----- | ----- | ----- | ----- | ----- | ----- | 
| `unit.forager` | 采集工蜂簇 | `1` | `40` / `0` / `20` | `90` (3s) | `120` / `0` / `1` | `0.45` / `4.5` | `8` | `Kinetic` / `6` / `1.2` / `20`（采集速率 `4.0 Bio/s`） | 
| `unit.striker` | 突击刺胞簇 | `2` | `65` / `10` / `30` | `120` (4s) | `200` / `0` / `3` | `0.55` / `5.2` | `10` | `Kinetic` / `16` / `6.5` / `12`（高机动游击与集火） | 
| `unit.carapace` | 重甲盾簇 | `4` | `130` / `25` / `50` | `180` (6s) | `620` / `200` / `12` | `0.85` / `2.8` | `9` | `BioElectric` / `24` (前方 $90^\circ$ 扇形 AOE) / `3.0` / `30` | 
| `unit.spitter` | 腐蚀孢子簇 | `3` | `90` / `30` / `40` | `150` (5s) | `160` / `0` / `2` | `0.55` / `3.6` | `12` | `Corrosive` / `22` (+沉积 `15.0 Hazard`) / `11.0` / `30` | 
| `unit.weaver` | 突触编织者 | `4` | `110` / `55` / `60` | `180` (6s) | `240` / `80` / `4` | `0.60` / `4.0` | `11` | 支援光环（半径 `5.0` 友军 `+12 HP/s`，脉冲相位同步命中/闪避 `+15%`） | 
| `unit.leviathan` | 巨型神经母体 | `10` | `350` / `140` / `150` | `360` (12s) | `1500` / `400` / `16` | `1.40` / `2.2` | `15` | `BioElectric` 主炮 / `75` / `13.5` / `45`（自带半径 `8.0` 移动调质光环） | 

### 5.4 三阶工业-生物协同科技与演化树（补全 GAP-20）

所有科技与演化升级均在 `Synapse Refinery`（Tier 1 升级）或 `Mutation Crucible`（Tier 2 & Tier 3 升级）中研发，全局共享一套确定性科技状态机：

| 科技层级 | 科技节点 ID (`tech_id`) | 名称 | 前置条件 | 研发成本 (`Biomass` / `Neurogel` / `Flux`) | 研发耗时 (Ticks @30Hz) | 解锁内容与确定性数值加成 | 
| ----- | ----- | ----- | ----- | ----- | ----- | ----- | 
| **Tier 1** | `tech.chemotaxis_amp` | **趋化受体增敏** | `bld.spawning_vat` | `150` / `30` / `100` | `450` (15s) | `Forager` 与 `Striker` 对 `signal_a/b` 及营养梯度响应灵敏度 `+30%`，`Forager` 采集速率由 `4.0/s` 提升至 `5.2/s`。 | 
| **Tier 1** | `tech.flux_capacitance` | **超导通量电容** | `bld.flux_reactor` | `200` / `40` / `150` | `600` (20s) | 所有 `Flux Reactor` 发电量提升 `+15%`（`+69 Flux/s`），阵营基础通量电容上限增加 `+1000`。 | 
| **Tier 1** | `tech.kinetic_resonance` | **高频脉冲聚焦** | `bld.pulse_turret` | `180` / `50` / `120` | `600` (20s) | `Striker` 与 `Pulse Turret` 的 `Kinetic` 武器基础伤害提升 `+20%`，射程增加 `+1.0 Unit`。 | 
| **Tier 2** | `tech.multicellular_chitin` | **多细胞几丁质装甲** | `bld.mutation_crucible` | `250` / `80` / `200` | `750` (25s) | **解锁 `unit.carapace`（重甲盾簇）与 `unit.spitter`（腐蚀孢子簇）**；所有友方单位基础护甲 `+2`。 | 
| **Tier 2** | `tech.hebbian_plasticity` | **四因子可塑性激活** | `bld.mutation_crucible` | `300` / `100` / `250` | `900` (30s) | 开启战地实时四因子 STDP 学习（$\eta_{\text{learn}}$ 由 `0` 提升至 `0.02`），存活超过 `900 Ticks` 的老兵单位自动获得最高 `+20%` 射速与闪避协同加成。 | 
| **Tier 2** | `tech.gap_junction_sync` | **间隙连接电耦合** | `tech.hebbian_plasticity` | `320` / `120` / `250` | `900` (30s) | **解锁 `unit.weaver`（突触编织者）**；`Signal Beacon` 调制广播半径由 `16 Tiles` 扩大至 `22 Tiles`。 | 
| **Tier 3** | `tech.morphogenesis_apex` | **高阶形态发生学** | `tech.gap_junction_sync` | `500` / `220` / `400` | `1200` (40s) | **解锁 `unit.leviathan`（巨型神经母体）**；单脑最大允许神经元数量上限 $N_{\max}$ 扩充至 `256`，支持动态突触生长页（`PagedDynamicAdjacency`）。 | 
| **Tier 3** | `tech.metabolic_superconduct` | **超导代谢闭环** | `tech.flux_capacitance` + `Crucible` | `400` / `180` / `350` | `1050` (35s) | 神经网络每 Tick 脉冲代谢能耗 $c_{\text{spike}}$ 降低 `-40%`（由 `0.80` 降至 `0.48 milli-Energy`），死亡残骸生物质回收率由 `75%` 提升至 `90%`。 | 
| **Tier 3** | `tech.directed_speciation` | **定向物种跃迁** | `tech.morphogenesis_apex` | `450` / `250` / `400` | `1200` (40s) | `Mutation Crucible` 精英基因繁育耗时缩短 `-50%`，且鲍德温效应后天经验遗传同化率 $\alpha_{\text{baldwin}}$ 由 `0.05` 提升至 `0.15`。 | 

### 5.5 竞技地图程序化生成、PvE 演化虫潮与胜负结算公式（补全 GAP-19, GAP-21）

#### 5.5.1 五种标准地形 Tile 与 180° 中心对称地图生成算法

1. **四种标准地图尺寸**：

   * `Duel_1v1`：`256 × 256 Tiles`（$1 \times 1\text{ Chunk}$）

   * `Skirmish_2v2`：`512 × 512 Tiles`（$2 \times 2\text{ Chunks}$）

   * `Arena_4P`：`768 × 768 Tiles`（$3 \times 3\text{ Chunks}$）

   * `Persistent_Sector`：`2048 × 2048 Tiles`（$8 \times 8\text{ Chunks}$，每 Partition 为 $4 \times 4\text{ Chunks}$）

2. **五种标准地形 Tile 物理属性**：

   * `Tile 0 (OpenSubstrate)`：标准工业基质地面，移速系数 `1.00x`，可自由建造。

   * `Tile 1 (ConductiveBasalt)`：导电玄武岩，移速系数 `1.10x`，其上建造的建筑护甲 `+2`。

   * `Tile 2 (NutrientMarsh)`：富营养有机沼泽，移速系数 `0.85x`，每 Tick 自动再生 `+0.05 field.nutrient`（直至 `80.0`），最适合部署 `Biomass Harvester`。

   * `Tile 3 (AcidicFissure)`：高能热液裂隙，仅允许在上面建造 `Flux Reactor`（发电量加成 `+50%`），普通地面单位踏入每 Tick 受到 `1.5` 点灼烧伤害。

   * `Tile 4 (ImperviousWall)`：高耸玄武岩山体，不可通行、不可建造，且完全阻挡单位视线（Line of Sight）与直线弹道。

3. **180° 中心旋转对称确定性生成算法（PvP 绝对公平保障）**：
   给定 64 位地图种子 `map_seed`，地图生成器仅在左下半场 $y < H/2$（或主对角线一侧）利用多层 `Philox` 柏林/Simplex 网格噪声生成山体走廊、出生点平地、3 处富营养沼泽区与 2 处热液裂隙；随后严格通过中心反演变换 $(x', y') = (W - 1 - x, H - 1 - y)$ 镜像写入右上半场，并运行一次确定性洪水填充（Flood Fill）验证双出生点之间至少存在 2 条宽度 $\ge 6\text{ Tiles}$ 的连通路径。

#### 5.5.2 `PvE_Survival` 自适应自然选择虫潮公式

在 `PvE_Survival` 模式下，地图外围四个边缘裂隙每 `1,800 Ticks`（60 秒）刷新第 $w$ 波野生神经生物潮（$w = 1, 2, \dots, 30$）：

* **波次生物质总预算**：$B_w = \lfloor 150 \cdot w^{1.35} \rfloor$（按预算自动配比生成野生 `Forager/Striker/Carapace/Spitter/Leviathan` 变体）。

* **自然选择跨波次演化**：系统在每波结束时，按个体适应度函数 $F_i = 2.0 \cdot \text{DamageDealtToPlayer}_i + 0.5 \cdot \text{SurvivalTicks}_i$ 对该波所有野生生物排序，选取前 `10%` 精英个体 `Genome` 进行双亲交叉与权重/拓扑突变（$\sigma_{\text{mut}} = 0.08$），作为第 $w+1$ 波的孵化基因池。这意味着如果玩家只造近战盾簇，野生虫潮会在 3–4 波内自发演化出高比例的远程风筝与腐蚀行为！

#### 5.5.3 `PvP_Ranked` 胜负条件与超时积分裁决（Tie-Breaker）方程

1. **即时胜利（Elimination Victory）**：任一阵营的 `bld.nexus_core` 生命值归零（或主动发起 `Surrender` 指令），存活敌对阵营立即获胜。

2. **20 分钟超时积分裁决（`Tick == 36,000` Tie-Breaker）**：若达到最大比赛时长双方主脑均存活，按以下确定性定点数公式计算各阵营综合战略得分 $S_{\text{total}}$，高分者获胜（若分差 $< 1\%$ 则加赛 `3,600 Ticks` 突然死亡模式，期间主脑每秒流失 `20 HP`）：
   

   $$
   S_{\text{total}} = 1000 \cdot \frac{\text{HP}_{\text{nexus}}}{\text{HP}_{\text{nexus\_max}}} + 2.0 \sum_{b \in \text{Buildings}} \text{Cost}(b) + 1.5 \sum_{u \in \text{Units}} \text{Cost}(u) + 0.25 \cdot \text{Biomass}_{\text{harvested\_total}}
   $$

## 第六章：视觉艺术（`shadcn/ui × Mindustry`）与声音设计（工业电子 × 神经声音化）体系

### 6.1 视觉定位：Rynvek Industrial Emergence（工业涌现美学 — D593–D616）

Rynvek 拒绝传统奇幻 RTS 的繁复写实贴图，确立了独树一帜的 **“科学仪器界面（shadcn/ui）× 模块化工业像素世界（Mindustry）× 有机神经脉冲可视化”** 融合美学：

* **两套独立视觉系统**：外层应用工作区（`Rynvek Studio` / `Rynvek Lab` / `Rynvek Arena` 面板）采用 `Geist Sans` + `Geist Mono` 字体、1px 细边框（`#303A34`）、深石墨碳黑背景（`#090B0A`）与工业青柠强调色（`#A3D65C`）；内层 WebGPU 世界画布采用严格的 `32×32` 网格像素、1px 深色轮廓线、左上 $135^\circ$ 统一斜投影与高辨识度几何机械轮廓。

* **L0–L4 五级多尺度神经可视化（随摄像机缩放无缝过渡）**：

  * `L0 (远景全图)`：仅渲染群体活动密度热力云与阵营控制边界；

  * `L1 (中景战区)`：显示多细胞单位簇几何外框、朝向与 Kuramoto 同步脉冲环；

  * `L2 (近景集群)`：渲染个体之间的粘附键与间隙连接电耦合光束；

  * `L3 (微观个体)`：展开选中生命体内部的神经元节点与兴奋/抑制突触图；

  * `L4 (突触检视)`：在 Inspector 浮层实时绘制单条突触的膜电位波形、权重 $w_{ji}$ 与资格迹 $e_{ji}$ 变化曲线。

### 6.2 声音定位：Playful Industrial Electronica × Neural Sonification（D617–D637）

受《Mindustry》与《Turing Complete（图灵完备）》启发，Rynvek 的音乐与声效传递 **“轻快、理性、富有机械秩序感与生命涌现律动”** 的听觉体验：

* **6 轨自适应音乐（Multi-stem Adaptive Music @ 120 BPM）**：同一首乐曲拆分为 `Stem A (Ambient Pad)`、`Stem B (Sub Bass)`、`Stem C (Industrial Percussion)`、`Stem D (Logic Arpeggio)`、`Stem E (Combat Synth)`、`Stem F (High Intensity Lead)` 六条同步音轨。随世界状态从“安静观察 → 基地建设 → 规模生产 → 局部交火 → 决战告警”演进，在小节边界平滑叠加或抽离音轨。

* **前期零资产依赖的程序化占位符架构**：开发期全部 24 类 UI、建筑、单位、战斗与神经事件均由 `@rynvek/audio` 内置的 Web Audio 合成器实时生成（精确参数见《开发规范文档》规范 12.3），后期仅需在 `AudioManifest` 中将 `status: "placeholder"` 替换为 CC0/原创 `.ogg` 文件路径即可无痛热替换。

## 第七章：AGPL-3.0 开源许可架构、安全威胁模型与社区治理

### 7.1 `AGPL-3.0-only` 核心与 SDK 接口豁免双层许可架构（ADR-L001–L016, D425–D430, D675）

Rynvek 致力于成为开放、不可被云厂商闭源白嫖（Strip-mining）的公共基础设施，同时又要保障广大规则创作者、科研学者与 AI 赛事选手的作品拥有完全的知识产权自主权。为此，项目冻结了清晰的 **双层许可证边界**：

1. **核心平台层（`AGPL-3.0-only`）**：

   * 覆盖 `rynvek-engine`、`rynvek-server`、`rynvek-studio`、`rynvek-arena`、`rynvek-lab` 全部 Rust 与前端应用源码。

   * 明确采用 **`AGPL-3.0-only`**（而非 `or-later`），所有源文件头部强制嵌入符合 REUSE 3.3 标准的 SPDX 注释，并在服务端内置 `/api/v1/meta/source-offer` 端点自动履行 AGPLv3 第 13 条网络源码提供义务。

2. **协议与客户端 SDK 接口层（`Apache-2.0 WITH LLVM-exception`）**：

   * 覆盖 `protocols/*`（`.wit`、`.proto`、JSON Schemas）、`crates/rynvek-protocol`、`@rynvek/core`、`@rynvek/protocol`、`@rynvek/rules`、`@rynvek/agent-sdk` 及 Python `rynvek` 包。

   * **法务边界承诺**：任何通过标准 IPC、WebSocket/WebTransport、`.rir` 字节码或 `rynvek:plugin@1.0.0` WIT 边界与引擎交互的外部 Agent、TypeScript 世界规则包（`.rpack`）、世界包（`.rworld`）和视觉包（`.rview`），均被视为运行在平台之上的独立用户程序/内容，**不受 AGPL-3.0 传染**。

3. **资产、数据与贡献者协议**：

   * 官方文档与基础工业像素资产采用 `CC-BY-4.0`；程序化占位音效与基础公共音效采用 `CC0-1.0`；字体采用 `SIL OFL-1.1`（`Geist` & `Noto Sans SC`）。

   * 贡献者治理采用轻量级 **`DCO 1.1`（`git commit -s`）**，不要求签署转让版权的 CLA。

### 7.2 不可信代码四级信任模型与多维防耗尽保护（D139, D240–D246, D650–D653）

由于在 Rynvek 中“用户编写的规则、着色器与 Agent 就是平台的主要输入”，安全架构默认将所有非官方内置包视为 **完全不可信输入（Untrusted Input）**，实施四级信任隔离：

* **`TrustLevel 0 (Core Built-in)`**：随官方二进制编译发布的 Rust 内核模块。

* **`TrustLevel 1 (Official Signed)`**：由 Rynvek 官方私钥（Ed25519）签名并发布于 `rynvek.*` 命名空间的扩展包。

* **`TrustLevel 2 (Verified Creator)`**：通过 `Rynvek Hub` 实名开发者签名、自动化静态分析与 1,000 Tick 沙箱压力测试的社区包。

* **`TrustLevel 3 (Untrusted Ephemeral)`**：未签名或本地导入的第三方包、自定义房间临时下载的 `.rworld` / `.rview`。强制运行于最严格的沙箱配置（零网络权限、零宿主文件系统访问、单 Tick CPU 预算 $\le 500,000\text{ Gas}$、堆内存 $\le 64\text{ MiB}$、WGSL 片段着色器指令数 $\le 512$）。

## 第八章：全量架构决策统一注册表（Unified ADR Register）

本章将项目 66 轮讨论中产生的全部 **732+ 项架构决策（`ADR-BRAND-001`、`ADR-L001–L016`、`ADR-001–020`、`E001–E040`、`D041–D675`）** 整理为无冲突的终局决策字典，供 AI Coding Agent 在实现任意子系统时快速检索决策结论与设计初衷。

### 8.1 品牌与许可证治理决策集（`ADR-BRAND-001` & `ADR-L001` – `ADR-L016`）

| 决策编号 | 决策主题 | 终局冻结结论（Frozen Decision） | 核心理由与否决备选方案 | 
| ----- | ----- | ----- | ----- | 
| `ADR-BRAND-001` | 项目主品牌与核心命名 | 主品牌定为 **Rynvek** (`RYNVEK`)；核心引擎定为 **Rynvek Engine**；视觉包定为 **`.rview`**；保留 `Neural Soup` 作为学术/技术范式术语。 | 否决 `Neuroverse`（美国已注册商标及多项同名项目）与 `Zivren`（已有同名商业网站）。 | 
| `ADR-L001` | 核心代码开源许可证 | 采用 **`AGPL-3.0-only`**。 | 防止云服务商闭源托管修改版 `Rynvek Server`；明确版本以避免跨大版本条款不确定性。 | 
| `ADR-L002` | 商业双重许可政策 | 默认不开放闭源商业双许可；通过官方托管云服务（Arena/Hub/Lab Cloud）实现商业可持续。 | 配合 DCO 1.1 保持社区贡献纯粹性。 | 
| `ADR-L003` | 版权持有模式 | `The Rynvek Contributors` 共同持有版权。 | 契合开放社区治理。 | 
| `ADR-L004` | 贡献者授权机制 | 强制采用 **`DCO 1.1`**（`Signed-off-by`），CI 自动检查。 | 避免 CLA 带来的外部贡献法律阻力。 | 
| `ADR-L005` | 源码文件许可证标记 | 全仓库遵循 **REUSE 3.3** 标准，每文件头标注 `SPDX-License-Identifier`。 | 支持 `reuse lint` 自动化 100% 扫描验证。 | 
| `ADR-L006` | 第三方依赖白名单 | 由 `deny.toml` 强制执行白名单（`MIT`, `Apache-2.0`, `BSD`, `MPL-2.0`, `Zlib`, `CC0`），严禁 `GPL-2.0-only` 与 `SSPL`。 | 防止许可证冲突污染 AGPLv3 主工程。 | 
| `ADR-L007` | 官方文档许可证 | 采用 **`CC-BY-4.0`**。 | 便于学术引用、教程传播与翻译。 | 
| `ADR-L008` | 官方美术资产许可证 | 基础工业美术包采用 **`CC-BY-4.0`**，每个 `.rview` 包内含独立 `LicenseManifest`。 | 将代码许可证与艺术资产许可证严格解耦。 | 
| `ADR-L009` | 官方字体许可证 | 采用 **`SIL OFL-1.1`**（`Geist Sans`, `Geist Mono`, `Noto Sans SC`）。 | 允许免版税Web嵌入与离线打包分发。 | 
| `ADR-L010` | 科研数据集许可证 | 基准实验与谱系数据集采用 **`CC0-1.0`** 或 **`CC-BY-4.0`**。 | 符合开放科学（Open Science）期刊复现要求。 | 
| `ADR-L011` | 用户生成内容（UGC）权属 | 创作者保留版权；公开上传至 `Rynvek Hub` 时授予平台免费托管与分发许可。 | 保护创作者权益，消除平台托管法务风险。 | 
| `ADR-L012` | 插件与规则包许可证边界 | 官方扩展包采用 `AGPL-3.0-only`；第三方 `.rpack` 通过公开 ABI 调用，自主选择许可证。 | 繁荣第三方 Mod 与规则生态。 | 
| `ADR-L013` | 外部 Agent 与 SDK 许可证 | `protocols/*` 与客户端 SDK 采用 **`Apache-2.0 WITH LLVM-exception`**。 | 消除参赛 AI 选手与科研团队的传染顾虑。 | 
| `ADR-L014` | AGPLv3 §13 网络合规实现 | 服务端内置 `/api/v1/meta/source-offer` 自动返回当前 Commit 源码包 URL 与 SPDX SBOM。 | 将法律合规转化为编译期自动化工程特性。 | 
| `ADR-L015` | 商标与品牌视觉保护 | `Rynvek` 文字与图形 Logo 独立于 AGPL 代码授权，未经认证的修改版衍生服务不得自称 Official Rynvek。 | 遵循 Mozilla / Linux 基金会标准开源商标实践。 | 
| `ADR-L016` | 软件供应链可信证明 | 正式 Release 强制生成 **SPDX 2.3 SBOM** 与 **SLSA Level 3 Build Provenance**。 | 保障可复现构建与防篡改分发。 | 

### 8.2 基础工程与早期核心决策集（`ADR-001–020` & `E001–E040`）

| 决策编号范围 | 核心领域 | 终局冻结标准（已整合进《开发规范文档》对应模块） | 
| ----- | ----- | ----- | 
| `E001–E005` (`ADR-001–003`) | 工作区与三层实体身份 | Cargo Workspace Resolver 3；24 Crates 单向分层依赖；`EntityId(u128)` 持久身份 + `EntityHandle(u64)` CPU 代数句柄 + `GpuEntityRef(u64)` GPU 紧凑槽位。 | 
| `E006–E011` (`ADR-004–005`) | 时钟、坐标、量纲与分页 SoA | 30Hz 固定权威主时钟；`Chunk(i32,i32) + Q32_32` 权威坐标；16B 对齐 GPU ABI；`65,536` 容量 `DomainPage` + `Phase 13` 延迟结构提交。 | 
| `E012–E020` (`ADR-006–009`) | Tick 阶段、冲突、RNG 与 Rule IR | 16 阶段标准拓扑信封；8 策略 `ConflictPolicy`；无状态 `Philox4x32-10` RNG；D0–D3 确定性分级；36 条指令的 `Typed SSA Rule IR`。 | 
| `E021–E026` (`ADR-010–012`) | CPU/GPU 双后端与资源管理 | 强制维护 `rynvek-compute-cpu` 作为黄金参照；`wgpu` 32MiB 分段缓冲分配器；3-Slot 异步 Readback Ring；ABI 感知的着色器缓存。 | 
| `E027–E029` (`ADR-013–014`) | 神经网络批处理与演化时机 | `Genome` / `BrainTopology` / `BrainInstance` 生命周期分离；按 `NeuralGroupKey` 同拓扑合批推理；结构突变严格限制在 Coupling Epoch 边界执行。 | 
| `E030–E040` (`ADR-015–020`) | Web Worker、渲染、网络与持久化 | Dedicated Engine Worker 独占 `wgpu::Device` + `OffscreenCanvas`；GPU-Driven 实例渲染；Single-Writer World Authority；`WebTransport + WebSocket`；`.rsnap + .rjournal` 持久化；Wasmtime WIT 插件。 | 

### 8.3 全生命周期深化架构决策集（`D041` – `D675` 按专题完整归档）

| 决策号段 | 专题域名称 | 终局冻结规范摘要（全部具体数据结构见《开发规范文档》） | 
| ----- | ----- | ----- | 
| **`D041–D090`** | **v0.7 跨领域基础设施与生态契约** | \- `D041, D084`：确立 `Q32.32` 定点坐标、编译期物理量纲与跨设备数值一致性测试。  \- `D042, D061–D062, D076–D077`：Domain 四元拆分（Schema/Storage/Executor/Capability）、RuleSet 组合覆盖规则、WASM WIT 插件与恶意包吊销列表（Revocation List）。  \- `D044, D063`：Tick 事务候选态（Candidate State）隔离，任何阶段验证失败原子回滚至上一已提交版本。  \- `D046, D053, D060, D073–D075`：多层控制仲裁、Match 状态机、按 Ruleset Hash 隔离匹配池、带延迟的观战流（Delayed Spectator）与版本化 `.rreplay`。  \- `D048–D049, D068, D078–D079`：World Copy-on-Write 分支 Fork、带租约 Fencing Token 的迁移、CRDT 创作态协作编辑。  \- `D050, D069–D070, D085`：不可变科研实验任务（Immutable Experiment Job）、标准消融实验对照集与开放 Parquet/Arrow 导出。 | 
| **`D091–D115`** | **v0.8 规则语言复用与多语言运行时** | \- `D091–D095, D100–D104, D107–D112`：采用 TypeScript 作为唯一主力规则创作语言；Tier-0 通过 `@rynvek/rules` AST Builder 编译为 `Typed Rule IR` → `WGSL`；Tier-1 运行于 `QuickJS-NG WASM` 确定性沙箱；支持 Tick 边界热重载与 CPU 源码级断点调试。  \- `D096, D105–D106, D113`：Python 定位于独立进程 Research Worker 与 AI 训练，通过 `PyO3 + Apache Arrow IPC` 零拷贝交换列式张量；AI 模型权重封包为内容寻址不可变 Artifact。 | 
| **`D116–D146`** | **v0.9 状态根、GPU 安全与网络复制** | \- `D116–D118`：`WorldStateRoot` 增量 `BLAKE3` 哈希树；实体三层身份；数据修改与结构修改分离。  \- `D119–D123`：D0–D3 确定性 Profile；WGSL 显式边界检查与资源声明；Pipeline 磁盘/IndexedDB 缓存。  \- `D124–D129`：服务端权威仿真协议、受限客户端预测、战争迷雾权限隔离、WebTransport/WebSocket 传输、Single-Writer 线程模型。  \- `D130–D140`：`Metadata + .rjournal + .rsnap + BlobStore` 四层存储、Schema 有向迁移图、Ed25519 包签名、四级扩展信任模型。  \- `D141–D146`：`Intent → Order → Action` 控制链、分层导航、渲染与仿真解耦、三种官方部署形态（Browser Standalone / Desktop / Server Cluster）。 | 
| **`D147–D188`** | **v1.0 Tick 精确语义、神经生命周期与编辑器** | \- `D147–D151`：16 阶段 Tick 执行契约、`ReadPhaseSnapshot` 可见性、`Command/Event/StateMutation` 三元分离、结构变更后自动刷新 GPU 绑定。  \- `D152–D156`：神经对象三层模型、推理与学习阶段分离、`Genome → Morphogenesis → Phenotype` 发育流、神经元与脉冲代谢扣费预算。  \- `D157–D162`：四级多保真度仿真 LOD、连续场反应扩散模型、Domain 注册表与 Schema 字段向后兼容规则。  \- `D163–D167`：`Principal / Controller / Entity` 区分、统一 `DecisionModel`、LLM Agent 强制经过受控 `ToolBroker` 调用。  \- `D168–D174`：统一 GPU 资源所有权、Submission Graph、Rust `#![deny(unsafe_code)]` 策略与软件供应链审计。  \- `D175–D188`：`Rynvek Studio` 的 `Draft → Validate → Publish` 工作流、资产管线独立于 World IR、竞技规则集认证与双倍记账经济结算。 | 
| **`D189–D232`** | **v1.1 跨模块事务、快照格式与调度器** | \- `D189–D195`：统一 `CommitManifest`、逻辑/物理/持久化事务区分、`Philox` 随机流隔离、权威状态与派生表现状态严格边界、Tombstone 保留期与 Canonical Ordering。  \- `D196–D201`：`.rsnap` 64B Header + 分块二进制格式、分层 `.rjournal`、三种回放等级（Bit-Exact / State-Checkpointed / Kinematic-Stream）、冷热数据分层。  \- `D202–D208`：16B 网络帧生命周期、分层 Delta 复制、Fencing Token 迁移、服务端反作弊校验与裁判回放系统。  \- `D209–D215`：CPU/GPU 统一工作调度器、GPU 准入控制（Admission Control）、背压协议、Staging Buffer 3-Slot 环、`DeviceLost` 恢复。  \- `D216–D232`：`ControlLease` 与人类无感接管、Agent 记忆隔离、在线学习与天梯策略锁定分离、24 小时 Soak Test 与 Conformance Matrix。 | 
| **`D233–D286`** | **v1.2 编译器管线、空间物理与 RTS 结算** | \- `D233–D246`：36 条 `Typed SSA Rule IR` 算子、静态读写集与有界循环验证、Capability Token 鉴权、沙箱 Gas 计量。  \- `D247–D253`：多空间计算域、UniformGrid CSR + DynamicBVH 双空间索引、碰撞检测与冲量求解分离、CFL 自适应子步。  \- `D254–D260`：神经拓扑/动力学/可塑性解耦、三种神经图存储后端、事件驱动脉冲调度、Dale 定律与鲍德温同化边界。  \- `D261–D266`：RTS 三层控制、服务端 `FogOfWarDomain`、确定性战斗伤害与护甲克制公式、胜负与超时积分结算契约。  \- `D267–D286`：Render/Simulation/UI 状态三分离、独立输入抽象层、WCAG 无障碍契约、以 Commit 为中心的可观测性、属性测试（Property-based Testing）与 Metamorphic Testing。 | 
| **`D287–D315`** | **v1.3 多时钟因果、页表内存与实时热编** | \- `D287–D290`：30Hz/120Hz/10Hz/2Hz 多速率时钟、`EventKey` 因果全序、异步 Agent 请求-响应与逻辑时间对齐。  \- `D291–D296`：Hybrid ECS + SoA 页表、三层实体 ID、`ConflictPolicy` 8 种合并策略、延迟结构变更缓冲、64B/256B 对齐内存池。  \- `D297–D304`：GPU 状态页 Copy-on-Write 版本化、NaN/Inf 零容忍检查与候选帧自动回滚契约。  \- `D305–D315`：会话断线重连令牌（`ResumeToken`）、创作态与运行态隔离、三方语义 `WorldPatch`、四级 `LiveEditSafetyClass`、流式资产驻留管理（Residency Manager）。 | 
| **`D316–D369`** | **v1.4 分布式分区、WAL 恢复与组织治理** | \- `D316–D321`：`WorldAuthority` / `ExecutionWorker` / `StorageOwner` 职责模型、`1024×1024` 分区与 Halo 镜像交换、跨界 `EntityHandoff`。  \- `D322–D327`：`.rjournal` WAL 预写日志、4 步崩溃恢复状态机、日志压缩保留策略、World Fork/Branch/Merge 语义。  \- `D328–D333`：竞技权威 Profile、输入缓冲延迟补偿、反作弊威胁矩阵、Glicko-2 分赛道天梯排位。  \- `D334–D349`：多级 Agent 委派仲裁、Schema 注册表与有向迁移图（`MigrationGraph`）、`dependencies.lock` 依赖锁定与可复现构建。  \- `D350–D369`：多级调度器、`Principal / Account / Membership` 三层身份、RBAC + ABAC 混合授权、多 Registry 联邦、SLO/SLI 错误预算与生产运维 Runbook。 | 
| **`D370–D430`** | **v1.5 沙箱执行、API 基元与安全供应链** | \- `D370–D382`：统一执行环境抽象、QuickJS-WASM 与 Wasmtime WIT 隔离域、插件热升级与一致性测试套件。  \- `D383–D394`：`Command / Query / Event / Stream` 四大 API 基元、`UUIDv7` 幂等键、GPU Kernel 变体注册表与执行成本模型。  \- `D395–D418`：异步任务取消与超时契约、世界休眠唤醒语义、确定性执行追踪（Execution Trace）、`rynvek` CLI、本地开发服故障注入（Fault Injection）。  \- `D419–D430`：供应链根证书信任链、恶意包紧急吊销协议、密钥轮转、GDPR 数据导出删除契约、四类资产许可证矩阵与 AI 生成内容溯源标记。 | 
| **`D431–D524`** | **v1.6 仿真、神经、场、RTS 与测试终局收敛** | \- `D431–D449`：固化 16 阶段 Tick 信封、Simulation Invariant Registry、ECS 组件 Schema 注册与默认值迁移策略、世界状态所有权映射表。  \- `D450–D469`：固化四大神经元模型接口、突触图 ABI、脉冲调度器、可塑性引擎、连续场求解器、场-神经双向耦合端口与守恒律验证器。  \- `D470–D498`：固化 `rynvek-rts` 领域包、HPA\*+流场+定点 ORCA 导航、战斗与经济事务、AI 模型推理契约、`ToolBroker`、分层状态 Delta 编码与网络拥塞 QoS。  \- `D499–D524`：固化 12 层测试金字塔、Golden Vectors、差分测试、模糊测试（Fuzzing）、资产三层流水线（Source/Intermediate/Runtime）与长期存档可复现契约。 | 
| **`D525–D592`** | **v1.7 分区提交、内存池、并发锁与科研平台** | \- `D525–D540`：固化单主与多分区权威边界、三级跨分区一致性协议、存储引擎抽象、`.rjournal` 帧格式与外部副作用 Outbox 对齐。  \- `D541–D560`：固化四级内存预算、GPU 分段分配与驻留管理、线程亲和性、严格无死锁锁层次（Lock Hierarchy）、阶段屏障内存可见性与多级背压调度。  \- `D561–D578`：固化 TypeScript 三种执行模式、Canonical 数值语义、编译缓存、多租户配额隔离、审计日志、WebGPU 能力协商与页面后台生命周期处理。  \- `D579–D592`：固化 `Rynvek Lab` 实验运行时、实验谱系追踪、分布式训练作业契约、统计显著性评估、部署拓扑与运行时自检诊断。 | 
| **`D593–D616`** | **视觉设计系统与工业像素美术（Turn 50）** | \- 固化 `Rynvek Design System`（shadcn 风格 `--rv-*` Tokens，`Geist Sans/Mono`）、可停靠工作区（Dockable Workspace）、`Rynvek Industrial Emergence` 32×32 工业像素规范、L0–L4 神经多尺度可视化、L0–L7 WebGPU 渲染层、`.rview` (`RYVW`) 视觉包与截图视觉回归测试。 | 
| **`D617–D637`** | **音频运行时与神经活动声音化（Turn 52）** | \- 固化工业电子与有机演化融合音色、6 场景多轨自适应音乐（Multi-stem @ 120 BPM）、8 大音效分类、群体聚合 `NeuralSonification`、独立 Web Audio + `AudioWorklet` 运行时、Commit-Aware 发布、五大混音总线与 24 类程序化合成占位符。 | 
| **`D638–D675`** | **终局产品、输入、反作弊、构建与治理（Turn 54）** | \- `D638–D644`：固化 8 大产品矩阵、游戏设计圣经与平衡数值表、三维正交世界配置、统一输入抽象层与 36 项多上下文动作映射。  \- `D645–D657`：固化权威多人协议、命令校验、反作弊五防线、对局状态机、回放验证、RBAC+ABAC+Capability 鉴权与数据保留分级。  \- `D658–D675`：固化资产三层体系、`dependencies.lock` 依赖图、Monorepo 构建图、CI 全套门禁、三档硬件性能量化预算、6 级优雅降级、文档架构与双层许可证矩阵。 | 

## 第九章：M0–M8 研发里程碑路线图与 AI Coding Agent 自治执行指南

当 AI Coding Agent 接收本文档与《开发规范文档（`Rynvek_Development_Specification_v1.0.md`）》后，应严格按照以下 **M0 至 M8 九个顺序阶段** 推进开发。每个阶段均具备明确的输入依赖与可自动执行的验收测试命令（零人工主观判断）：

1. **阶段 M0：工作区初始化、核心定点数、坐标、RNG 与跨语言协议底座**

   * **实现目标**：创建 Monorepo 根目录配置文件与 `rynvek-core`, `rynvek-schema`, `rynvek-protocol`, `xtask`, `@rynvek/core`, `@rynvek/protocol`。

   * **自治验收指令**：`cargo test -p rynvek-core -p rynvek-schema -p rynvek-protocol && pnpm --filter @rynvek/core test && cargo xtask verify-golden-vectors`。

2. **阶段 M1：混合列式存储（Paged SoA）、三层实体标识与 `.rsnap` / `.rjournal` 持久化引擎**

   * **实现目标**：实现 `rynvek-memory`, `rynvek-ecs`, `rynvek-storage`, `rynvek-journal`, `rynvek-snapshot`（含 SQLite/PostgreSQL DDL 迁移与 `CrashRecoveryStateMachine`）。

   * **自治验收指令**：`cargo test -p rynvek-ecs -p rynvek-snapshot -p rynvek-journal`（验证 100,000 实体增删改、两阶段 Tombstone 回收与断电重放状态根 `BLAKE3` 100% 一致）。

3. **阶段 M2：16 阶段仿真调度器、连续环境场、空间索引与 CPU Reference 后端**

   * **实现目标**：实现 `rynvek-field`, `rynvek-spatial`, `rynvek-simulation`, `rynvek-compute`, `rynvek-compute-cpu`。

   * **自治验收指令**：`cargo test -p rynvek-simulation -p rynvek-field -p rynvek-spatial`（验证 5 通道反应扩散场守恒、CFL 自动子步、CSR 邻居表与 16 阶段 Tick 推进 `10,000 Ticks` 零误差）。

4. **阶段 M3：Neural Soup 原生运行时、四因子 STDP、规则编译器与沙箱**

   * **实现目标**：实现 `rynvek-neural`, `rynvek-rules`, `@rynvek/rules`（含 4 种神经元模型、3 种图存储布局、`NEUR` 快照分块、36 条 `Typed SSA Rule IR` 转 WGSL 编译器与 QuickJS-WASM 沙箱）。

   * **自治验收指令**：`cargo test -p rynvek-neural -p rynvek-rules && pnpm --filter @rynvek/rules test`。

5. **阶段 M4：Dedicated Engine Worker、`wgpu` 计算与 L0–L7 世界渲染器**

   * **实现目标**：实现 `rynvek-wgpu`, `rynvek-render`, `rynvek-engine`, `rynvek-wasm`, `@rynvek/engine`（含 `OffscreenCanvas` 桥接、BindGroup 0–3、WGSL 核心着色器、CPU/GPU 差分测试与 `DeviceLost` 恢复）。

   * **自治验收指令**：`cargo test -p rynvek-wgpu -p rynvek-render -p rynvek-engine`。

6. **阶段 M5：Rynvek UI 设计系统、程序化占位音频合成器与 `Rynvek Studio`**

   * **实现目标**：实现 `@rynvek/ui`, `@rynvek/audio`, `@rynvek/visualization`, `@rynvek/assets`, `@rynvek/studio`（含 `--rv-*` 三主题 Token、24 类 Web Audio 程序化合成音效、`.rview` 加载器、Dockable Workspace 与 CRDT 实时编辑补丁）。

   * **自治验收指令**：`pnpm --filter @rynvek/ui --filter @rynvek/audio --filter @rynvek/studio test`。

7. **阶段 M6：RTS 领域包（`rynvek-rts`）、多级 Agent 控制与 `Rynvek Arena` 可玩闭环**

   * **实现目标**：实现 `rynvek-agent`, `rynvek-rts`, `@rynvek/client`, `@rynvek/arena`（含 3 资源、8 建筑、6 神经单位、9 项科技演化树、180° 对称地图生成器、HPA\*+流场+定点 ORCA 寻路、L3 四维调质广播、LLM `ToolBroker` 与 PvE 自适应演化虫潮）。

   * **自治验收指令**：`cargo test -p rynvek-rts -p rynvek-agent && pnpm --filter @rynvek/arena test`。

8. **阶段 M7：权威多人联机服务（`Rynvek Server`）、战争迷雾裁剪、天梯匹配与反作弊**

   * **实现目标**：实现 `rynvek-network`, `rynvek-security`, `rynvek-server`, `rynvek-cli`（含 `WebTransport/WebSocket` 16B 帧同步、服务端 `VisibilityGrid` 物理迷雾裁剪、Glicko-2 排位、`.rreplay` 验证与 `/api/v1/meta/source-offer` AGPL 合规端点）。

   * **自治验收指令**：`cargo test -p rynvek-network -p rynvek-security -p rynvek-server -p rynvek-cli`。

9. **阶段 M8：Python 科研生态（`Rynvek Lab`）、WASM WIT 插件与 `Rynvek Hub` 分发闭环**

   * **实现目标**：实现 `python/rynvek`（PyO3 + Arrow 零拷贝 + Gymnasium/PettingZoo 环境）、`@rynvek/lab`、Wasmtime `rynvek:plugin@1.0.0` 宿主与 `Rynvek Hub` 包签名注册服务，并通过 24 小时浸泡测试（Soak Test）。

   * **自治验收指令**：`pytest python/tests && cargo xtask ci-full-gate`。