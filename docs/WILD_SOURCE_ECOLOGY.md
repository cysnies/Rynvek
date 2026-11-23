<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 自然野生来源链：显式创世、摄食、结构、复制与遗骸

阶段44，从阶段43已推的 `56231b5` 继续。**主体仍未齐，项目未完成或正式发行。**

## 来源与不混淆的两条赛道

- 原设计 `Rynvek_Project_Documentation_v1.0.md` §1 要求生命选择通过真实能量、存活和死亡，不以人工适应度作为自然生态目标。
- 同文档 §5.5 的 RTS PvE 则明确有每1800 Tick的虫潮预算和 Damage/Survival 精英排序。该导演保留为原玩法，不把它的免费行政出生说成自然演化。
- 新的显式 `ecology.wild` 非空角色表启用**自然野生profile**。此profile不创建PvE导演，不追加死亡精英记录，也不累计野生伤害适应度。仍借用现有PvE容器、玩家/迷雾/胜负和原五场，并非已经实现没有RTS阵营的一般World。
- 创世种子使用原六兵种种子基因与脑构造，HP/盾仍是原RTS参数；初始内能和结构材料均为0。无营养会走原代谢死亡，不能按兵种造价生成免费尸体营养。不能把初始HP/盾称为物质或三维体积。

## 原链中的真实实现

1. 创世只接受显式合法角色和地图种子位置；不能越界、与其他野生种子重叠或落在不可通行地形/初始主脑占格。生成地图入口验证真实生成地形，自定义地图和恢复则校验实际地图，不另造一张图。
2. 原Tick首脑更新前，所有配置野生兵种按实体序从**当前所在格**实际扣 Nutrient。上限按角色显式配置，不依赖空能量脑先发出 Harvest；不重复进入玩家采集/阵营供给，不向阵营Biomass借钱。0摄食速率是显式饥饿对照，不是免费存活许可。
3. 原同一个 `EnergyAccount` 接收真实营养，再承担维护、脉冲/连续脑、运动、动作与规则实际Gas等费用。野生移动直接消费原脑 `a_move`，仍走原ORCA/导航/实际位移收费，不受玩家出生默认Hold封死，也不被虫潮攻击路标牵引。
4. 野生生长从**同一身体化学/能量预算**实转完整milli，并另付合成热耗；保留配置维护/脑/活动预算。`1 milli预算=4000q=256u`，`1 Nutrient raw=15625q=1000u`。这些是沿用现有Biomass等价量纲的单材料实验选择，不是新的元素化学或真实质能定律。已摄食营养不再扣第二遍，转出的预算也不能继续驱动动作。
5. `MaterialAccount.converted_u` 单列内部转入，`external_u` 仍只表示真正外源材料，不能把同一营养记成两份外源。材料继续影响原ORCA/攻击接触半径和原裁剪 `radius_milli` → FrameScene/WorldRenderer/Canvas2D，fog及私有权限没有放宽。
6. 长满才可自然复制，复用原单亲Philox突变、亲本实付内能/出生合成热耗、亲子材料与HP/盾拆分、邻格容纳、真实脑注册和不可变出生事实。不用玩家设施/人口；以独立显式野生存活上限代替，不绕过容量。亲子可再次摄食生长；失败不扣费、不占序号。
7. 饥饿/战斗/环境死亡复用唯一退役路径，**同一**材料和能量账进入遗骸。只返还当前格容得下的完整raw；满场/亚raw余量留在遗骸，后续重试不会重复归还。原沼泽再生和环境扩散仍存在，不冒充全世界封闭热力学证明。

## 显式契约与安全边界

当前 `EcologyContract` 必须同时给出 `roles` 和 `wild`；缺少wild的旧格式拒绝，不增加兼容分支。保留原RTS波次时，完整的非空玩家 `roles` 与下面这个 `wild` 对象一起提交：

```json
{"roles":{},"population_limit":0,"seeds":[]}
```

下面是可提交的完整自然野生契约示例（数值和坐标均为**可编辑实验建议，不是规范冻结值**；实际地图可能拒绝坐标或没有食物）：

```json
{
  "roles": {},
  "wild": {
    "roles": {
      "unit.forager": {
        "uptake_raw_per_tick": 128,
        "energy": {
          "basal_cost_micro_per_tick": 10,
          "reserve_ticks": 1,
          "birth_energy_milli": 10,
          "movement_cost_micro_per_tile": 1,
          "synapse_maintenance_micro_per_tick": 0,
          "attack_cost_micro_per_shot": 1,
          "repair_cost_micro_per_milli": 1,
          "secretion_cost_micro_per_field_raw": 1,
          "continuous_cost_micro_per_neuron_step": 1,
          "rule_cost_micro_per_gas": 1,
          "material_capacity_milli": 8,
          "material_growth_milli_per_tick": 4,
          "material_synthesis_micro_per_milli": 1,
          "birth_synthesis_milli": 1,
          "activity_reserve_milli": 1,
          "natural_reproduction": true,
          "replication_mutation_rate_permille": 50
        }
      }
    },
    "population_limit": 4,
    "seeds": [{"kind":"unit.forager","x":10,"y":10}]
  }
}
```

