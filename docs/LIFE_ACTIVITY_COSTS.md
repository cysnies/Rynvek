<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 原生身体的真实动作成本（第三十九批）

本批闭合**已有动作**的发射、护盾再生/Weaver治疗、Spitter危害场沉积、设施出生/自然分裂合成。不是一般生长/物质模型或全生态完成声明。价格为显式实验设计选择，不是原规范已冻结的平衡值。

## 实际入口

- `/studio`创建`arena_pve`时提供完整`ecology.roles`；创建页初始JSON使用同一成本草案。已配置角色可在原参数编辑器修改`ecology.roles.unit.*`，经原权限/Actor Epoch边界/CRDT修订提交，再点「在Arena运行当前世界」。原Arena的真实生产、战斗、治疗链消费，不是第二个模拟器。需要Spitter/Weaver时显式配置它们，使用已有合法生产/基因/供给条件。
- `/lab`选择自然生命W3，启用所需角色，表单可改本批五项；原持久作业、扫描、四表、分页/Parquet继续使用。参数实体profile没有这些原生身体，仍`energy.enabled=false`，不伪造其生命账。
- 原API：`POST /api/v1/worlds`及`POST /api/v1/worlds/{id}/patches`；Lab为`POST /api/v1/lab/experiments`及其私有rows/parquet。原鉴权、所有者/定向准入不扩权。
- TS `@rynvek/lab.RoleEnergyContract`、严格`validateSpec`与精确`EnergyRow`；Python同名dataclass必填字段同步。C#仍可连接既有已配置世界，但现有创建方法未暴露生态参数；本批未扩展其生态创建/类型化Lab消费或重跑.NET。

建议草案（不是平衡承诺），可绑定任一显式玩家角色：

```json
{
  "basal_cost_micro_per_tick": 100,
  "reserve_ticks": 4,
  "birth_energy_milli": 1000,
  "movement_cost_micro_per_tile": 100,
  "synapse_maintenance_micro_per_tick": 1,
  "attack_cost_micro_per_shot": 100,
  "repair_cost_micro_per_milli": 1,
  "secretion_cost_micro_per_field_raw": 1,
  "birth_synthesis_milli": 10,
  "activity_reserve_milli": 10,
  "natural_reproduction": false,
  "replication_mutation_rate_permille": 50
}
```

五项都是必填u32；0明确关闭该费用/预留，不偷偷补旧值。基础代谢/出生投入仍必须正数。`activity_reserve_milli`只影响真实补给目标和Forager转出留存，不是新收入或每Tick免费发放。没有生态的标准RTS/排位、未配置角色/野生仍不受这些可选玩家角色契约收费；不能因此声称所有赛道生命成本已齐全。

## 逐项原子性与边界

| 真实动作 | 付款来源与计价 | 不发生/不足时 |
| --- | --- | --- |
| 发射，包括被闪避的一发 | 射手原身体池，`attack_cost_micro_per_shot × 4q`；Carapace一发多受体只收一次 | 无有效活目标、弹道阻挡、冷却/动作门控、射手已死不收。欠费整发拒绝，不启动新冷却、不造成伤害；实际Flux弹药与身体费在同一调用预检付款 |
| 护盾再生 | 自身池，真实恢复milli shield × repair价格 × 4q | 按缺损/速率/支付能力取小，满盾/尸体不收，不能付一个milli就保留余款 |
| Weaver治疗 | 活着且未耗尽的同阵营Weaver原池，真实恢复milli HP × repair价格 × 4q | 环境伤害先统一结算；死源不治疗。按受体/供体序号确定分配，多个供体最多填满原一份光环速率，不叠加无限疗量；满血不收 |
| Spitter危害分泌 | 发射后独立按实际容纳Q8.8 raw × secretion价格 × 4q | 饱和不收；攻击已付而分泌不足可减量至0，不退已发生的发射费。最小量子余款留原池。这里只消费已有15.0最大沉积，不假装EMIT通用分泌已接通 |
| 孵化设施出生 | 原阵营Biomass一次实扣`birth_energy_milli + birth_synthesis_milli` | 总额不足，队列停最后Tick，不扣任一部分、不出体。新体收入完整实扣额，立即将合成费耗散，启动池只含原birth投入，原生产材料造价另照旧实付 |
| 自然分裂 | 原亲本池转给子体birth投入，并耗散亲本合成费；保留原亲本预留 | 邻格、容量、序号、候选脑与总支付额都预检；失败不转账/耗散/分裂。HP/护盾仍拆分，不免费复制满血；冻结出生契约用于恢复校验 |

`1 micro = 4q`、`1 milli Biomass = 4000q`保持不变。Hazard不是Nutrient摄食能量，分泌的实验价格不冒称Nutrient raw的15625q映射。修复是现有HP/盾机制，不臆造实体质量/碳元素或体积成长。可选动作恰好耗尽原池时，后半Tick也走唯一死亡/遗骸路径，不留可补给复活的零池幽灵；该Tick实际发生的动作照实计账。

## 根、恢复、Lab与格式

`BodyEnergy.activity`四项`attack_q/repair_q/secretion_q/synthesis_q`是**总dissipated_q内的子账**，不可重复加入守恒式；死亡后完整随原遗骸保留。子账之和不得超过账户总热耗，出生时冻结合成费也与亲本实付子账交叉校验。ECOL进入原规范根/快照/WAL恢复，没有新公开脑/身体余额观测。

Lab energy四个同名列使用u128汇总和Arrow Decimal128(38,0)，覆盖活体与遗骸；UI/TS用BigInt/十进制精确字符串展示，不丢精度、不双算。原式仍是`received - transferred = external_received = stored + dissipated + exported + returned`。它不是环境/阵营/建筑/身体全域物质守恒证明。

Arena格式升为`rynvek-arena/11`与`CURRENT_11`；Lab `rynvek-lab-kernel/4`防旧run检查点混算。WAL/2记录框架不变。旧快照/旧合约不自动迁移，旧原始资料/CAS/Git历史保留，不建立旧格式兼容层。

## 本批必要直接验证

- 五个`life_cost_`原Engine动作边界用例，包括真源池供给后定向耗散夹具、欠费、残余、饱和、死源/满量、自然分裂体量拆分、精确耗尽退役、私有快照与真实文件WAL撕裂尾恢复。
- 一项真实建设/生产队列合成费边界；一项600 Tick原Lab生态实验真实自然谱系、合成子账/守恒与确定Parquet四表。
- 一项真正本机认证HTTP创建/创作修订/完整价目私有恢复、所有者隔离；受影响TS规格/精确列和React组件定向快验及必要类型编译。HTTP验证是协议/恢复，非真人浏览器操作或整条Node客户端Worker端到端重跑。
- 没有workspace、Clippy、license、SBOM、soak、CI轮询；无新依赖、公开发行、外部部署或策略绕过。文件撕裂尾与内存Store HTTP不等于真实硬杀/PG多机/断电灾备。浏览器/GPU/硬件听验/PG实库/并发SQL/长稳及Python扩展/pyarrow与.NET本批未验。

下一优先正式音频采样与空间化、视觉registry/制品；一般生长/材料合成、自主通用EMIT、规则/连续神经计算计价、野生供能仍是实质欠项。不以收费字段或已覆盖四类动作声称全产品完成。
