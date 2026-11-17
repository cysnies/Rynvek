<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 持续开发与可执行缺口

更新时间：2026-11-17，中国标准时间 UTC+8。旧M0–M8标签不是全项目完成证明；当前仍在主体功能阶段。

## 第二十四批：Arena结构化生产/科技/信标操作面板（从main fd4b7ec继续）

**主体持续开发，整体未完成；当前仍只有Arena/9、CURRENT_9。** 本批不另建仿真器或本地演示账本，不恢复已拆除的JSON命令主入口，不加旧格式兼容层。复用原ArenaRuntime/Actor、`ArenaController.send`、阵营观测、Canvas2D/WebGPU Renderer 与现有 `--rv-*` 设计令牌。

### 真实可操作入口

- `/arena` 侧栏主操作区由权威观测驱动的结构化面板取代原先 JSON 文本入口。可从地图点选或建筑名册选中己方建筑：孵化槽排队/取消队尾（全额退款）、突触精炼厂研发 Tier 1、演化坩埚研发 Tier 2/3 与定向繁育、信标配置 `field.signal_a` / `field.signal_b`、拆除非主脑建筑（返还 50% 造价）。建造菜单补上精炼厂与坩埚。
- 造价、科技前置与队列上限只作提示，**不预扣资源、不乐观改队列**；只有 `ArenaController.send` 的真实回执记成功。回放、断开、终局或非己方建筑时写入控件禁用。信标表单将观测中的 Q8.8 整型换算为命令用毫单位预填（`milli = q8 * 1000 / 256`，向零截断）。
- 观测补上己方 `research_remaining_ticks` 与公开 `BreedingView`（仅兵种与剩余 tick）。`BreedJob.child` 基因组不进入观测。基因组下拉为观测 slug 加该兵种缺省种子；无法仅凭 slug 过滤原型，错配由服务器拒绝。

### 提交与快验边界

本批代码提交：`94a3796`（观测投影）、`22c2dcd`（协议对齐）、`3015ff7`（控制器公开方法）、`89e2ab3`（结构化面板）、`791790e`（定向快验）。最终 HEAD、一次普通 push 与 PRIVATE/干净证据记同一 Page。

必要快验：`cargo check -p rynvek-rts` 通过；`@rynvek/arena` 与 `@rynvek/web-platform` 的 `tsc --noEmit` 通过；web-platform 定向快验 13 项通过（面板视图/命令形状 10 项，已有世界挂载名册选中并 `cancel_queue`、回放只读 3 项）。不是实际受限浏览器、GPU 或仿真验收。未重编浏览器 WASM，未跑整 workspace / Clippy / license / SBOM / soak 或轮询 CI，未跑 Vite 打包。`@rynvek/client` 源码 `tsc` 仍有既有 `client.ts` welcomeWaiter 报错，本批未改该文件，也未当作本批回归。真实 GPU/PG/断电/24h 仍未验。不绕既有浏览器/端口网络限制。

仍欠：排位队列/观战 UI、多人自创席位流程、任意编辑时间线、客户端独立归档验签/持久抽帧索引；Studio 规则删除与一般参数/实体模板/任意 TS/rpack/.rview、协作流与 Lab 一般 W3/作业持久治理；攻击/修复/分泌/生长合成及其他未覆盖生命成本；Hub registry/agent/audio、C# SDK、音频接入/神经声化。不是完整 Arena / 全热力学 / 全项目完成。

## 第二十三批：Studio已有世界进入Arena原Actor与真实私有重演（从main 4e079c0继续）

**主体持续开发，整体未完成；当前仍只有Arena/9、CURRENT_9。** 本批不改身体/神经/场数值、不加适应度或能量来源，不另建仿真器、practice世界或兼容旧格式框架。依据原设计的Studio→Arena、世界权限、签名日志/确定性回放，复用原Runtime/Actor、输入控制器和Canvas2D/WebGPU Renderer。

### 真实可操作入口

