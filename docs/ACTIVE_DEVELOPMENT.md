<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 持续开发与可执行缺口

更新时间：2026-11-17，中国标准时间 UTC+8。旧M0–M8标签不是全项目完成证明；当前仍在主体功能阶段。

## 第十九批：权威自然谱系进入真实Lab（从main c9691a4继续）

原设计自然繁殖意图与来源见[上一批](RYN-1_Natural_Mitosis_2026-11-17.md)。本批优先补上过去Lab把基因目录当谱系的断链，不捏造未核定的多角色代谢数值。

### 可执行入口

- Rust：`rynvek_rts::lab::run_experiment`，`VariantSpec.ecology`显式传入现有Forager契约；`policy: "ecology"`只调用已有合法采集和建孵化槽命令，不训练单位、不生成适应度、不注入个体能量。这是有脚本控制器干预的觅食/设施实验，不称自主食物发现。
- HTTP：`POST /api/v1/lab/experiments`，`GET /api/v1/lab/experiments/{exp_id}`结构化状态，`GET .../{exp_id}/parquet?table=runs|metrics|lineage|energy`下载私有结果。状态和下载统一LabRead/所有者校验；非所有者/未知均404，结果`private, no-store`。此状态端点是现有规范20路由之外的扩展，不伪造新规范编号。
- TypeScript：`@rynvek/lab`的`LabClient.getExperiment/downloadAll`、`lineageForest/descendants`、`energyBalance`；扫描轴支持显式生态契约。Parquet解码仍由调用方Arrow实现负责。
- Python：`rynvek.lab.EcologyContract`、`Variant(ecology=...,policy="ecology")`、`ExperimentResult.energy`及原生四表接口；本批仅原生编译和Python语法检查，未运行Python/pyarrow完整路径。

真实HTTP/Rust请求示例（数值是**本批快验用实验参数**，不是原规范冻结或平衡推荐值）：

```json
{"name":"自然家谱","seeds":[57],"ticks":600,"sample_every":300,
 "variants":[{"label":"自然复制","policy":"ecology","ecology":{
 "basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000}}]}
```

### 当前事实与数据边界

- `lineage`现在按真实run/entity输出节点，父ID、出生时间、父子基因指纹和亲本实付q均来自不可变自然出生事实。不从slug/目录推测关系。初始、设施/虫潮等无自然亲代事实的节点是`initial`/`spawn`根，不冒充完整有性/定向繁育谱系。
- 每Tick收集节点，不随指标采样丢失自然出生；不可变事实补足同Tick已死亡的自然子代。当前并未承诺捕获同Tick已死亡的非自然spawn节点。
- `runs`新增自然出生数/最大实际代数；`energy`对全部活体及遗留尸体账本求精确u128总量，以Arrow/Parquet `Decimal128(38,0)`输出。关闭生态时明确`enabled=false`，不能当作零能量验收。
- 个体生命账边界：`received - transferred = external_received = stored + dissipated + exported + returned`。亲子内部转账只扣一次；此式**不是环境/设施/阵营/全局完整热力学证明**。其他角色目前仍可能记录未供能诊断成本，未升级为完整生命经济。
- TypeScript能量分析只接收bigint/精确十进制字符串，拒绝number静默丢精度；规范基因指纹含slug和代数，`genomeIdentityCurve`只是身份数量，不冒充遗传距离/无偏多样性。
- Arena仅当前`rynvek-arena/6` / `CURRENT_6`。出生事实补充faction/unit/slug，随原ECOL规范根、快照、WAL/Actor路径走，不增公开观测泄露。活体及自然父代的出生元数据/基因身份有交叉校验；已死亡初始根的历史身份仍由原权威根认证，不声称能独立复算其已丢弃基因。

### 依赖DAG与快验

`neural能量/遗传 → rts权威世界/自然出生 → rts::lab四表 → server CAS/私有API → TS/Python消费`。无新crate依赖，无反向引用，无并行假仿真器；原始附件、旧工作区、缓存及Git历史保留。

只执行改动所需编译/类型和定向快验：真实地图600 Tick产生至少连续两次自然代际、亲子基因和支付链、个体总账、确定Parquet、私有HTTP作业与所有者隔离、TS消费；具体结果/最终提交见同一项目Page与交付记录。不运行workspace/Clippy/license/SBOM/90秒soak，不借旧CI成功冒充新HEAD验收。

## 下一阶段与剩余实际缺口（按依赖推进，不逐项等用户决策）

| 顺序 | 可执行工作 | 完成时应有的实际证据 |
| --- | --- | --- |
| 下一优先 | 把现有Forager专属预算统一成显式角色供能/完整生命成本契约；先查原最终意图，不猜“规范数值” | 其他角色真实资源支付、移动/突触维护/生物合成成本，失败原子性与守恒小快验；不向未供能者免费注能 |
| 随后 | W3规则消费与Lab真实交互工作台 | 自然谱系真实图、能量曲线、可配置遗传规则、实验进度/结果；明确控制器干预，实际多代数据非虫潮代数 |
| 主体缺口 | Studio通用权威编辑/挂载；Arena完整交互/回放 | 使用原Actor/权限/规范根/WAL，不设孤立演示状态；真实入口操作与恢复可重现 |
| 主体缺口 | Hub registry/agent/audio、C# SDK、音频接入与神经声化 | 当前模型端到端消费、真实设备/服务边界和隐私；非占位API |
| 主体完成后集中 | 集成门禁、长期种群/资源稳定、性能、浏览器、硬件GPU、真实PG、断电、24小时 | 届时真实执行记录，不复用旧CI截图；现有网络策略不绕过 |

本阶段不宣称项目、全生态、W3/Lab UI或所有生命周期成本已经完成。每个可交付阶段普通commit/push后继续上述缺口，只有真实阻塞才请求最小用户动作。
