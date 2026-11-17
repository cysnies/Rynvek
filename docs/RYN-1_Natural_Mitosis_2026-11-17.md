<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 亲本实付自然出生与遗传谱系（有界实现）

## 收尾先行

按用户t22u明确指示使用已认证GitHub CLI。PR2于2026-11-17 20:03:10中国标准时间正常merge commit，main当时为`9884225722b3de98fa0e8f823a237105a5fcd614`，完整包含`c26526e`及57项细提交。此前CI44071851032已于20:02:45成功。逐支证明main之外0提交后普通删除两条远端开发分支及剩余本地开发分支；20:03:31本地/远端heads均只剩main，工作树干净，PRIVATE=true。未force/admin/rebase/skip hooks或改写历史。从该main开始本批。

## 设计证据与当前规则

原始讨论`Rynvek_84cf0fa3d150.md` L54933–54948要求Energy Threshold、Genome Replication、Mutation During Replication、Parent Energy Splitting，并区分玩家生产、定向繁育与自然有丝分裂。项目文档v1.0 §1.1要求能量约束下自然存活/分裂、突触/拓扑复制突变，无人工显式适应度。

本批不将原定向繁育的伤害/寿命适应度排序带入自然出生。只覆盖已有完整供能/基础代谢契约的**显式PvE玩家Forager**；不猜测其他角色、野生或开局贫营养条件的供能。

没有新增平衡参数、繁殖模式或迁移开关。复用同一`ForagerEcologyContract.birth_energy_milli`：

- 子代投入 `B = birth_energy_milli × 4000 q`。
- 原下一Epoch神经预算及基础代谢预留之和为`R`。
- 母体最低保留`P = max(B,R)`，代谢支付后原池至少`B+P`才能出生。
- 资源盈余导出保留`B+P`，允许自然摄食逐Tick积累一份出生投入，而不是先把全部余量转走。

这是当前规则的明确工程定义，并非宣称原v1冻结了此繁殖门槛。没有新造摄食率、能量或适应度。出生成功时亲本原池实扣B、子代同额收入；不会再扣阵营Biomass。孵化槽出生仍独立使用既有真实Biomass生产池。

同一Tick的原存活序号列表每个最多出生一次，子代不递归参与该Tick的代谢/繁殖。仍须有现有RTS人口容量（含生产队列预留，来自已建成SpawningVat）、可容纳的邻格、可递增的序号/代数和至少能拆成两个存活个体的HP。邻格复用导航`step_cost`含边界/墙体/禁止切角，按y/x选第一个未被单位占据的位置，序号有序提交共享占位集合。

HP/护盾按整数拆分，余数留亲本，二者总量不增加，不能从自然出生免费复制满血战力。没有把HP假冒能量或发明HP→能量换算。

## 实际权威路径

`step_forager_uptake / step_harvest → step_metabolism支付实际脑/基础代谢 → try_mitosis → 原池try_transfer_to → spawn_unit_with_brain共享注册 → 私有births`。

- `EnergyAccount`新增`transferred_q`，与资源导出/热耗区分。转账前同时预检双方表示范围和亲本预留，失败时两账不改。守恒式加入真实对外转账。
- 单亲克隆基因组、代数checked+1，再复用既有权重/拓扑变异算子与现有默认50‰（不冒充原设计另行冻结的自然平衡）。Philox沿用原演化流，自然复制使用phase1，与定向繁育phase0隔离；键为world_seed/tick/预检后的子代序号。
- 候选真实`UnitBrain`先表达成功，才扣款和共享注册；失败不扣能量、占格或序号。不复制父代脑动态态/脉冲，不经过孵化队列或精英适应度。
- `MitosisBirth`保存亲本ID、出生Tick、父子代数、出生时双方基因指纹、投入q。随原ECOL入规范根及私有快照，父代死亡后保留。快照校验逐亲本转出总数等于其子代出生投入总数，并校验DAG序号、合法代数及接收账。
- 原ArenaRuntime候选事务、服务Actor提交及TickRecord/WAL路径直接运行相同权威Tick。观察与公开元数据不新增谱系/能量字段。

## 当前格式与验收边界

按t15u不背旧兼容负担：第十八批交付时仅支持`rynvek-arena/5` / `CURRENT_5`；现已由下一批`rynvek-arena/6` / `CURRENT_6`取代以纳入不可变出生元数据（见[持续开发记录](ACTIVE_DEVELOPMENT.md)）。当前拒绝旧格式及旧能量对象；不添加自动补字段、历史根仿造或迁移路径。通用容器/TickRecord协议识别未机械删除。未删除原始附件、旧工作区、缓存数据或git历史。未增加crate依赖或改变原DAG方向，新代码保留SPDX/简体注释。

按t18u/t22u本批只有必要编译与少量定向快验，详见最终Page/交付消息。未跑workspace完整门禁、90秒soak、license/SBOM；没有反复查看CI或称新HEAD已全面验收。原始CI成功只属于c26526e，不冒充本批新提交。

## 仍缺

- 其他单位及野生个体的有依据供能/自然生命周期契约；贫营养开局自主觅食；脱离RTS人口设施的开放规则生态实验入口。
- 自然复制的可配置遗传规则包、神经元数量变异、后天经验的自然遗传同化；当前只复用既有突触权重/边拓扑变异。
- 突触维护、移动、生长/生物合成等未核定的成本，不假称全部生命经济已计价或物质总量已证明。
- Lab真实谱系/能量四表导出及SDK查询已在下一批接通；真实交互式可视化、长期种群稳定性和资源预算验收仍缺。
- 浏览器、硬件GPU、真实PostgreSQL、断电及24小时长稳；本批未做GPU生命周期内核或上述验收，不绕网络策略。