- `/studio`连接后，“在Arena运行/重连此已有世界”只携带世界ID与服务origin。`/arena`重新输入正式凭据后调用`POST /api/v1/worlds/{id}/arena`，要求WorldEdit、WorldConnect、MatchPlay及Human准入。本批绑定**真实已有单人PvE**，从原已提交快照/WAL恢复实际地图、参数、基因、规则及谱系，严格复用恢复配置；不调用local/practice、不分配另一身份/世界、不把自创场景送入排位。并发/刷新复用同一活动MatchRow/席位；已有终局仅返回归档入口，不重置世界。
- 继续原WS首帧能力认证、原席位/控制租约、阵营迷雾观察、选择/移动/采集/建造/投降与原Renderer。生产/取消队列、科技、繁育、信标、拆除等当前命令均有**真实JSON提交入口**和选中建筑ID填入；只有真实ack才显示成功，资源不本地修改。这不是已经完成了所有生产/科技的结构化操作面板。
- 凭据只在内存、HTTP头/首帧，不再把练习令牌写sessionStorage。显式“开始本地练习”与已有世界运行分开。断开/离页销毁原客户端、关闭传输、拒绝握手/命令/观测等待者、清空私有视图；代际请求隔离阻止迟到结果重新附回。断开不冒充服务器暂停或撤销已提交命令；正式同一身份可重新认证进入原Actor。

### 恢复、签名归档与正确边界

- 新节点上下文可从真实持久世界恢复**原match_id**；原World fence同时保护Tick CAS和对局状态写入，旧宿主迟到Aborted/终局不得覆盖恢复后的状态。恢复宿主Lobby/Ready不会让持久Running倒退或重写started_at；手动推进若未提交/已终止返回错误，不再把旧Tick当推进成功。Memory和PG存储实现均更新，真实PG尚未运行。
- 归档起点是**最后真实已提交Studio创作快照**（没有前置创作时为genesis），不是重建模板，也不是伪造编辑过程帧。起点后的全部实际TickRecord/WAL（包含恢复前命令、活动编辑及谱系）重新演算并核对实际已提交终根，再签名；不只保存新宿主内存后缀。不支持在运行日志中遇到非Actor创作后悄悄丢掉前缀，未知/损坏日志拒绝。UI公开实际起点Tick/revision/root。开局前Studio编辑过程时间线不在本批播放器重演范围。
- `POST matches/{id}/playback`打开服务端验签的当前归档；`POST .../playback/{cursor}`按真实记录有界定位，`DELETE`关闭。每次最多120条记录、同时2个重演工作线程、全局4游标、空闲10分钟淘汰；这些是本批工程资源上限，不是原物理规范数值。仅节点内可丢弃游标，打开新回放替换本身份旧游标；离页可清理尚在构建的游标，迟到任务不能复活它。原归档和权威世界不因此修改；每个重演批次隔离后发布，坏记录/终态分歧也不偷偷推进游标。
- Arena真实挂载播放/暂停/定位/倒退/关闭/原签名文件下载。只读消息适配将**服务端真实重演观测**送进原Arena reducer/Renderer，没有浏览器仿真或插值假帧，拒绝控制租约及任何写命令。显示“签名已验”和“仅已重演前缀”是两个状态；到达实际终根且命令/结局/检查点全一致才显示完整复算通过。倒退从真实起点有界重演，可停止；未做持久随机访问索引/长片性能优化。真实事件复用实时阵营可见性过滤，每帧最多显示256条并报告裁剪，不删日志；逐Tick释放临时事件。
- 完整归档含私有基因/规则，迁入原私有世界CAS。非排位自创世界完整归档/播放器要求WorldEdit，不能把WorldRead当基因源码授权；私有对局元数据按WorldRead隔离，无凭据/他人404，成功响应no-store/Vary。公共排位归档维持公开语义。Hub公共blob入口拒绝RYRP，即使归档曾误放公共CAS也不按CID泄漏；不删除原始制品、历史或无关数据。

