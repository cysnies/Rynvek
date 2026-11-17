<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 持续开发与可执行缺口

更新时间：2026-11-17，中国标准时间 UTC+8。旧M0–M8标签不是全项目完成证明；当前仍在主体功能阶段。

## 第二十批：配置角色真实供能、运动和突触维护（从main 07105d8继续）

当前入口仍为真实世界创建`POST /api/v1/worlds`的`ecology`字段、Lab变体的同名字段，以及Rust `EcologyContract`。TypeScript/Python同步采用**唯一当前角色表**；旧平铺Forager三参数不再接受，不留别名/迁移。Arena当前仅`rynvek-arena/7` / `CURRENT_7`，私有BodyEnergy新增运动小数余量，随原规范根、快照/WAL恢复。

```json
{"roles":{
  "unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true},
  "unit.striker":{"basal_cost_micro_per_tick":100,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":100,"synapse_maintenance_micro_per_tick":1,"natural_reproduction":false}
}}
```

数值仅为**本批真实HTTP快验参数，不是原规范冻结值或平衡推荐**。所有六项角色参数都必须显式填写；两个新增成本的零值明确表示该实验关闭此项，不能宣称完整生命成本。`roles`可选用六种真实单位原型，但不能为空；野生个体不使用玩家阵营供应契约。未配置角色仍为原诊断口径，不伪称全体已预算化。

实际行为：

- 玩家Forager继续等额扣局部营养摄食；其他五种配置角色使用同一现有Biomass账本补给，按实体序确定分摊。只转移实际可付且身体能接收的整milli，每milli=4000q，满账/余额不足不先扣款。此为原RTS阵营共享池工程规则，不声称新增了有物流半径的物理输运；原开局库存也是明确来源，未新造身体能量。
- 补给目标至少覆盖配置出生投入、维护/运动预留及既有脉冲模型下一Epoch上界；目标只是在原池内的预算，不是额外能量。允许自然繁殖时再预留一份实际出生投入，否则不自动自然出生。生产完成时对**所有配置角色**执行真实出生付款；欠款队首停在最后Tick，不弹队或耗序号。
- 脑仍用原候选真实脑原子支付。运动先求导航/ORCA/滑墙后的**实际位移**，再按micro/tile计费；直接对Q32原始位移平方开整数根，不能把小步先截成0逃费；亚q余量持久化。余额不足不提交位置/计费余量或生成欠账，速度置零。
- 每Tick按**实际非零突触数**支付配置维护，加基础代谢，耗尽即死亡并进入原实际遗骸回收。配置非采集角色无资金也不免费冷启动存活；Forager原首次局部摄食激活边界仍保留，未改称完整饥饿开局生态。
- 自然复制仍是同一亲本实付/真实脑/确定突变/私有谱系路径，按角色的显式`natural_reproduction`许可执行，非Forager不会偷用Forager摄食率。没有新适应度或影子仿真。依赖DAG不变：neural精确支付→rts权威世界→原runtime/Actor与Lab→HTTP/SDK。

定向证据见同一Page/阶段报告：五种非采集原型用真实初始阵营库存闭合扣款/收入，供给枯竭死亡无欠费，真实命令驱动运动及小步余量/拒付原子性，Striker真实建槽/排队/付款后共享出生，候选回滚与快照重放同根；真实认证Lab请求同时启用Forager和Striker。旧检查没有删掉，只把其夹具改为当前角色模型；长Actor用例只编译、不在本阶段重跑。

尚不是完整物质/生命模型：未覆盖生物合成/生长与修复/攻击/分泌成本、CTRNN/ANN独立运算计价、野生供能、贫营养自主觅食、物理物流半径、自然后天经验遗传及长期平衡；不把HP或躯体造价臆换能量。上述无冻结数值，需继续显式规则设计而不是伪造规范。浏览器/GPU/PG/断电/24h和全面门禁仍待主体齐全后集中验收。

## 第十九批历史：权威自然谱系进入真实Lab（当前配置以第二十批为准）

原设计自然繁殖意图与来源见[上一批](RYN-1_Natural_Mitosis_2026-11-17.md)。本批优先补上过去Lab把基因目录当谱系的断链，不捏造未核定的多角色代谢数值。

### 可执行入口