计算/规模安全边界：1–16初始种子、1–128存活上限、每角色0–256 raw/Tick摄食；不是新冻结平衡值。种子数不能超过上限，角色须完整显式价格且结构容量至少2 milli、增长速率大于0。非法/缺少/额外字段、重复格、非法角色和超界均拒绝。种子只有创世时放置一次；来源速率、野生角色、种子与种群上限目前在创世固定，`SetWorldParam("ecology.wild",...)` 不能偷偷重播种或切回免费导演。玩家既有角色组仍按原Epoch安全参数路径编辑。

## 实际入口、原四表与格式

- `/studio` 原私有世界创建JSON中给出上述契约，或 `StudioClient.create(title,mapSeed,ecology,signal)`。原已提交状态显示 `ecology_contract` 只读事实，保留owner/editor准入、其他身份404与no-store。点击原“在Arena运行/重连此已有世界”，不是另建practice，也不向链接带凭据。
- `/lab` 原工作台增加野生JSON与显式“填入野生实验建议（不加营养）”。建议按钮只生成可编辑配置，不加场、不另造运行器；可取消玩家角色得到纯野生身体记账对照，但容器仍有原玩家主脑。W3扫描/0‰消融覆盖玩家和野生启用自然复制角色的复制突变率。
- `@rynvek/lab` 导出当前 `WildEcologyContract`、`WildRoleContract`、`WildSeed`；原实验/持久Store/run/谱系/metrics/energy四表复用。自然野生谱系保留真实faction=255、parent/genome/generation和实付出生事实，不拿波次代数冒充自然谱系。
- 原Energy Parquet新增 Decimal128 `material_converted_u`，BigInt分析分别核对：材料received-transferred=external+converted=stored+returned；converted为完整milli且不得超过原能量exported的对应预算。原能量账独立核对，不能把converted当成额外外源叠加。UI明确展示内部转入。
- 快照 `rynvek-arena/14`，根配置域 `RYNVEK_ARENA_CONFIG_CURRENT_14`，Lab `rynvek-lab-kernel/7`。线上Arena JSON仍/2、WAL容器仍2。本批没有新线上可见字段，不改C#协议或原fog策略；旧快照/原始资料/历史不删除，不读旧格式兜底。

## 已运行的定向快验

代码提交 `5021737` 与 `329649e`。90个不同用例通过，新增10，重跑不累计：

| 定向范围 | 通过数 | 真实边界 |
| --- | ---: | --- |
| RTS `wild::tests` | 5 | 非Forager真实扣场、预算转材料/面积、付费EMIT/自主移动、亲子拆分/上限、饥饿/有限源、原Gas欠费原子拒绝、初始快照/真实Tick/文件WAL根重演、导演隔离 |
| RTS `lab_ecology`（arrow） | 4 | 原玩家回归、真实野生多代、四表Parquet、Decimal128转账与非法创世提交前拒绝 |
| Server指定野生HTTP用例 | 1 | 真实本机认证HTTP创建/私有读回、其他身份404、野生创世补丁422且根不变、越界/初始主脑格400 |
| 既有 `life_cost_tests` / `forager_uptake` | 14 / 3 | 原生命/结构/规则原子账与玩家首摄食不退化 |
| Lab TS包 | 42 | 当前严格契约、精确材料转入不冒充外源/不可重复用已转出能量等快速回归 |
| 原React Lab/Studio / Studio远端协作 | 14 / 7 | 原入口/只读创世响应、W3野生突变扫描、私有控制链快速回归 |

必要server库检查（含engine）、Lab/Studio build、web类型、diff检查通过；必要秘密/私人联系信息/敏感文件名模式筛查无命中，不冒充完整审计。开发过程曾纠正新测试的导入/现有WAL方法名，并真实重跑；初始编译失败不计通过。没有新增运行时依赖、CI全门禁、外部发行/部署、网络策略更改或完整安全认证。

## 未完成与未验

仍是单材料相对额定容量的二维面积模型，非完整元素化学、三维物理ECS、全部World插件/CPU配额的物理能量模型。一般World/TS-rpack、独立Agent赛道/受控实际智能、协作presence/离线多主等真实主体继续。Pi仅经MIT核对的设计参考，没有外部模型或支付授权。

真实浏览器WebGPU、硬件GPU、真实PG/多机断电/长稳未验；阶段41 llvmpipe只代表软件回读。Node22正式运行时、C#本批重编仍未验；Node24引擎警告不能算Node22验收。本批仅必要编译/类型与定向快验；主体齐后才集中全门禁/验收/安全/发布打磨，不因阶段结束自称全项目完成。
