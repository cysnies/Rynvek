<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 原16阶段内核的非RTS物理World

阶段46从干净 `83427c0` 继续。本段不是参数表追加：原 `SimulationEngine` 不再隐式创建四个静止坐标或500/200/50资源，而是实际持有原 `EntityRegistry` 与分页刚体组件列；没有阵营、主脑、RTS生产或免费生物营养。原Arena权威路径、fence、观察权限和迷雾未改变。

## 来源与明确的实现边界

最终《开发规范v1.0》§1.2规定 `rynvek-spatial` 的定点辛欧拉与碰撞，§2.1规定权威位置/速度的Q32.32，16阶段Phase09为KinematicsAndCollision，§6.2给出256×256单Chunk/128×128 CSR格。定点积分/接触核按规范分层放入 `rynvek-spatial::circle`，复用原唯一 `Q32_32` 乘除语义（乘法算术右移、除法向零截断）。原 `EntityRegistry`、`PagedComponentColumn`、`SpatialCsrIndex`、16阶段/30Hz时钟、10Hz五场、RYC2十域根、RYSS快照、RYJL日志、`EntityProvider`/`FrameScene`/`WorldRenderer`均实际复用。

原规范没有冻结圆形求解器、半径、质量区间、接触迭代数或实体容量。下列是显式版本化的**实现剖面** `rynvek.general-circle-ecs.v1`，不是规范原值或最终平衡：

| 项目 | 本段实际语义 |
|---|---|
| 范围 | 256×256二维单Chunk，最多128个圆形刚体，固定壁；不是任意组件/任意World |
| 分量 | 位置、速度、外加加速度、半径、正质量、fixed；原持久身份与稀疏generation/稠密列 |
| 数值 | 全权威Q32.32、i128中间运算、整数平方根；只读渲染时转f32 |
| 积分 | 每30Hz Tick四个120Hz辛欧拉子步，先速度后位置，除法向零截断 |
| 接触 | 无摩擦/无旋转的离散圆形弹性法向冲量，按质量与固定体逆质量分配；四轮位置修正 |
| 边界 | 半径0.25–2，质量1–1024，速度各轴±8，加速度各轴±32；圆心必须按实际半径留在壁内 |
| 最坏工作 | 每子步完整枚举至多8128对、四轮；不使用CSR的32邻居截断漏掉密集接触 |
| 拒绝 | 固定体互穿、修正剩余超64 raw、速度等越界则整个候选Tick回滚；不是无限迭代求解 |
| 结构 | 单次最多64条显式Spawn/Despawn/SetAcceleration，持久serial不复用，原墓碑延迟回收 |

这不是连续碰撞检测、任意凸体、角动量、关节、软体、三维、多Chunk或完整材料热力学。无摩擦弹性是求解目标，离散与定点舍入/位置修正不构成精确机械能守恒证明。加速度是明确外加驱动；本段没有把机械质量/外力、CPU配额或原宏观资源INV冒充完整生物能量账。创世刚体与本地所有者结构命令是外部编辑输入，不是自然生长/免费代谢路径。阶段47已在通用World接入独立的有来源生命场与可选付费LIF（见[GENERAL_WORLD_LIFE.md](GENERAL_WORLD_LIFE.md)），与Arena生态分账，不把机械质量或CPU配额冒充生物能量。

## 可执行入口

输入见 `examples/worlds/general-collision.world.json`；JSON Schema见 `protocols/schemas/general-world.schema.json`，动态的“圆心按实际半径留壁”仍由同一Rust验证器强制执行。

```sh
# 输出路径须不存在，工具不覆盖旧包、快照或日志。
cargo run -p rynvek-cli --locked -- world build \
  --config examples/worlds/general-collision.world.json \
  --id world.general.collision --out /tmp/general-collision.rworld
cargo run -p rynvek-cli --locked -- world run \
  --package /tmp/general-collision.rworld \
  --world-id 01010101010101010101010101010101 --ticks 4 \
  --snapshot /tmp/general-genesis.rsnap --journal /tmp/general.rjournal
cargo run -p rynvek-cli --locked -- world recover \
  --snapshot /tmp/general-genesis.rsnap --journal /tmp/general.rjournal
```

