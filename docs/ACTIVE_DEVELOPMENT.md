<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 持续开发与可执行缺口

更新时间：2026-11-17，中国标准时间 UTC+8。旧M0–M8标签不是全项目完成证明；当前仍在主体功能阶段。

## 第二十一批：W3自然复制规则实消费与真实Lab工作台（从main 990e52d继续）

**当前唯一模型：`rynvek-arena/8` / `CURRENT_8`。** 每个角色增加必填`replication_mutation_rate_permille`（0–1000）；Rust/HTTP/TS/Python统一，不兼容旧平铺或缺字段格式。此值进入原ECOL/规范根、快照/WAL与自然复制，不只是实验回显。50‰只是既有工程取值；工作台预填项均标明实验建议，不冒充原规范冻结数值。真实0‰消融不改变亲本付款、出生阈值或权威谱系；原有定向繁育10–250‰限制留在其自身入口，不再暗中覆盖自然规则的0‰。

### 已挂载、可操作的真实入口

- 启动现有server与web-platform开发服务，打开`/lab`；Arena页头也提供“Lab 实验台”链接。根路由按应用动态挂载：Lab不初始化Arena/WASM练习世界，不生成替代数据。复用现有`@rynvek/ui`的主题、Panel、Button和键盘Tabs。
- 输入真实服务origin和有Lab权限的Bearer（仅React/客户端内存，输入框连接后清空，无URL/localStorage/sessionStorage落盘）。可配置六角色成本/自然繁殖/突变、真实多seed、突变网格及0‰消融，预览并提交同一个`ExperimentSpec`。可载入已有私有实验，追踪排队/运行/失败/完成及真实run进度；停止本地追踪不冒称服务端取消。
- 实际四表`runs/metrics/lineage/energy`逐页查看、下载原Parquet；新增`GET /api/v1/lab/experiments/{exp_id}/rows?table=…&offset=…&limit=…`，复用LabRead/所有者校验，只从该实验CID的**同一CAS Parquet**解码，不重跑仿真。单页1–500；未知/他人404，`private,no-store`。u64/i64/Decimal128以十进制字符串输出，不能经JSON浮点损失精度。
- 由真实`runs`选择变体/seed，读取实际谱系图、节点父子基因/付款事实、代际/后代数，以及样本时间滑块、BigInt个体生命账平衡和能量曲线。图上代数来自自然事实，不把虫潮序号当自然繁殖；曲线只做显示比例，不用浮点复算能量。
- 断开身份、离开页面销毁请求/凭据/私有图表；旧身份延迟响应不能重新附着。往返缓存恢复重新挂载空身份，不复活旧结果。

### 范围、依赖与直接证据

依赖DAG：`角色W3参数 → 原自然实付/变异/谱系 → 原Lab四表 → CAS Parquet → 私有rows/原下载 → @rynvek/lab精确消费 → React /lab`。只把已有`bytes`提升为arrow可选生产依赖；没有影子遗传引擎、免费能量或UI伪实验。设计依据仍是[原设计](Rynvek_Project_Documentation_v1.0.md)的W3/Lab/现有React外壳，与上一批自然复制原意；本批不声称任意RuleIR/rpack/后天经验遗传已实现。

必要快验：server编译；真实0‰自然子代与亲本等基因（除slug/代数）且规则改根/1001拒绝；真实认证HTTP同时验证四表分页/CID/大整数/非所有者404/超限400；CAS分页2^96与u64::MAX不丢精度；SDK身份/边界消费、React真实提交分析/切换清空/延迟隔离、`/lab`真实挂载和往返缓存重挂载、原Arena挂载；TS类型、Python语法和Vite实际分包构建。不是实际受限浏览器交互验收；未重编浏览器WASM、未跑整workspace/Clippy/license/SBOM/soak或轮询CI，待主体齐全集中验收。

明确边界：服务端现有作业目录仍是进程内，重启后的作业发现/取消/持久任务治理未补；分析每张表最多5000行，图最多显示200节点并明示截断（完整分页/Parquet仍可取），不是无限规模工作台。`ecology`控制器仍有实际采集/建槽命令干预，不称自主觅食；只是W3自然突变的权威参数切片，不是通用规则编辑。个体账平衡不是全热力学证明；生长/合成/攻击/修复/分泌等成本继续欠项。浏览器、GPU、PG、断电、24h未验且不绕既有网络策略。

## 第二十批历史：配置角色真实供能、运动和突触维护（从main 07105d8继续）

当前入口仍为真实世界创建`POST /api/v1/worlds`的`ecology`字段、Lab变体的同名字段，以及Rust `EcologyContract`。TypeScript/Python同步采用**唯一当前角色表**；旧平铺Forager三参数不再接受，不留别名/迁移。第二十批当时为`rynvek-arena/7` / `CURRENT_7`，当前以第二十一批/8为准，私有BodyEnergy新增运动小数余量，随原规范根、快照/WAL恢复。