依赖DAG：`Studio已提交世界 → 原私有CAS/WAL恢复 → 原MatchHost/输入租约/命令 → TickRecord及同一Runtime重演 → 原阵营观测/事件过滤 → 原Arena控制器/Renderer`；不新增crate或第三方依赖、反向引用和第二仿真器。简体注释/SPDX保留。

本批代码提交：`19f2d54`（原客户端销毁）、`ce6f41c`（既有世界/原Actor/私有有界重演）、`2693dc6`（真实Arena挂载）。最终HEAD、一次普通push与PRIVATE/干净证据记同一Page。

必要快验：Rust共4项通过：新增真实HTTP/WS/Actor两项（已有创作锚点/并发开局不增世界、原位置改变/重连/终局、私有归档与Hub绕路拒绝、前缀/终根/倒退/坏日志不改权威、同Store+私有磁盘CAS的新AppState恢复同match及恢复前命令）；终局记录消费与原fence失败原子性各1项。前端直接挂载/传输/原客户端等待者/Studio链接共18项（不重复累计重跑子集）通过，最终类型及Vite打包通过。具体通过数、HEAD与一次普通push证据记同一Page及阶段报告。**不是实际受限浏览器验收**，未运行全workspace/Clippy/license/SBOM/长soak/CI轮询，未重编WASM；真实GPU/PG/断电/24h仍未验。不绕既有浏览器/端口网络限制。

仍欠：Arena生产/科技/信标的完整结构化面板、排位队列/观战UI、多人自创席位流程、任意编辑时间线、客户端独立归档验签/持久抽帧索引；一般参数/实体模板/规则删除、任意TS/rpack/.rview、协作流与Lab一般W3/作业持久治理；攻击/修复/分泌/生长合成及其他未覆盖生命成本；Hub registry/agent/audio、C# SDK、音频接入/神经声化。不是完整Arena/全热力学/全项目完成。

## 第二十二批：Studio有界通用创作进入真实权威链（从main b5495b2继续）

**唯一当前Arena/9、CURRENT_9；原始规范附件不改写。** 当前`UpdateRuleModule`明确使用`rule_graph_json`取代未消费的`ts_source`，不接受旧字段或假装执行任意TypeScript。`/studio`现真实挂载现有React设计系统/停靠布局；Arena页头可进入。可连接已有WorldEdit世界，或显式使用WorldCreate创建**Private**当前格式PvE世界后连接；预填生态复用Lab建议并标注不是规范定值，不自动生成演示世界。

### 当前可操作的真实范围

- `GET /api/v1/worlds/{id}/authoring`：从同一已提交Actor检查点/WAL恢复基线（地形、共享种子目录、角色参数、动作规则、revision/tick/root/规则故障）。要求WorldEdit，不向只读玩家暴露基因/规则；`private,no-store`。不同阵营的私有/分歧目录不伪装成共享可撤销基线，修改或删除这种同slug分歧会拒绝。
- 现有`POST .../patches`整包预编译后原子提交：真实矩形地形；`ecology.roles`或`ecology.roles.<unit-id>`的已配置同一角色组参数；有效Genome JSON的标准Base64共享种子目录新增/替换；`genome/<slug>`模板删除。种子变更只影响未来排队，不悄悄换活体脑/已冻结队列；目录删除不杀活体或抹历史。未绑定参数/一般非基因实体模板仍拒绝，不能把本批说成任意WorldParam或所有模板类型已完成。
- `POST .../authoring/compile-rule`真实调用原四阶段RuleGraph编译器并给出RIR BLAKE3；PATCH再次从同一源编译/核验摘要。最多32个模块，每源≤256KiB；仅绑定六个`unit.actions.*` Q8.8真实脑动作，常量为原始定点整数（256=1.0），禁止HP/能量/场/邻居绑定。模块按ID确定执行，原Philox/CPU解释器、每模块500000 Gas，整组输出先暂存；任何失败不提交动作，故障事实入根/快照。输出在原脑之后、原运动/采集/战斗前消费，位移/维护仍由既有身体账支付，不另造演示引擎或免费能量。
- CRDT/选择性撤销复用原AuthoringDocument/HLC/UndoManager。基因与`genome/`目录删除合并到同一LWW寄存器，可删除→恢复→重做，并跳过别人后来的写入；普通世界revision由真实提交回执校准，不拿本地补丁数代替。UI仅展示回执后的地形/版本/根，409/网络不确定使本地会话失效，刷新基线再操作，不自动重试或覆盖他人。首建规则仍无删除逆操作，既有规则替换可撤销，界面明确告知。
- 凭据仅内存，断开/离页清理基因、参数、规则和回执；旧请求不能附回新身份，往返缓存恢复空会话。没有伪造播放/单步按钮；当前Tick/Epoch由原Actor调度，第二十三批已接通已有世界运行/恢复与有界签名回放，完整结构化交互仍需推进。

