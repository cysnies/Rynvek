<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 通用 World 声明组件、受控规则与有来源计价

阶段50补充：[真实TS AST与可复验Tier0构建](TIER0_TS_AST_BUILD.md)已接入受限Kernel语法及原World链；以下当批“TS AST未做”保留为历史口径，不意味着已交付完整私有rpack依赖签名安装链。

阶段49补充：单Chunk通用World现已通过原服务器身份/权限/控制租约/宿主fence/Tick提交与私有恢复链接入；精确入口和仍未交付范围见[GENERAL_WORLD_SERVER_ACTOR.md](GENERAL_WORLD_SERVER_ACTOR.md)。下文阶段48当批验项/缺口保留为历史口径。原本地Engine限制仍适用于该原入口，不代表新受限服务器入口已具备浏览器流/协作或任意World。

阶段48从干净 `56982a0` 继续。此段实现原 RYWL → 声明模板/实例/RuleGraph 或受限 RYPK → 原 Phase07 → 真实刚体加速度/位移 → 原权威根/文件快照/WAL 恢复，不是只存规则或另造平行执行器。仍是单 Chunk 圆形物理剖面的受控组件适配，不称任意 World 或正式发行。

## 复用关系与安装边界

将阶段36/45 Arena 的模板、组件校验、Tier0/Tier1 同旧态执行、写冲突与沙箱实现下沉至 `rynvek-rules::components`；Arena 保留原 public re-export、原创作补丁与故障语义。Simulation 不依赖 Arena 或 rts crate，而调用同一 Registry、原 RuleGraph→RIR/CPU 与无 I/O QuickJS。不是复制另一套规则解释器，也没有重做阶段47生命场。

RYWL `world.json` 的可选 `components` 使用 `rynvek.general-components.v1`，含 `templates/instances/bindings/rule_graphs/tier1_rules/body_controllers`、显式 `world_seed/gas_budget` 与可选 `rule_cost_micro_per_gas`。只有创世显式安装，本段无热改、Studio通用身体编辑或服务器Actor入口。原 `GENERAL_WORLD` 命令仍仅本地、暂停、Tick0空世界；远端Arena/既有世界拒绝，不修改fog或生产fence。

## 声明与受控物理绑定

- 原 `I32/Q8_8/Bool` 的 `state.*` 动态值和只读 `param.*`；同模板组件类型、默认值和显式实例参数必须一致。16模板/128实例/16组件/16绑定，最多32图与4个Tier1包；RuleGraph单源256KiB、总源1MiB、完整组件配置JSON1MiB，原RYWL整体128KiB另有限制。
- `body_controllers` 按持久 `entity_serial` 绑定，不按稠密槽位；身体与实例各至多一个绑定，固定体拒绝。只映射 `state.accel_x/state.accel_y` 的Q8.8值至原Q32.32加速度，不夹制越界输出；原±32加速度/±8速度界仍有效，违规整Tick拒绝。
- 组件可以记录有界整数计算状态，但不能写原生命账户、HP/质量/场或任意内核组件；不接受场采样/邻居接口。本段没有额外赋予身体传感器。规则仅看声明组件、tick和seed；Tier1不能看到宿主隐藏身体/fog/源码凭据或I/O。
- Tier0与Tier1同读旧组件态；模板组件单写者预检，所有写集一并提交。通用World中任一规则故障、欠费或物理输出非法，使原候选Tick连同场步/摄取/组件/账户/物理/时钟整帧回滚。Arena保留原故障标志/原脑动作语义，并不改为通用World的整Tick拒绝策略。
- serial退役不复用；保留声明实例可继续CPU计算，但无身体动作、无生物收费，不会控制后来占同一稠密槽位的新身体。无执行Gas的生命绑定每Tick清零加速度，不以默认/旧意图免费推进。

## CPU预算不等于生物能量

所有规则共享每Tick正预算，最多500000。Tier0收费基于原CPU执行器的实际 `entity_gas`；Tier1仍为原QuickJS中断轮询**粗配额**（成功调用至少10000），不是JS逐指令精确Gas。Tier1一次模板调用实际占用的配额按实例ID排序，以商/余数确定分摊，分摊总量严格等于该次配额，不向每身体重复收取整次配额。

