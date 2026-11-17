<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Studio一般参数模型 → 持久Lab/W3 → 原四表

第三十八批，2026-11-18，中国标准时间UTC+8。这里只闭合**有界参数实体实验**，不冒称任意W3/TS/rpack、场/邻居/物理ECS或全产品完成。

## 真实入口

1. 原`/studio`用正式WorldEdit身份创建/连接世界，真实创建模板、模块、实例及绑定。沿用[PARAMETER_ENTITY_RULES.md](PARAMETER_ENTITY_RULES.md)原编译器/CRDT/Actor/规则执行。
2. 原`/lab`连接本人服务与LabSubmit/LabRead凭据，在「实验模型」选「Studio一般参数实体」。输入源世界ID，显式「导入有权限的Studio参数模型」；源读取另须本人WorldEdit及世界所有者/当前定向编辑准入，LabRead本身不能读世界源码。
3. 检查/编辑真实模型JSON及明确outputs，填实例、`param.*`与不重复原始整数值、多seed、Ticks和采样间隔。按原「提交真实实验」进入相同持久作业；未知提交结果固定原UUID/规格对账，不盲建第二个任务。
4. 原发现/状态/取消/重试/恢复和四表下载不另起系统。「加载真实谱系与能量分析」现在读取同一CAS的全部四表，也显示参数真实轨迹；Run从`runs`选择，不从空谱系猜测。参数profile不画零能量或伪谱系图。模型/凭据/结果仅内存，断开清空并阻断迟到响应，断开不等于取消服务端作业。

模板默认值是新实验初态，**不是当前世界动态态快照**。源更新/撤权不修改已捕获的不可变模型或进行中的作业；撤权阻止下一次导出，不承诺追回曾获准阅读的源码副本。导入只含参数模板/实例/被绑定规则/绑定，不复制地形、原生身体、基因或凭据；默认选择排序前16个state输出，在JSON可明确改选。

## 同一权威实现，不造toy runner

`VariantSpec.mode="parameters"`、`policy="idle"`、`players=0`并嵌入`model`。该profile禁止ecology和原身体动作模块；pve/duel禁止偷偷携带model。

`rynvek.lab-parameter-model.v1`字段为`schema/templates/instances/rules/bindings/outputs`；rules值是原始有界RuleGraph JSON字符串，不是脚本、包路径或URL。模板/实例/绑定仍用原World/Rules结构，outputs是`{instance_id,component}`。示例完整源在`examples/lab/parameter-counter.json`；可直接用SDK提交，不必开一个空壳演示服务。

- 原编译器→RIR/写权限/类型/引用/重复writer/静态总gas校验→原`AuthoringState`与`ParameterState::reconcile`原子安装。
- `MatchMode::ParameterEntities`明确定义无阵营/原生身体；仍用原`ArenaWorld::step`执行已有Phase07和规范Tick、相同参数规则暂存写与500000 gas/Tick，不跑RTS经济/战斗/胜负，不因没有主脑在Tick 1假败北。没有第二份参数积分器；例子累加来自真实RuleGraph。
- 定义、动态值/serial/故障照原config/entity根、COMP和Arena/10快照保存；mode已随原完整MatchConfig入配置根。参数快照拒绝混入RTS身体/阵营/导演/终局。静态地图支架仍会构造，并非优化过的无空间通用内核。
- 持久worker仍执行原`run_one_interruptible`，Tick边界取消；完整run结果写私有CAS并fenced登记检查点，恢复剩余run仍执行原Engine。未完成run从种子重算，**不是Tick WAL断点续算**。
- 原Store owner隔离、提交幂等、认领租约/fence、取消/重试代次、四CID一次原子completed不变。不新增DB框架、表或公开发布。

复用16模板/128实例/16组件与16绑定的原上限；模型序列化≤256KiB，明确采样输出1–16且不得重复/引用不存在组件。总run≤256、每run≤108000 Tick；参数采样行预算为`Σ(outputs × ceil(ticks/sample_every) × seeds)`，整个实验≤200000行。输出支持I32、Q8_8原始i16位、Bool 0/1，不经浮点，不自动更改原运算溢出/饱和语义。单轴扫描只改`param.*`，不把state覆盖当参数扫描。

## 四表真实语义与当前执行契约

本批Lab执行/结果契约升为`rynvek-lab-kernel/3`，不可变模型及输出选择进入原spec_digest，run另有model_digest。旧kernel检查点不混算、不自动迁移；旧原始数据/CAS/WAL/Git历史均保留。Arena/10、CURRENT_10、WAL/2不变，无额外旧格式适配框架。

| 表 | 参数profile含义 |
|---|---|
| runs | `mode=parameters, players=0`，真实final_tick/最终规范root、模型digest、真实参数故障；达到请求Tick为`tick_budget`，真正绑定故障为`rule_fault`并停止该run、记录binding ID/错误码。不捏造胜负/出生。 |
| metrics | 每采样Tick×明确输出一行；`parameter_entity/component/type/value`为实际实例ID/组件/类型/原始i32。非该模型的faction/资源/人口/units/buildings/score等为**null**，不是零经济测量；原RTS行这四个参数列为null。UInt64仍精确十进制字符串，新增Int32安全原数值。 |
| lineage | 参数实体不是自然身体，无真实出生事实，因此**空表**；不借serial或实例名伪造家谱。 |
| energy | 保留同采样Tick/root并明确`enabled=false`；计数字段零不是测得零耗或守恒证明，UI不画该profile能量曲线。 |