### 权威连续性修复与验证边界

原普通事务仍冻结配置；仅认证创作通路允许同一角色组数值更新，不改变地图/模式/seed/玩家结构。`last_edit_revision`记录最近创作提交，普通Tick不再令排往下一Epoch的补丁天然过期；途中有真实创作更新仍冲突。Actor与TickRecord重放使用同一检查及创作事务，普通冻结没有放开。分叉新身份重绑创作时钟，保留规则和生物状态。

出生事实现在冻结**出生时角色契约**。后续修改出生投入、关闭自然繁殖或修改突变率，不再用新策略反证旧实付事实；原父子转账/基因/代数校验仍在。基因输入先验尺寸再表达原真实脑，避免不可信LIF输出尺寸导致索引panic/巨额分配。

依赖DAG：`core/field/neural/rules → rts权威世界/动作消费 → world补丁整包计划 → 原runtime/Actor/CAS/WAL → Studio CRDT/HTTP回执 → React /studio`。仅增加现有workspace规则库和已有base64依赖引用，没有新外部版本/反向依赖/第二仿真器。

只执行必要编译/类型和本批直接快验：真实基因/规则/目录原子编辑、原运动位置变化、快照/分叉/恢复同根、越权组件/坏基因尺寸、真实私有HTTP四操作/版本冲突、活动Actor Epoch规则提交、角色调参WAL重放/普通配置冻结、历史出生契约、CRDT删除撤销/顺序收敛与修订、React真实创建/回执前不展示假权威/409失效/路由挂载、Vite构建。具体通过数与提交见同一Page和阶段报告，不把这些称完整验收。未运行全workspace/Clippy/license/SBOM/soak/CI轮询；未验实际受限浏览器、GPU/PG/断电/24h，未重编WASM或跑Python原生全路径，不绕网络策略。

仍欠一般参数注册/实体模板、任意TS/rpack/.rview、实时多人协作流、规则删除与Lab任意规则变体；动作控制器计算尚未形成身体能价，不声称完整生命成本。作业持久治理、一般W3、全生态、Hub/C# SDK/音频等缺口不因Studio入口挂载而消失。

## 第二十一批历史：W3自然复制规则实消费与真实Lab工作台（从main 990e52d继续）

**第二十一批当时为`rynvek-arena/8` / `CURRENT_8`，当前以第二十二批/9为准。** 每个角色增加必填`replication_mutation_rate_permille`（0–1000）；Rust/HTTP/TS/Python统一，不兼容旧平铺或缺字段格式。此值进入原ECOL/规范根、快照/WAL与自然复制，不只是实验回显。50‰只是既有工程取值；工作台预填项均标明实验建议，不冒充原规范冻结数值。真实0‰消融不改变亲本付款、出生阈值或权威谱系；原有定向繁育10–250‰限制留在其自身入口，不再暗中覆盖自然规则的0‰。

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