```json
{"roles":{
  "unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"replication_mutation_rate_permille":50},
  "unit.striker":{"basal_cost_micro_per_tick":100,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":100,"synapse_maintenance_micro_per_tick":1,"natural_reproduction":false,"replication_mutation_rate_permille":50}
}}
```

数值仅为**本批真实HTTP快验参数，不是原规范冻结值或平衡推荐**。以上示例已补入第二十一批字段，当前所有七项角色参数都必须显式填写；两个新增成本的零值明确表示该实验关闭此项，不能宣称完整生命成本。`roles`可选用六种真实单位原型，但不能为空；野生个体不使用玩家阵营供应契约。未配置角色仍为原诊断口径，不伪称全体已预算化。

实际行为：

- 玩家Forager继续等额扣局部营养摄食；其他五种配置角色使用同一现有Biomass账本补给，按实体序确定分摊。只转移实际可付且身体能接收的整milli，每milli=4000q，满账/余额不足不先扣款。此为原RTS阵营共享池工程规则，不声称新增了有物流半径的物理输运；原开局库存也是明确来源，未新造身体能量。
- 补给目标至少覆盖配置出生投入、维护/运动预留及既有脉冲模型下一Epoch上界；目标只是在原池内的预算，不是额外能量。允许自然繁殖时再预留一份实际出生投入，否则不自动自然出生。生产完成时对**所有配置角色**执行真实出生付款；欠款队首停在最后Tick，不弹队或耗序号。
- 脑仍用原候选真实脑原子支付。运动先求导航/ORCA/滑墙后的**实际位移**，再按micro/tile计费；直接对Q32原始位移平方开整数根，不能把小步先截成0逃费；亚q余量持久化。余额不足不提交位置/计费余量或生成欠账，速度置零。
- 每Tick按**实际非零突触数**支付配置维护，加基础代谢，耗尽即死亡并进入原实际遗骸回收。配置非采集角色无资金也不免费冷启动存活；Forager原首次局部摄食激活边界仍保留，未改称完整饥饿开局生态。
- 自然复制仍是同一亲本实付/真实脑/确定突变/私有谱系路径，按角色的显式`natural_reproduction`许可执行，非Forager不会偷用Forager摄食率。没有新适应度或影子仿真。依赖DAG不变：neural精确支付→rts权威世界→原runtime/Actor与Lab→HTTP/SDK。

定向证据见同一Page/阶段报告：五种非采集原型用真实初始阵营库存闭合扣款/收入，供给枯竭死亡无欠费，真实命令驱动运动及小步余量/拒付原子性，Striker真实建槽/排队/付款后共享出生，候选回滚与快照重放同根；真实认证Lab请求同时启用Forager和Striker。旧检查没有删掉，只把其夹具改为当前角色模型；长Actor用例只编译、不在本阶段重跑。

尚不是完整物质/生命模型：未覆盖生物合成/生长与修复/攻击/分泌成本、CTRNN/ANN独立运算计价、野生供能、贫营养自主觅食、物理物流半径、自然后天经验遗传及长期平衡；不把HP或躯体造价臆换能量。上述无冻结数值，需继续显式规则设计而不是伪造规范。浏览器/GPU/PG/断电/24h和全面门禁仍待主体齐全后集中验收。

## 第十九批历史：权威自然谱系进入真实Lab（当前配置以第二十一批为准）

原设计自然繁殖意图与来源见[上一批](RYN-1_Natural_Mitosis_2026-11-17.md)。本批优先补上过去Lab把基因目录当谱系的断链，不捏造未核定的多角色代谢数值。

### 可执行入口

- Rust：`rynvek_rts::lab::run_experiment`，`VariantSpec.ecology`显式传入现有Forager契约；`policy: "ecology"`只调用已有合法采集和建孵化槽命令，不训练单位、不生成适应度、不注入个体能量。这是有脚本控制器干预的觅食/设施实验，不称自主食物发现。
- HTTP：`POST /api/v1/lab/experiments`，`GET /api/v1/lab/experiments/{exp_id}`结构化状态，`GET .../{exp_id}/parquet?table=runs|metrics|lineage|energy`下载私有结果。状态和下载统一LabRead/所有者校验；非所有者/未知均404，结果`private, no-store`。此状态端点是现有规范20路由之外的扩展，不伪造新规范编号。
- TypeScript：`@rynvek/lab`的`LabClient.getExperiment/downloadAll`、`lineageForest/descendants`、`energyBalance`；扫描轴支持显式生态契约。Parquet解码仍由调用方Arrow实现负责。
- Python：`rynvek.lab.EcologyContract/RoleEnergyContract`、`Variant(ecology=...,policy="ecology")`、`ExperimentResult.energy`及原生四表接口；本批仅原生编译和Python语法检查，未运行Python/pyarrow完整路径。