- Rust：`rynvek_rts::lab::run_experiment`，`VariantSpec.ecology`显式传入现有Forager契约；`policy: "ecology"`只调用已有合法采集和建孵化槽命令，不训练单位、不生成适应度、不注入个体能量。这是有脚本控制器干预的觅食/设施实验，不称自主食物发现。
- HTTP：`POST /api/v1/lab/experiments`，`GET /api/v1/lab/experiments/{exp_id}`结构化状态，`GET .../{exp_id}/parquet?table=runs|metrics|lineage|energy`下载私有结果。状态和下载统一LabRead/所有者校验；非所有者/未知均404，结果`private, no-store`。此状态端点是现有规范20路由之外的扩展，不伪造新规范编号。
- TypeScript：`@rynvek/lab`的`LabClient.getExperiment/downloadAll`、`lineageForest/descendants`、`energyBalance`；扫描轴支持显式生态契约。Parquet解码仍由调用方Arrow实现负责。
- Python：`rynvek.lab.EcologyContract/RoleEnergyContract`、`Variant(ecology=...,policy="ecology")`、`ExperimentResult.energy`及原生四表接口；本批仅原生编译和Python语法检查，未运行Python/pyarrow完整路径。

真实HTTP/Rust请求示例（数值是**本批快验用实验参数**，不是原规范冻结或平衡推荐值）：

```json
{"name":"自然家谱","seeds":[57],"ticks":600,"sample_every":300,
 "variants":[{"label":"自然复制","policy":"ecology","ecology":{
 "basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000}}]}
```

### 当前事实与数据边界

- `lineage`现在按真实run/entity输出节点，父ID、出生时间、父子基因指纹和亲本实付q均来自不可变自然出生事实。不从slug/目录推测关系。初始、设施/虫潮等无自然亲代事实的节点是`initial`/`spawn`根，不冒充完整有性/定向繁育谱系。
- 每Tick收集节点，不随指标采样丢失自然出生；不可变事实补足同Tick已死亡的自然子代。当前并未承诺捕获同Tick已死亡的非自然spawn节点。
- `runs`新增自然出生数/自然因果链最大代数（不混入虫潮生成代数）；`energy`对全部活体及遗留尸体账本求精确u128总量，以Arrow/Parquet `Decimal128(38,0)`输出。关闭生态时明确`enabled=false`，不能当作零能量验收。
- 个体生命账边界：`received - transferred = external_received = stored + dissipated + exported + returned`。亲子内部转账只扣一次；此式**不是环境/设施/阵营/全局完整热力学证明**。其他角色目前仍可能记录未供能诊断成本，未升级为完整生命经济。
- TypeScript能量分析只接收bigint/精确十进制字符串，拒绝number静默丢精度；规范基因指纹含slug和代数，`genomeIdentityCurve`只是身份数量，不冒充遗传距离/无偏多样性。
- 第十九批当时使用`rynvek-arena/6` / `CURRENT_6`，现仅第二十批/7。出生事实补充faction/unit/slug，随原ECOL规范根、快照、WAL/Actor路径走，不增公开观测泄露。活体及自然父代的出生元数据/基因身份有交叉校验；已死亡初始根的历史身份仍由原权威根认证，不声称能独立复算其已丢弃基因。

### 依赖DAG与快验

`neural能量/遗传 → rts权威世界/自然出生 → rts::lab四表 → server CAS/私有API → TS/Python消费`。无新crate依赖，无反向引用，无并行假仿真器；原始附件、旧工作区、缓存及Git历史保留。

只执行改动所需编译/类型和定向快验：真实地图600 Tick产生至少连续两次自然代际、亲子基因和支付链、个体总账、确定Parquet、私有HTTP作业与所有者隔离、TS消费；具体结果/最终提交见同一项目Page与交付记录。不运行workspace/Clippy/license/SBOM/90秒soak，不借旧CI成功冒充新HEAD验收。

## 下一阶段与剩余实际缺口（按依赖推进，不逐项等用户决策）

| 顺序 | 可执行工作 | 完成时应有的实际证据 |
| --- | --- | --- |
| 已推进，仍有边界 | 第二十批已统一六角色配置，接通其他角色真实供能及运动/突触维护；继续补未覆盖生命成本 | 生物合成/生长、攻击/修复/分泌、野生与非脉冲计算等仍需显式规则；不冒充物质总守恒 |
| 下一高价值主体阶段 | W3规则消费与Lab真实交互工作台 | 自然谱系真实图、能量曲线、可配置遗传规则、实验进度/结果；明确控制器干预，实际多代数据非虫潮代数 |
| 主体缺口 | Studio通用权威编辑/挂载；Arena完整交互/回放 | 使用原Actor/权限/规范根/WAL，不设孤立演示状态；真实入口操作与恢复可重现 |
| 主体缺口 | Hub registry/agent/audio、C# SDK、音频接入与神经声化 | 当前模型端到端消费、真实设备/服务边界和隐私；非占位API |
| 主体完成后集中 | 集成门禁、长期种群/资源稳定、性能、浏览器、硬件GPU、真实PG、断电、24小时 | 届时真实执行记录，不复用旧CI截图；现有网络策略不绕过 |

本阶段不宣称项目、全生态、W3/Lab UI或所有生命周期成本已经完成。每个可交付阶段普通commit/push后继续上述缺口，只有真实阻塞才请求最小用户动作。