只有显式生命profile且有身体绑定时，必须声明正 `rule_cost_micro_per_gas`：每个仍存活绑定身体按其Tier0实际Gas + Tier1分摊配额 × micro价 × 4q，向阶段47同一 `EnergyAccount` 调用 `try_dissipate_q`。收入仍只能来自真实营养摄取，费用只转原dissipated子项；`paid_q`为当Tick已付款观测值，不是额外能量，也不二次加入闭世界式。宏观Biomass/Flux/Neurogel仍为0，不记Arena阵营账。

无生命身体/无身体参数实例只受CPU硬预算，不伪造生物能量收费；这不表示任意扩展均已物理计价。生命身体绑定与LIF暂互斥，避免两个加速度writer和重复驱动计费；独立参数计算可与无绑定LIF共存。安装顺序不能绕过该检查。价格与加速度映射是显式实现profile，不是假称规范冻结的物理定律。

费用是生物规则执行许可；原加速度驱动仍非严格机械功—化学能转换模型，物理质量也不是生命结构材料。本段不从运动凭空增加生命能量/营养，不声称完整机械能、材料或宇宙热力学证明。

## 原根与恢复

原十域Commit的config_root绑定完整规则定义，entity_root绑定实际组件值/分配器/计量；快照追加可选 `RULS`（最多2MiB），重新编译源、验模板/安装/状态/宿主引用，再逐实际领域重算权威根。重封装为合法压缩容器但改配置/动态值/付款计量仍拒绝。未启用组件时原物理四块及原LIFE/NEUR格式不变；可选块不做静默旧格式迁移。原文件8MiB/解压16MiB/WAL32MiB等限制保留。

原日志 `sync_data` 后才发布，原恢复器从创世快照逐提交重仿真，规则随同原生身体/生命账户一起恢复；不是旁路规则日志或只打印最终镜像。文件恢复测试不是真实掉电。

## 真实可执行入口

配置为 `examples/worlds/general-rules.world.json`，JSON schema为 `protocols/schemas/general-world.schema.json`。输出路径必须不存在：

```sh
cargo run -p rynvek-cli --locked -- world build \
  --config examples/worlds/general-rules.world.json \
  --id world.general.rules --out /tmp/general-rules.rworld
cargo run -p rynvek-cli --locked -- world run \
  --package /tmp/general-rules.rworld \
  --world-id 03030303030303030303030303030303 --ticks 4 \
  --snapshot /tmp/general-rules-genesis.rsnap --journal /tmp/general-rules.rjournal
cargo run -p rynvek-cli --locked -- world recover \
  --snapshot /tmp/general-rules-genesis.rsnap --journal /tmp/general-rules.rjournal
```

CLI `components` 返回同一声明/实际动态态及当Tick `tier0_gas/tier1_quota/paid_q`；不启用时为null。示例真实正向位移，摄取与付款都来自给定营养沉淀；不新增营养源或生命出生禀赋。

## 必要快验与真实未完成项

新增13个不同用例：simulation规则10、RYWL跨profile1、真实CLI子进程1、原生Engine命令1。合计本批56个不同定向用例通过（重跑/过滤不累计）：通用规则10、原生命8、物理12、Arena Tier1 8/参数Lab2、World registry2/通用库4、Engine通用5、CLI3、原16阶段/10Hz1、原迷雾1。含精确有来源扣款、Tier1粗配额分摊、同旧态、欠费/坏输出/预算/沙箱循环整Tick回滚、稳定serial、RULS篡改拒绝、真实文件WAL与后续根一致；原Arena单写者/权限边界与四表仍通过。

必要 `cargo check -p rynvek-server -p rynvek-wasm --offline --locked` 通过，但wasm为**宿主原生target类型检查**，不是wasm32产物或真实浏览器Worker；JSON与schema文件仅做JSON语法检查，本机未安装jsonschema，未声称运行schema语义验证。源码rustfmt及diff检查通过；完整workspace/Clippy/license/SBOM/安全/soak/CI未跑。

下一主体优先统一权限Actor与通用World服务端命令/观察/持久fence接入；Tier0 TS AST、完整私有rpack签名依赖发现验签安装链仍欠，本地RYWL内嵌原包不冒称私有registry链。多Chunk/三维/任意组件、完整Neural Soup材料/生长/自然遗传迁移、Agent独立赛道与协作presence/离线多主也未完成。旧6个PvP_Custom Actor夹具仍需真实lease前置整改，禁止降低生产fence。

真实Node22运行时、C#本批重编、wasm32、真实浏览器/硬件GPU、真实PG多机/并发/断电/24h长稳未验；阶段41llvmpipe仍仅软件回读。主体齐后集中完整门禁、安全及正式发布打磨，不以本段宣布项目完成。