真实HTTP/Rust请求示例（已更新当前角色契约，数值是**快验用实验参数**，不是原规范冻结或平衡推荐值）：

```json
{"name":"自然家谱","seeds":[57],"ticks":600,"sample_every":300,
 "variants":[{"label":"自然复制","policy":"ecology","ecology":{
 "roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"replication_mutation_rate_permille":50}}}}]}
```

### 当前事实与数据边界

- `lineage`现在按真实run/entity输出节点，父ID、出生时间、父子基因指纹和亲本实付q均来自不可变自然出生事实。不从slug/目录推测关系。初始、设施/虫潮等无自然亲代事实的节点是`initial`/`spawn`根，不冒充完整有性/定向繁育谱系。
- 每Tick收集节点，不随指标采样丢失自然出生；不可变事实补足同Tick已死亡的自然子代。当前并未承诺捕获同Tick已死亡的非自然spawn节点。
- `runs`新增自然出生数/自然因果链最大代数（不混入虫潮生成代数）；`energy`对全部活体及遗留尸体账本求精确u128总量，以Arrow/Parquet `Decimal128(38,0)`输出。关闭生态时明确`enabled=false`，不能当作零能量验收。
- 个体生命账边界：`received - transferred = external_received = stored + dissipated + exported + returned`。亲子内部转账只扣一次；此式**不是环境/设施/阵营/全局完整热力学证明**。其他角色目前仍可能记录未供能诊断成本，未升级为完整生命经济。
- TypeScript能量分析只接收bigint/精确十进制字符串，拒绝number静默丢精度；规范基因指纹含slug和代数，`genomeIdentityCurve`只是身份数量，不冒充遗传距离/无偏多样性。
- 第十九批当时使用`rynvek-arena/6` / `CURRENT_6`，现仅第二十一批/8。出生事实补充faction/unit/slug，随原ECOL规范根、快照、WAL/Actor路径走，不增公开观测泄露。活体及自然父代的出生元数据/基因身份有交叉校验；已死亡初始根的历史身份仍由原权威根认证，不声称能独立复算其已丢弃基因。

### 依赖DAG与快验

`neural能量/遗传 → rts权威世界/自然出生 → rts::lab四表 → server CAS/私有API → TS/Python消费`。无新crate依赖，无反向引用，无并行假仿真器；原始附件、旧工作区、缓存及Git历史保留。

只执行改动所需编译/类型和定向快验：真实地图600 Tick产生至少连续两次自然代际、亲子基因和支付链、个体总账、确定Parquet、私有HTTP作业与所有者隔离、TS消费；具体结果/最终提交见同一项目Page与交付记录。不运行workspace/Clippy/license/SBOM/90秒soak，不借旧CI成功冒充新HEAD验收。

## 下一阶段与剩余实际缺口（按依赖推进，不逐项等用户决策）

| 顺序 | 可执行工作 | 完成时应有的实际证据 |
| --- | --- | --- |
| 已推进，仍有边界 | 第二十批已统一六角色配置，接通其他角色真实供能及运动/突触维护；继续补未覆盖生命成本 | 生物合成/生长、攻击/修复/分泌、野生与非脉冲计算等仍需显式规则；不冒充物质总守恒 |
| 已推进，仍有边界 | 第二十一批已挂载Lab真实工作台并消费W3自然突变规则；一般规则编辑/持久任务治理待补 | 真四表、自然谱系图、BigInt能量曲线、seed/网格/消融、私有进度/下载已接通；规模上限/控制器干预明确 |
| 下一高价值主体阶段 | Studio通用权威编辑/挂载，随后Arena完整交互/回放 | 使用原Actor/权限/规范根/WAL，不设孤立演示状态；真实入口操作与恢复可重现 |
| 主体缺口 | Hub registry/agent/audio、C# SDK、音频接入与神经声化 | 当前模型端到端消费、真实设备/服务边界和隐私；非占位API |
| 主体完成后集中 | 集成门禁、长期种群/资源稳定、性能、浏览器、硬件GPU、真实PG、断电、24小时 | 届时真实执行记录，不复用旧CI截图；现有网络策略不绕过 |

本阶段交付当前有界Lab工作台，不宣称项目、全生态、W3一般规则/持久作业治理或所有生命周期成本已经完成。每个可交付阶段普通commit/push后继续上述缺口，只有真实阻塞才请求最小用户动作。