当前入口仍为真实世界创建`POST /api/v1/worlds`的`ecology`字段、Lab变体的同名字段，以及Rust `EcologyContract`。TypeScript/Python同步采用**唯一当前角色表**；旧平铺Forager三参数不再接受，不留别名/迁移。第二十批当时为`rynvek-arena/7` / `CURRENT_7`，当前以第二十二批/9为准，私有BodyEnergy新增运动小数余量，随原规范根、快照/WAL恢复。

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

## 第十九批历史：权威自然谱系进入真实Lab（当前配置以第二十二批为准）

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
- 第十九批当时使用`rynvek-arena/6` / `CURRENT_6`，现仅第二十二批/9。出生事实补充faction/unit/slug，随原ECOL规范根、快照、WAL/Actor路径走，不增公开观测泄露。活体及自然父代的出生元数据/基因身份有交叉校验；已死亡初始根的历史身份仍由原权威根认证，不声称能独立复算其已丢弃基因。

### 依赖DAG与快验

`neural能量/遗传 → rts权威世界/自然出生 → rts::lab四表 → server CAS/私有API → TS/Python消费`。无新crate依赖，无反向引用，无并行假仿真器；原始附件、旧工作区、缓存及Git历史保留。

只执行改动所需编译/类型和定向快验：真实地图600 Tick产生至少连续两次自然代际、亲子基因和支付链、个体总账、确定Parquet、私有HTTP作业与所有者隔离、TS消费；具体结果/最终提交见同一项目Page与交付记录。不运行workspace/Clippy/license/SBOM/90秒soak，不借旧CI成功冒充新HEAD验收。

## 下一阶段与剩余实际缺口（按依赖推进，不逐项等用户决策）

| 顺序 | 可执行工作 | 完成时应有的实际证据 |
| --- | --- | --- |
| 已推进，仍有边界 | 第二十批已统一六角色配置，接通其他角色真实供能及运动/突触维护；继续补未覆盖生命成本 | 生物合成/生长、攻击/修复/分泌、野生与非脉冲计算等仍需显式规则；不冒充物质总守恒 |
| 已推进，仍有边界 | 第二十一批已挂载Lab真实工作台并消费W3自然突变规则；一般规则编辑/持久任务治理待补 | 真四表、自然谱系图、BigInt能量曲线、seed/网格/消融、私有进度/下载已接通；规模上限/控制器干预明确 |
| 已推进，仍有边界 | 第二十二批已挂载Studio参数/共享基因/动作规则/目录删除与权威撤销；一般TS/rpack/模板/协作流仍欠 | 当前只支持显式绑定范围，不伪称所有参数/编辑器完成 |
| 已推进，仍有边界 | 第二十三批已有Studio世界运行/重连/节点上下文恢复、完整运行WAL私有签名重演 | 真实单人PvE/原Actor；前置编辑过程/多人席位/客户端独立验签/持久抽帧索引仍欠 |
| 已推进，仍有边界 | 第二十四批已用结构化生产/科技/信标面板替换 JSON 命令主入口 | 真实观测与 ack，不预扣资源；排位/观战/多人席位、Studio 规则删除与一般绑定仍欠 |
| 下一高价值主体阶段 | 排位队列/观战或多人自创席位，或紧邻的 Studio 规则删除/一般绑定 | 复用原匹配/席位/权限与当前权威规则补丁，不造假队列或静态大厅 |
| 主体缺口 | Hub registry/agent/audio、C# SDK、音频接入与神经声化 | 当前模型端到端消费、真实设备/服务边界和隐私；非占位API |
| 主体完成后集中 | 集成门禁、长期种群/资源稳定、性能、浏览器、硬件GPU、真实PG、断电、24小时 | 届时真实执行记录，不复用旧CI截图；现有网络策略不绕过 |

本阶段交付 Arena 结构化生产/科技/信标操作面板，不宣称整个 Arena、项目、全生态、W3一般规则/持久作业治理或所有生命周期成本已经完成。每个可交付阶段普通 commit/push 后继续上述缺口，只有真实阻塞才请求最小用户动作。
