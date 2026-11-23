<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 阶段50：真实TS AST到可复验Tier0制品与原World链

从干净 `d44e92af451b3fde1821f28357235388f03102da` 继续，先核交接、ACTIVE与远端一致，没有重做阶段49。**主体继续，整体未完成、未达正式发布质量。** 按真实依赖先补可审计的源码前端与可复验制品基底；没有把已有作者自附TS当编译器，也没有把本段冒充完整私有rpack发布/发现/验签/安装链。

## 真实链路与不执行源码的边界

`rule.ts` → TypeScript **5.8.3真实语法树** → 有界白名单节点遍历 → **原KernelBuilder/RuleValue** → 原 `rynvek.rule-graph.v1` → 原Rust类型/访问集/循环三遍编译 → 原RYIR → 原RYPK → 本地所有者显式命名模块创世安装 → 原RYWL → 阶段49原HTTP身份/ABAC/双租约Actor → 原Phase07实际付费规则/物理 → 原TickCommitter私有提交/WAL恢复。

- `packages/rules/src/ast.ts` 的 `compileKernelSource` 使用固定 `typescript@5.8.3`，不调用eval/Function/转译后执行、不运行作者回调，不读取tsconfig、默认lib或依赖文件，不解析作者import到磁盘/网络。虚拟CompilerHost只提供内存rule.ts。
- AST不是另一套规则解释器：仅将允许的语法映射到原KernelBuilder/RuleValue方法。原图作用域、Phi/循环携带值、读写检查和36指令出口复用；Rust仍是完整类型/权限/循环与IR/Gas权威。
- 新入口只在 `@rynvek/rules/ast` 子路径导出，未挂到原浏览器主入口；TypeScript从dev依赖移为精确5.8.3生产依赖，锁内现有包/完整性不变。Node构建bin为 `packages/rules/bin/rynvek-rule-ast.mjs`，源码64KiB有界读取、UTF8严格解码，输出使用wx拒绝覆盖。
- 这不是任意TypeScript执行环境或整个TS语言到WGSL的通用编译器。当前静态语法剖面 `rynvek.ts-kernel-ast.v1`，没有模块加载、作者辅助函数、变量重赋值或一般JS控制流。没有把语法诊断称为完整TypeScript语义类型检查。

## 明确语法剖面

恰好 `import { Kernel } from "@rynvek/rules"` 和 `export default Kernel.define(name, declaration, (k) => { ... })` 两条顶层语句。声明只能是直接reads/writes对象与Bool/I32/Q8_8/Q32_32字面量类型，拒绝重复、getter/展开/计算属性/保留原型字段。名称及数值/字符串均由真实AST读取，不用文本正则抽取业务代码。

允许单个简单const规则值绑定；Builder/SSA受控方法调用；原 `k.loop(1..64, [初始值], (i, carried) => [下一值])` 或末尾return数组的块回调；携带数组只能常量合法索引（允许数组索引后的TS非空断言）。原嵌套总展开≤256。原36指令全部有构建出口，但受限上下文能否运行仍由原Rust/World组件权限决定。常量只允许安全表示的直接数值/负数字面量、原精确Q8.8和整数Q32.32、布尔/字符串字面量；不解释一般算术表达式。

源码≤65536 UTF8字节，AST≤8192节点、深度≤96；解析器栈异常归原0x5001，不暴露宿主异常。拒绝动态import/require/eval、新建对象/函数、while/for/if、async/generator、可选链、方法计算索引、任意属性、遮蔽/重绑定、类型断言和用户顶层副作用。只用 `k.loop` 表达业务循环，不执行JS循环。

## 可复现原RYPK构建与实际验证

`crates/rynvek-rules/src/package.rs` 提供唯一共享 `build_tier0_package` / `Tier0RulePackage::parse`，继续调用core原 `read_package/write_package`，不复制容器解析器，不放宽条目路径fence。

受限 `rynvek.tier0-build.v1`：原RYPK、固定 `engine_compat=>=0.1.0 <0.2.0`、≤1MiB，恰好三条：

| 条目 | 内容 |
|---|---|
| `rule.json` | 固定规范字节的编译器/profile标记，不是作者声明的TS等价证明 |
| `graph.json` | 原解析器规范化的权威RuleGraph（≤256KiB），保留声明/SSA |
| `rule.rir` | 原三遍编译器生成的RYIR |

每次解析重新编译graph，原decode_rir再次验证IR，并要求实际IR与重新编码逐字节一致。合法容器重封装后换图/换IR/改编译标记也拒绝；多附rule.ts或其他执行/依赖条目拒绝。**本剖面不携带TS，不接纳自报TS→IR关系**；源TS确实由上游AST前端解析生成图，但制品层只证明graph→IR，不能把来源附件当远程构建认证。

同一工具链实现/输入/包ID/版本/许可证，重复构建字节相同；没有时钟、随机、绝对路径或作者回调执行。不是跨任意编译器版本/平台的可复现认证。`rynvek rule build` 只写一个新文件；`rule inspect` 报原CID/IR hash、静态gas_estimate、`signed:false`。原公共Hub扫描现在识别此profile并调用同一复验器；`graph_rir_recompiled_equal:true` 与 **`ts_source_equivalence:false`** 分开报告，IR静态扫描不冒充1,000 Tick运行沙箱。

`world build --rule-pack module=path` 是显式**本地创世**安装，最多16个、不重复；必须已有对应绑定，不能覆盖Tier1。提取已重新验证的权威图，替换命名模块，再经过原GeneralWorldConfig组件/宿主/价格与128KiB RYWL上限检查。没有包则不改变旧行为；未知模块/非法包/欠缺原组件声明拒绝，不隐式造绑定或绕生产fence。最终World保存实际图/根，不保留一个虚假的私有registry锁引用。