CLI实际调用原唯一包写/读器，产出RYWL；执行器仅接收严格schema/profile、唯一 `world.json`、128KiB以内的包，兼容范围固定 `>=0.1.0 <0.2.0`。附加脚本/依赖条目不忽略而是拒绝；不执行JS、shader、插件或URL。不是通用 `.rpack` 安装器，也没有自动发布Hub。

`world run`生成的是**创世**快照与每Tick真实sync_data日志，不是只把最终内存对象打印为“恢复”。输出包括实际身体列、状态根、原 `EntityProvider` 投影的 `FrameScene` 与 `gpuVerified:false`。`world recover`从文件解压/重建注册表与五场，并逐条原WAL重新执行命令/积分/碰撞，逐提交核对十域根。

原Worker命令 `GENERAL_WORLD {packageBytes}` 与 `EngineHost.loadGeneralWorld(Uint8Array)`明确安装到暂停、Tick0且无实体的本地世界。成功发布真实创世根；原STEP/START、原FrameScene/WorldRenderer继续消费真实位置与半径。远程Arena模式、已运行或已有实体的世界拒绝安装；不借此绕过服务端owner/editor或迷雾。

本段真实使用原GPU实例管线，但定向验收只运行了CPU/场景与原Worker原生命令处理，**没有**本批真实浏览器/硬件GPU消费证据。通用World尚无服务器Actor/多人在线入口；不要把本地所有者工具称为公开服务器World权限系统已交付。

## 事务、身份与恢复

- 命令在原Phase01进入候选；Phase09真实运动/碰撞后重建CSR；原Phase14不变量仍执行。任何阶段失败不发布时钟、结构、场、经济或根。计算后端克隆独立计数器，不共享候选可变状态。
- 状态根使用原RYC2十域聚合，绑定物理剖面/完整world_id、原注册表分配器/墓碑、全部刚体分量、场参数/当前缓冲/未来sources、时钟/场步与原宏观资源容量。未实现的神经/进化/可见性/事件域用明确的剖面常量，不伪称这些系统已启用。
- 原快照容器版本2与RYC2、COMP/FILD/RNGS/CMIT实际使用；不是Arena/15迁移或新私有存档格式。恢复重算实际领域根，不只验压缩包络；修改合法COMP或未来源项、即便重新打包有效容器，仍拒绝。禁止尾随数据；文件至多8MiB、解压至多16MiB；原快照解析器新增更紧调用者上限，既有默认不变。
- 原日志先写接受批次、再写Tick提交并经过sync_data才发布候选。I/O错误后必须丢弃writer并恢复，不能在未知落盘状态继续。原恢复器新增可选硬字节上限，本剖面32MiB；世界/创世基线/保留位/顺序/命令阵营/精确清单都核验，尾部未提交命令不执行，原撕裂写标志保留。未做真实掉电，不能以进程内模拟截断代替。
- 稠密swap-remove必须与原注册表对齐；求解顺序按持久ID，不按易变稠密槽位。`PICK_RESULT.entitySerial`现为稳定序号，不再返回会在删除后指向别人的denseIndex；Rust/TS同步，不做旧字段fallback。旧 `protocol_messages.json` 保留作历史资料，当前双端只消费 `protocol_messages_v2.json`，同时补上原主题命令/事件遗漏覆盖。

## 定向证据与下一缺口

实际命令、不同用例计数与本次提交记录见 `ACTIVE_DEVELOPMENT.md`。验证包含真实CLI子进程打包→运行→文件恢复/根与FrameScene相同、拒绝覆盖；不把未选中用例或重跑计入通过。Node24类型/单元检查不算Node22正式运行时验收。

阶段47已交付通用World的有来源生命场与可选付费LIF，仍不是声明组件或任意World。下一主体仍包括：通用World的声明组件/受控规则及统一权限Actor；TS AST、完整私有rpack精确签名依赖发现验签安装；多Chunk物理/迁移实际权威链；Agent独立赛道/实际受控智能；协作presence与离线多主等。一般组件参数、已有RTS、此次刚体链不能拼成“任意World已完成”。

主体齐后必须集中完整workspace测试、Clippy、license/SBOM、安全/运行时/发布打磨。Node22、C#本批重编、真实浏览器WebGPU/硬件GPU、真实PG多机断电/长稳仍欠；llvmpipe仍仅阶段41软件回读。整体项目**未完成、未达到正式发布验收**。
