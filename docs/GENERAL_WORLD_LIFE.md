<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 通用 World 的有来源生命场（实现剖面）

阶段47从干净 `a22fbe5` 继续，在阶段46单 Chunk 圆形刚体 World 上接入**有来源**营养与可选付费 LIF。这不是把 Arena 生态/Tier1 迁入通用 World，也不是声明式任意组件或统一权限 Actor。原 Tick 事务、迷雾、权限与生产 fence 未放宽。

阶段48已复用同一有来源账户接入[声明组件/受控规则计价](GENERAL_WORLD_COMPONENT_RULES.md)，下文“未接规则”保留为阶段47历史边界；当前有规则身体与LIF互斥，不双写/双记。

## 账本与来源

复用原 `rynvek-neural::metabolism::EnergyAccount` 与同一内部量子（`ENERGY_Q_PER_FIELD_RAW=15625`），不另造第二套能量单位。活体创世时为空账，能量只来自配置中的明确 Nutrient 沉淀，以及随后从所在格**先收款再等额扣场**的摄取；禁止先扣后钳，也禁止把不足的格值补成正库存。世界级 Biomass/Flux/Neurogel 保持 0，不写入阵营收获，避免与 Arena 双记。

INV-03 仍是当帧 harvest 恒等式：摄取 raw 记入 `nutrient_consumed` 与 `biomass_harvested`；死亡或命令回收把残骸整 raw 返还当前格，**不**记 `carcass_reclaimed`，以免回场再被算一次采集。亚 raw 与格已满装不下的余量只能记热耗，不抬高通道上限、不写到别的格。

反应扩散仍按原 10Hz 门控运行（Nutrient 带扩散与衰变）。场步导致的营养减少记入 `env_dissipated_q`；营养总量若在无摄取/返还的场步中增加，则整 Tick 拒绝。闭世界核对为：

`nutrient_energy_q(field) + env_dissipated_q + retired_closed_q + Σ(stored+dissipated+exported+transferred) == genesis_energy_q`

其中 `returned_q` 已回到场，不再另加；退役账户在删除前列入 `retired_closed_q`。无维护需求的空仓不是死亡；只有实际基础代谢把库存耗尽才回收。命令 Despawn 走同一残骸返还，避免直接丢掉库存。

## 实现剖面 `rynvek.general-sourced-life.v1`

下列数值是实现选择，不是规范冻结原值：每 Tick 最多摄取 0–256 raw、维护热耗 0–256 raw 当量、回收比 0–100。可选共享基因组（隐层 1–8、权重长度 `hidden*(16+6)`），无可塑性。LIF 只在成功 `try_charge_spikes` 且本 Tick 有脉冲时写入加速度；欠费或无脉冲将加速度清零，避免沿用上帧意图做无成本推进。加速度比例（Q8.8 的 1.0 → 1 世界单位/秒²，再钳到 ±32）同属实现选择。控制器/声明规则主体仍待后续阶段。

纯物理世界快照仍是原四块（CMIT/COMP/FILD/RNGS），无 LIFE、无 NEUR，既有碰撞示例与测试保持有效。生命世界追加 LIFE；启用神经时再带原 NEUR。这不是旧格式静默回退。配置里的可选 `life` 字段是加法扩展，缺省即为纯物理。

## 可执行入口

输入见 `examples/worlds/general-life.world.json`；模式见 `protocols/schemas/general-world.schema.json`。输出路径必须不存在。

```sh
cargo run -p rynvek-cli --locked -- world build \
  --config examples/worlds/general-life.world.json \
  --id world.general.life --out /tmp/general-life.rworld
cargo run -p rynvek-cli --locked -- world run \
  --package /tmp/general-life.rworld \
  --world-id 02020202020202020202020202020202 --ticks 4 \
  --snapshot /tmp/general-life-genesis.rsnap --journal /tmp/general-life.rjournal
cargo run -p rynvek-cli --locked -- world recover \
  --snapshot /tmp/general-life-genesis.rsnap --journal /tmp/general-life.rjournal
```

`world run`/`recover` 的 JSON 在启用生命时包含账户摘要与创世能量；纯物理世界 `life` 为 null。恢复仍走原创世快照与逐提交 WAL，不是只打印最终内存。

## 明确未做

未声明任意组件、未接入受控规则/QuickJS、未做统一权限 Actor、未迁 Arena 生态或 Tier1、未做多 Chunk/三维、未改生产 fence 去迁就旧 Actor 夹具。场衰变是环境热耗，不是完整宇宙能量定理或材料化学。未做真实掉电；进程内撕裂与回滚不能代替。