## 可执行本地示例

在仓库根目录执行；每个输出必须为新文件，不覆盖原资料：

```sh
pnpm --filter @rynvek/rules build
work=$(mktemp -d)
node packages/rules/bin/rynvek-rule-ast.mjs examples/rules/general-steer.ast.ts "$work/graph.json"
cargo run -p rynvek-cli --offline --locked -- rule build --graph "$work/graph.json" --out "$work/steer.rpack" --id rules.ast.steer
cargo run -p rynvek-cli --offline --locked -- rule inspect --package "$work/steer.rpack"
cargo run -p rynvek-cli --offline --locked -- world build --config examples/worlds/general-rules.world.json --rule-pack "steer=$work/steer.rpack" --out "$work/steer.rworld" --id world.ast.steer
cargo run -p rynvek-cli --offline --locked -- world run --package "$work/steer.rworld" --world-id 01010101010101010101010101010101 --ticks 1 --snapshot "$work/genesis.rsnap" --journal "$work/world.rjournal"
cargo run -p rynvek-cli --offline --locked -- world recover --snapshot "$work/genesis.rsnap" --journal "$work/world.rjournal"
```

示例将param.thrust×Q8.8 0.5写到原state.accel_x；真实HTTP端到端实测加速度 `1<<31`、规则实付>0、原生命闭世界相等及私有WAL恢复同根。规则静态Gas/运行时Tier0实际计量仍是原机制；未改Tier1粗CPU配额，更没有把CPU额度叫逐指令Gas或生物能价。生命规则实付仍是原有来源EnergyAccount许可/热耗，不新增物质/能量，不称严格机械功化学能模型。

## 快验与真实未验

64个不同定向用例通过，新增35，重跑不累计：

- 原规则SDK vitest44（AST新增30、原Kernel7/QuickJS沙箱7），typecheck/build通过。包含逐字节Builder同图/重复构建、36出口、原Phi/循环、宿主副作用/动态模块/原型/控制流/超限与原错误码拒绝。
- Rust规则包2新增：可复现字节、图IR复验、合法包络篡改/自附TS拒绝。
- CLI ast_pipeline2新增，其中跨Node/Rust/HTTP用例缺省标ignore仅隔离构建前置，本批**显式 `--include-ignored` 实际运行通过**，不是跳过计入。真实两次Node前端/两次Rust构建同字节、拒绝覆盖/未知安装模块→RYWL→临时Ed25519登录/原双fence→真实付费位移→同MemoryStore新AppState+私有文件CAS恢复同根。不是PG断电，CI尚未编排此跨工具链前置。
- 原CLI general_world3，原server general_authority9，Hub4（新增签名合法但换图/附TS拒绝1，其余原3）通过。Hub签名测试只在回环测试服，复用原签名协议；**不是已交付私有规则发布链**。
- 必要原生 `cargo check -p rynvek-cli -p rynvek-server --offline --locked`、新增Rust文件/改动函数rustfmt和diff检查通过；必要敏感模式筛查无命中，不等同完整安全审计。

当前真实Node **24.22.1** / pnpm10.21.0 / TS5.8.3，根Node22约束有警告，**不能称Node22验收**。普通离线lockfile重解算因无其他workspace的元数据失败；只移动已经锁定的同一TS5.8.3记录并固定精确版本，`pnpm install --offline --frozen-lockfile --lockfile-only --ignore-scripts`一致性验证通过，没有下载/替换解析器或绕过网络。

没有跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询；C#本批重编、wasm32、真实browser/硬件GPU、PG并发SQL/多机/断电/24h长稳仍未验，阶段41llvmpipe仍仅软件回读。旧6个PvP_Custom Actor夹具须补真实lease前提，不能降低生产fence。

## 下一真实依赖与剩余发布阻塞

本段补的是完整私有规则链**必要的真实编译/可验证制品前置**，未交付owner-private规则命名空间签名发布/精确依赖锁/DAG发现/逐级验签与撤销、显式权限Actor安装及来源日志恢复。原公共Hub已有签名扫描，原Agent/视觉/音频已有私有注册表，但不能因此声称通用规则私库链现成；后续必须复用它们的原权限/私有CAS/不变版本机制，而非另造Hub或ACL。

本版本地命名模块安装不是在线Epoch热更新，也不声称已签名或自动依赖求解。需要后续把精确包身份/CID/签名/锁/撤销与安装准入、提交及恢复绑定；不能把本地嵌图当此证明。General仍为单Chunk/Private所有者显式Tick；一般World/完整生命材料生长遗传、多Chunk/三维、Agent独立赛道、协作presence/离线多主等主体仍欠。主体齐后集中完整门禁、安全及正式发布打磨，不以本批结束宣布项目完成。


## 阶段51接续（保留上文当批历史边界）

现已接通有界Tier0私有签名精确包组的真实HTTP发布/发现/逐级下载验签、原权限Actor下一Coupling Epoch显式安装、原有源实付Tick及私有来源证据恢复，详见[PRIVATE_RULE_PACKAGES.md](PRIVATE_RULE_PACKAGES.md)。不修改本地叶子RYPK三条目fence，不把此共同安装剖面称全规范跨种类锁/任意World完成；历史撤销与活Tick分开，非Epoch暂停后安全续跑仍欠，PG多机/断电/真实硬件和完整发布门禁仍待实证。
