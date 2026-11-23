<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 原RYPK Tier1组件规则的权威链

阶段50补充：[真实TS AST与可复验Tier0构建](TIER0_TS_AST_BUILD.md)已接入受限Kernel语法及原World链；以下当批“TS AST未做”保留为历史口径，不意味着已交付完整私有rpack依赖签名安装链。

## 来源与明确范围

依据最终设计 `Rynvek_Project_Documentation_v1.0.md` GAP-06 的Tier1 QuickJS-NG / 500000 Gas，以及原开发规范§8分层规则，复用原QuickJS、原参数模板/实例、Phase07、WorldPatch和恢复链。原RYPK唯一解析器下沉到core，world仍公开同一模块；不是第二种私有脚本包，也未引入旧格式兼容。

这是**有界参数组件上的真实Tier1 JS制品执行**，不是任意World/完整物理ECS。Tier0 TS AST自动编译、任意TS依赖/模块加载、通用刚体与碰撞、一般实体生命周期、规则私有Hub签名发现→明确安装仍未因此完成。CPU配额不是生物能量；无身体参数实体不凭空生成能量账。

## 真实入口

1. `/studio` 原规则面板的“TS构建产物 / 原RYPK Tier1”，或 `StudioClient.buildTier1`。
2. 作者先用自己的TypeScript构建得到单文件全局 `function tick(input)` JS。可附UTF-8 TS作来源资料，不验证二者等价、不在服务端运行tsc/npm或下载依赖。
3. `POST /api/v1/worlds/{id}/authoring/build-tier1` 只构建原容器，返回 `install / artifact_blake3 / executed:false`，SDK校验profile/不执行标志并从原包字节独立复算BLAKE3。原owner/editor、版本化编辑grant、构建后权限复核、private/no-store/Vary均保留。不执行顶层、不安装、不自动开局。
4. 明确提交 `SetWorldParam rules.tier1.<id>`，走原EpochBoundarySafe、修订/HLC/准入/fence和提交后回执；安装对象仅 `template_id / package_hex`。原工作台基线读回真实包；原参数逆补丁可撤销/重做，null明确删除。
5. 原Phase07消费；动态组件、故障和本Tick实际粗配额回执进入原权威根/快照/WAL。删除只停后续消费，不抹组件值、历史或原资料。
6. 原Lab捕获精确保留安装包，以当前 `rynvek.lab-parameter-model.v2` 重启默认态执行多seed/参数扫描，仍走原四表，不克隆秘密运行时隐态或杜撰RTS指标。

构建请求结构：`package_id/version/template_id/reads/writes/source_js/source_ts`。原包条目限定 `rule.json`（kind=data，media_type=application/json）、`rule.js`（rule_source，text/javascript）、可选 `rule.ts`（rule_source，text/typescript）。profile固定 `rynvek.tier1-components/1`，本profile仅接受冻结引擎范围字符串 `>=0.1.0 <0.2.0`（不冒称通用semver依赖求解器），容器摘要完整检查不等于作者数字签名；本入口是已授权创作者本地制品，不冒称已接Hub发布/发现。

## 输入、事务与量纲

输入仅十进制字符串 `tick/world_seed` 以及绑定模板的有序实体数组 `{id,serial,values}`，values只包含显式reads。没有RTS敌人、fog绕过、文件、网络、宿主桥或隐藏世界状态。I32、Q8_8、Bool均使用原始整数（Bool为0/1），输出不允许浮点/原生JSON布尔值冒充整数。

输出严格 `{writes:[{id,values}]}`：必须按原顺序逐一覆盖本次匹配实体、精确给出声明写集，只写state.*，不能写param.*、新增/销毁实体、任意规则动作或直接改场/能量。原Tier0与Tier1同Tick读相同旧态；组件写集冲突安装时拒绝，任一Tier1失败回滚本Tick整个参数组件候选，连较早Tier0写一起回滚。原身体动作仍是后续独立收费事务，不宣称任意世界所有副作用统一事务。

每Tick新QuickJS Context，不共享全局变量/模块单例；新单次入口避免权威调用前再执行一遍顶层。原Date/Promise等非确定性内建裁剪、Math.random禁用、Philox宿主原语与无I/O保持。共享原Tick的500000 CPU硬预算：Tier0→Tier1→原身体动作。QuickJS每个中断轮询计10000，每次Tier1调用另保留至少10000固定配额，包含顶层/调用/序列化；执行错误保守扣尽剩余额度，输出校验失败保留实际已扣额度。回执是粗粒度CPU预算，不是假装逐指令Gas或物理焦耳。

## 资源与当前格式

- 最多4条Tier1绑定；原16模板、128实例、每模板16组件等既有界限不放开。
- 原RYPK最多12KiB，安装仅小写hex（最多24KiB），沿用原32KiB registry值上限。JS最多8192字节，TS最多8192，合计仍须满足包总限额。
- 每条最多16显式reads/16显式writes；输出JSON最多64KiB；QuickJS默认64MiB堆与512KiB栈，原中断配额涵盖不可信顶层和tick中的循环。
- Arena快照15 / CONFIG_CURRENT_15，Lab kernel/8、参数模型v2。线上Arena JSON仍/2、WAL仍2；不提供旧形状fallback，不删旧原始资料或历史。

## 验收边界

定向测试命令与本批实际结果记录在 `ACTIVE_DEVELOPMENT.md`。不得把编译、重复重跑或未选中用例算新增通过。浏览器WebGPU/硬件GPU、真实PG多机断电长稳、Node22正式运行时与本批C#重编仍待集中验收；阶段41 llvmpipe仍仅软件回读。完整workspace测试、安全/许可/SBOM/发布打磨在主体齐后集中执行；本批不是正式发布或全项目完成。
