<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 一般参数实体规则：真实消费范围

2026-11-18，中国标准时间 UTC+8；第三十六批。

第三十七批已接所有者定向多编辑者准入和原日志同步，见[STUDIO_COLLABORATION.md](STUDIO_COLLABORATION.md)。第三十八批已接[同一模型的持久Lab扫描与原四表](LAB_PARAMETER_EXPERIMENTS.md)，模型本身仍为下述有界profile，不是任意W3脚本/物理ECS。

## 操作与SDK

原`/studio` → 连接正式WorldEdit世界 → RuleGraph面板。载入「一般参数实体规则草稿」，模块ID填`counter`并编译提交；registry依次选择templates/instances/bindings并使用预填示例创建。读取权威监视器：真实运行后`state.total`按`param.rate`累加。运行仍通过原「进入Arena」/正式席位连接启动，不在Studio凭空推进Tick。编辑实例参数rate不清空state，undo/redo更改定义而非回滚仿真时间。删除实例/绑定后才可删被引用模板/模块，允许SDK同补丁删除完整子图。身份/世界切换清私有草稿及监视值。

原`@rynvek/studio`导出`parameterRegistryOp`、`PARAMETER_TEMPLATE_EXAMPLE`和`PARAMETER_RULE_EXAMPLE`。通过`StudioClient.compile`取权威摘要，将UpdateRuleModule及以下三类操作合在`RemoteStudioSession.edit`即可原子创建；需服务器正常运行Actor时仍经原Epoch耐久提交：

```ts
parameterRegistryOp('templates', 'model.counter', PARAMETER_TEMPLATE_EXAMPLE)
parameterRegistryOp('instances', 'counter.one', {
  template_id: 'model.counter', parameters: {'param.rate': 3}
})
parameterRegistryOp('bindings', 'counter.step', {
  module_id: 'counter', template_id: 'model.counter'
})
```

这些是真正的原SetWorldParam，不增加第七工具或第二补丁协议。JSON null为幂等删除墓碑。原生`RemoveEntityTemplate {template_id:'parameter/model.counter'}`复用同一LWW寄存器与选择性逆操作；原生删除不存在目标仍拒绝。服务基线返回规范定义与真实`parameter_state`，同源WorldEdit权限/私有no-store/原CAS隔离不放宽。

## 数值与执行

`rynvek.parameter-template.v1`声明最多16个I32/Q8_8/Bool组件；Q8.8是有符号i16原始整数，I32有符号32位，Bool只0/1。`param.*`为只读实例参数，`state.*`为动态组件。参数未覆写时用模板default，实例不能覆写state。模板/实例/绑定结构拒绝未知字段；ID沿原slug规则。

非基因、非角色参数实体独立于原单位/建筑，不拥有物理碰撞/脑/生命账或阵营；状态在原Arena Engine Phase07运行。原规则编译器/RIR/hash/受控CPU求值器真正执行，没有eval或任意TS/rpack；独立profile不允许场/邻居/原动作/HP/能量访问。先读同一快照、各绑定暂存、全部成功才提交；失败记录真实绑定/错误码，不发布部分参数状态。同模板不同绑定重复写组件拒绝；binding ID域分离确定性随机。实体serial单调分配，删除不回用。

最多16模板、128实例、16绑定、每值32KiB、规则总源1MiB；参数profile共享500000 gas/Tick并按实例预检。原动作profile仍原每模块gas限制，不虚称全世界统一预算。实例参数/default编辑不重置活state；有活实体时组件形状/类型改变、实例换模板明确拒绝，需显式删建。

## 根、恢复与版本边界

定义在AuthoringState/config_root；动态值/serial/故障在原entity_root和必需COMP。整包投影预检引用/类型/资源后才修改原世界，失败不涂地或部分改状态。原Runtime/Actor/Store/CAS/fencing/TickRecord不另造一套。唯一当前Arena为`rynvek-arena/10`/`CURRENT_10`；旧/9快照与WAL原数据不删除，但新内核拒绝自动打开/无声迁移，没有兼容读层或转换器。规范默认根因此改变，历史golden和全面门禁待主体齐后集中验收，未称全套通过。

第三十六批Lab执行语义曾升为`rynvek-lab-kernel/2`。第三十八批现行为`/3`，已接参数模型ExperimentSpec/持久扫描与四表，原profile检查仍拒绝混用旧run检查点；不迁移或删除旧作业。

## 本批直接快验

```sh
pnpm --dir packages/studio exec tsc -p tsconfig.build.json
cargo test -p rynvek-world --test parameter_registry -- --nocapture
cargo test -p rynvek-server --test parameter_registry -- --nocapture
pnpm --dir packages/studio exec vitest run test/parameter_registry.test.ts
pnpm --dir apps/web-platform exec vitest run test/studio_workspace.test.tsx --config vite.config.mjs
pnpm --dir apps/web-platform exec tsc --noEmit
```

本批10个不同直接用例全部通过，重跑不重复累计：world2、真实服务1、CRDT2、原Studio UI5。必要server编译、Studio SDK tsc build、web tsc --noEmit及diff检查通过，真实服务最终于2026-11-18 03:14:01中国标准时间完成。首次快验发现SDK原生模板ID解析尚未接受parameter命名空间，已修实际解析而非放松测试；服务夹具最初未握手就推进Lobby，已按原真实WS开局路径修正，未绕Running约束。详细最终结果在ACTIVE_DEVELOPMENT与同一Page。真实服务快验用正式注册/认证HTTP、Node SDK与原Arena WebSocket握手启动原Actor，编辑/undo/redo后从初始COMP逐条原TickRecord replay核对末根和动态态，再从检查点续跑同根；匿名/他人/公共CID路径隔离。原生2项覆盖类型/引用/资源/重复写/整包原子性与动态根/恢复。CRDT2项、原Studio JSDOM5项验证入口/选择性撤销/断开清理。

MemoryStore/私有CAS/WAL回放不是PostgreSQL多机灾备或服务端硬杀重启；未验真实浏览器、GPU/WASM通用参数执行、硬件/断电/长稳。未运行全workspace、Clippy、license、SBOM、soak或CI轮询。项目尚未完成；协作与有界模型Lab已由第三十七/三十八批接通；下一完整生命成本、正式音频采样/空间化与视觉制品等依赖持续推进。