仍只有原四张表，不另造只在demo使用的第五表。原CAS rows分页/Parquet下载继续可用；UI分析仍每表最多5000行，超限显式提示完整分页/下载，不声称200000行都能无限制画在浏览器。轨迹只连接真实采样点，Q8_8保持raw，不补造中间指标。

## API与SDK

新增只读`GET /api/v1/worlds/{id}/authoring/lab-model`，复用Studio准入并在返回前重验当前permit；未知/他人私有404，`private,no-store`及`Vary: Authorization, X-Rynvek-Studio-Grant`。返回`world_id`、精确十进制字符串`revision`及`model_digest/model`；仅复制源码，不授予WorldRead/快照/归档/Arena控制，也不是第七工具。

```ts
import {LabClient, parameterSweep, parameterMetrics} from "@rynvek/lab";
// origin/token/worldId由自己的受信应用显式提供；不写入包/日志/URL或持久存储。
const lab = new LabClient({baseUrl: origin, token});
const captured = await lab.captureParameterModel(worldId);
const variants = parameterSweep(captured.model, "counter.one", "param.rate", [1, 3]);
const job = await lab.submit({name:"真实参数对照", seeds:[57], ticks:4,
  sample_every:2, variants}, crypto.randomUUID());
const tables = await lab.downloadAll(job.exp_id); // 同一私有四CID
const page = await lab.rows(job.exp_id, "metrics");
const actual = parameterMetrics(page.page.rows); // 精确bigint tick + 原始整数值
```

TS只做结构/整数/采样预算预校验，**服务端原编译器仍权威**。客户端不eval规则、不拉任意包/网络数据。Python `Variant(mode="parameters",policy="idle",model=<同一JSON对象>)`同步接收；本批仅语法/原生共享结构检查，未重建运行Maturin/pyarrow，不能报Python新场景实测。C#本批不冒称新增Lab全域SDK。

## 实际必要快验

本批共38个不同直接用例通过（新增9；其余为受影响规格/列/原入口直接回归，重跑不累计）：

- 原Engine 3：实际扫描轨迹、两次确定性结果/模型根分离、无原生身体、COMP完整快照恢复续跑同根；精确I32/Q8_8/Bool、空谱系/disabled能量/nullable指标四表真实Parquet往返；类型/重复writer/坏源/行预算/非法profile/取消拒绝。
- 真服务 1：Node Studio SDK创建Private世界→真实编译/原补丁→授权源捕获→TS SDK扫描1/3→同键幂等→真实run私有检查点→释放旧fence→同MemoryStore/私有磁盘CAS新服务→实际完成第二run→下载四张Parquet和rows。真实total轨迹分别2/4及6/12；其他所有者读源/作业/rows/Parquet与公共Hub已知CID均拒绝。
- Lab SDK 28（其中新增3）：精确模型扫描/结构和值/原请求源、行预算与CAS轨迹解码，原spec/列/谱系直接契约。
- React JSDOM 6（其中新增2）：实际原入口导入、提交、空谱系Run发现/轨迹、未启用能量不画假曲线、断开/迟到隔离和原作业治理；传输为夹具，不称真实浏览器。最终源取消/显示小改后仅2个新增参数用例再定向复核，不重复累计。

```sh
cargo check -q -p rynvek-server
cargo check -q -p rynvek-rts --features arrow --tests  # 只编译类型
cargo test -q -p rynvek-rts --features arrow --test lab_parameters
pnpm --filter @rynvek/studio build
pnpm --filter @rynvek/lab build
cargo test -q -p rynvek-server --test lab_jobs parameter_studio_source_sdk_scan_checkpoint_rebuild_and_four_tables_are_real
pnpm --filter @rynvek/lab exec vitest run test/parameters.test.ts test/spec.test.ts test/lineage.test.ts
pnpm --filter @rynvek/web-platform exec vitest run test/lab_workspace.test.tsx
pnpm --filter @rynvek/web-platform exec tsc -p tsconfig.build.json --noEmit
```

首次直验纠正的是本批夹具用了不存在的SetRuleModule（应为原UpdateRuleModule），及JSDOM中fs读取不能使用浏览器http import.meta URL；没有放宽原补丁安全分类、鉴权或断言。最终真实服务于2026-11-18 03:38:55中国标准时间通过。

恢复验证是**同MemoryStore+真实私有文件CAS重建AppState/HTTP服务**，首个完整run检查点为原Engine真实计算并手工登记；不是实际硬杀worker、Memory跨OS进程持久化或PG多机灾备。生产跨进程仍需原PG/持久共享CAS/稳定鉴权。未连接实PG、多机/故障分区/断电/长稳未验；实际浏览器/GPU-WASM/物理硬件未验。没有全workspace/Clippy/license/SBOM/soak/CI轮询、WASM/Vite重建、外部发行/部署、新依赖或网络绕过。

下一优先完整生命成本（攻击/修复/分泌/生长合成及仍缺计算计价），随后正式音频采样/空间化、视觉registry/制品；一般规则世界/TS-rpack/.rview、独立Agent赛道/真实LLM与协作presence等仍欠。Pi仍MIT设计借鉴未引代码；主体齐后集中全面验收，持续任务不关闭。

## 第三十九批现行格式与下一主体

原生身体实际动作成本升级为Arena/11、CURRENT_11与Lab kernel/4，旧run检查点不混算；参数实体仍无原生身体、谱系及生命账，不受该成本域计费。上文kernel/3、Arena/10与下一生命成本是第三十八批历史边界，现行详见[LIFE_ACTIVITY_COSTS.md](LIFE_ACTIVITY_COSTS.md)。下一优先正式音频采样/空间化与视觉制品，通用生长/材料/计算仍欠，整体未完成。
