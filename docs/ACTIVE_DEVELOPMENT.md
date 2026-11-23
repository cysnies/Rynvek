<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 持续开发与可执行缺口

更新时间：2026-11-24，中国标准时间 UTC+8。旧M0–M8标签不是全项目完成证明；当前仍在主体功能阶段。

## 第五十八批：耐久运行意图→原Actor服务器后台实付→暂停取消撤权→真实PG进程恢复（从main 02fab14继续）

**主体继续，整体未完成、未达正式发布质量。** 已核交接/ACTIVE、真实D337/D570和干净PUBLIC远端 `02fab1473990242c3c5966e874f1c53bf946fef7`。本批落实指定耐久主体，不再改选阶段57协作Web；详见 [GENERAL_DURABLE_RUNS.md](GENERAL_DURABLE_RUNS.md)。

- 正式Human所有者原Sovereign双fence+显式consent+精确根启动持久意图，限定1..128 Tick、10..900秒、调度间隔和累计规则实付EnergyQ预算（原paid_q之和，非全部自然代谢/世界能耗）。无规则/CPU-only/维护态拒绝，不保存Bearer，不暗开协作者后台或外部provider。
- 复用原Store及Lab Claim/OwnerAction/WorkerAction/租约治理，服务后台有限串行槽按PG SKIP LOCKED认领，再调原General Actor、原LeaseManager/CommandGate/规则有源Tick；没有客户端推进循环/第二世界。Memory回执明确进程内，PG还要求私有文件CAS。
- 原TickCommitter/Store最终锁中同时检查作业fence/租约/墙钟/步号/费用上限和原ABAC/目的/宿主双fence/精确依赖撤销/前驱，原子提交进度/费用/根与世界Commit。持久步键/`/5`日志不因重启重扣；旧格式/保护不放宽。
- pause/resume/cancel原操作CAS与持久幂等；暂停返回后旧候选拒绝，恢复不延预算或跳Epoch。暂停保留精确世界，撤包/禁用/欠费blocked不免费推进，可先cancel再沿原维护换装。失回执可按原提交键只读查询，无宿主认领。
- 首次真实PG17.12合成实库/至0013迁移与独立服务OS进程SIGKILL恢复：第3步崩溃后新进程等待旧健康租约，从同一PG+私有文件CAS自动续至8步，与原Engine同根/160 EnergyQ/9个含genesis提交；新job/host fence递增，旧提交键不重复应用。另实测PG暂停最终锁旧候选及原子进度拒绝。
- 42个不同必要定向通过，新增9：状态机3、后台HTTP4、签名私包撤销1、真PG进程恢复1，加原General19/私库12/Studio2；重跑不累计，另一个辅助子进程入口不计业务用例。原Node22.23.5 AST/SDK/Web ignored显式真跑，新代码必要格式/diff通过。不是Memory重建、浏览器、PG/OS断电或多机长稳证明。

本批主体提交 `bc15da6`，HTTP/私包撤销/真PG子进程实证提交 `532d3dc`；原Node22实际为v22.23.5。最终必要42项（辅助入口另计）及严格driver 403复验通过；20变更文件敏感特征0命中，非安全审计。PG测试服务已正常停止，合成库与私有CAS证据保留，不删除原资料。

剩余：此后台功能SDK/CLI/管理产品封装/多节点宿主亲和公平与诊断、协作者显式耐久委托/私有发现邀请、全平台QuickJS(WASM)、C#/Agent与全规范World；真实browser/硬件GPU、全项目Node22矩阵、PG多机断电24h长稳及完整安全发布门禁仍欠。未擅部署/网络绕过/外部模型支付或私有外发，不标整体完成。以下历史保留。

## 第五十七批：原General目的协作Web闭环与私有最小态生命周期（从main eeaac42继续）

**主体继续，整体未完成、未达正式发布质量。** 已核交接/ACTIVE/最终D337/D569–570与干净PUBLIC远端 `eeaac42fbb82ea2cd49704e0cc5b8b632f3c5b4d`；不重做阶段56。本批落实请求允许的必要协作Web闭环，详见 [GENERAL_COLLABORATION_WEB.md](GENERAL_COLLABORATION_WEB.md)。**没有交付耐久自治意图/服务调度/进程重启恢复**，不拿Lab/Pi控制面或同内存重建冒称完成。

- /general所有者与协作者互斥挂载，切模式销毁旧凭据/私有态/Worker。所有者明确consent与精确CAS授予/撤销规则驱动，列表/当前版本只属此目的，不发送邀请、不共享令牌。
- 协作者本人身份+明确世界/版本，只取原SDK最小帧；连接/原Sovereign/一个真实规则Tick/交还分别明确操作，无owner完整观察/源码/私包/安装/维护或自动循环。提交元数据与原Engine帧同Tick/revision/root才确认。
- 草稿换世界/准入立即清旧帧和双fence，未重绑不得误推进旧世界。断开/离线/隐藏/晚回执均清态；未启GPU也12秒失鲜清最小态。显示复用原Viewport/Worker/FrameScene/Renderer，不另造画布或前端World。本地清态不等于服务端交还/回滚，需显式交还或owner撤销接管。
- 58个不同定向通过，新增11：Web25（10新）、General19（1新真实Web控制器HTTP）、私库12/原Studio2；原ignored跨AST/SDK/Web在Node22.23.5显式真跑。新链真实三Tick实付守恒、撤销清态、旧版本拒绝、显式新准入/新控制fence、Tick4/交还。非浏览器/PG/耐久进程恢复实证。
- 类型/生产TS/Vite JS与原Worker打包、必要Rust格式/diff/敏感筛查通过；WASM只复用阶段55已有资源，不重复构建或冒称新ABI验收。服务器src/Store/Simulation/ABAC双fence/fog/源能量与提交锁零修改，权限保护未降。

下一首位仍是General耐久服务运行意图/有界预算/原Actor调度/原子进度/暂停取消/真实进程恢复；随后协作发现邀请、全平台QuickJS(WASM)、签名登录/分页编辑发布UI、C#/Agent与全规范World。真实browser/硬件GPU/实际PG并发多机断电长稳、全项目Node22矩阵及完整安全发布门禁欠实证。无新模型/支付/provider调用或私有外发，不重试/绕受管loopback，不擅部署。以下历史保留。

## 第五十六批：General按目的隔离准入到原Actor规则驱动与恢复（从main bb6beb9继续）

**主体继续，整体未完成、未达正式发布质量。** 已核交接/ACTIVE/真实规范及干净 PUBLIC 远端 `bb6beb9c6e4849e0954d095a0f2a9f8d0267c029`。按依赖先接最小协作规则驱动主体，没有重做 WASM；完整链及边界见 [GENERAL_PURPOSE_RULE_DRIVE.md](GENERAL_PURPOSE_RULE_DRIVE.md)。

- 同一原准入表新增明确目的，旧记录默认只限Studio；Memory键/PG迁移0012为世界+主体+目的，CAS版本/列表/配额独立，不把源码许可升级控制。所有者显式consent，仅Active正式Human与原WorldConnect/Read/Edit scopes，无Agent/组织共享扩权。
- driver只得六字段身体帧，不得owner观察/元数据/源码/私包下载；复用原Action/GeneralInput，只限Connect/Sovereign Acquire/Release/空规则Tick，拒Close/安装/维护/结构命令/无规则物理推进。仍在原TickCommitter提交锁核目的/精确grant、ABAC/宿主双fence/RulePermit；费用沿用原世界配置，有源夹具正EnergyQ实付守恒，不把CPU配额叫生物能量。
- 控制租约绑定grant版本；撤销或重授权清失效旧lease但不重置fence，不偷抢有效人类租约。新私有`/4`日志显式记录协作版本，旧owner日志仍严格owner；恢复不把历史grant当当前权限，同grant重启取原幂等回执，新grant不能复用旧key/控制fence。duplicate只回原提交，不假带当前帧。
- TS SDK复用原传输/有界JSON/信封编码；owner显式列表与CAS授权，driver明确观察/连接/控制/单Tick/交还，无后台循环或重试/邀请/外部provider。真实Node→HTTP→原Actor→私有恢复及原Engine帧校验已跑；不是协作Web面板或耐久自治完成。

68个不同必要定向通过，新增17，重跑不累计：General18（7新，含Node）、私库12（1新）、原Studio2、SDK33（8新）、Store3（1新）。相关Node22.23.5/TS类型、SDK与Engine构建、Rust锁定离线/必要格式/diff/敏感筛查通过；原AST/SDK/Web跨工具链显式include-ignored真跑。首次恢复日志边界与Node帧ID夹具错误已实测修正，没降生产保护或原帧validator。

下一仍欠General耐久服务作业与协作Web/发现产品流、全平台QuickJS(WASM)、签名登录/分页编辑发布UI、Agent/C#规则链与全规范World等。PG迁移/SQL仅类型与人工审，实际PG并发/多机断电长稳、真实browser/硬件GPU、C#本批重编、全项目Node22矩阵与完整安全发布门禁欠实证。不重试/绕既往受管loopback、不擅部署或发私有内容，无外部模型/支付调用；保原fog/有源事务/私有隔离，不以阶段结束标整体完成。以下历史保留。

## 第五十五批：已有SDK恢复、新General真实WASM与有界实付运行（从main 724825d继续）

**主体继续，整体未完成、未达正式发布质量。** 已核干净PUBLIC远端、阶段54交接/ACTIVE/真实规范与历史官方SDK32记录；没有重复下载已有工具。详见[GENERAL_WASM_BOUNDED_RUN.md](GENERAL_WASM_BOUNDED_RUN.md)。

- 重新核对既有官方SDK32与wasm-bindgen0.2.130归档SHA256，使用原Clang22.1/llvm-ar。标准构建新增显式WASI_SDK_PATH前置检测与目标CC/AR优先级；不改Rust浏览器目标、不下载、不用旧产物充数。
- 恢复clang后暴露原rquickjs-sys无unknown-unknown绑定/C运行时缺口。将原生QuickJS宿主依赖放入正确target，浏览器明确拒含Tier1的本地整体安装/执行，不假执行或跳过规则；服务器沙箱/预算/有源费用不变。**不是完成规范QuickJS-NG(WASM)：浏览器本地Tier1仍欠。** General远程消费原生权威Tier1身体投影已验。
- **新wasm32-unknown-unknown标准构建、同版胶水、真实General ABI与生产Worker打包实际通过**；新WASM SHA256 `2524a68fe82e116fd49cc8a9c56570ce3a6064649e27677942b5986e8b2b4e23`，12,264,321字节，dev profile非发布性能结论。没有WASI导入，不是复用旧产物；仍没有真实browser/GPU验收。
- /general明确1..128有界运行，经原SDK/原Actor逐Tick实付与确认，固定世界/双fence/会话互斥，独立幂等键；撤销/失败即停，不重试/续租/抢控制/自动安装维护。停止/断线/隐藏清私有状态，报告已确认数量及末请求不确定性，不把取消说成服务端回滚。是在线客户端调度，不冒称耐久自治或协作准入完成。

109个不同定向用例通过，新增10，重跑不累计：Engine TS54、Web DOM/路由15（4新）、真实WASM/构建10（5新）、原生沙箱7、实付/可复现夹具1新、server General11/私库11；含真实Node→Rust有界实付/守恒、签名链/撤销/维护换装/私有恢复。相关类型/tsc/AST与SDK构建/Vite/必要格式/diff/敏感筛查通过。实际Node22.23.5是本批新实证，不把此前Node24更名，不等于全项目Node22矩阵通过。

下一主体/发布阻塞：按目的隔离的General协作准入与耐久自治、全平台QuickJS(WASM)、签名登录/分页编辑发布UI、Agent/C#与全规范World等；C#本批重编/真实browser硬件GPU/实际PG多机断电长稳与完整门禁仍欠。原Store/Simulation/ABAC双fence/fog/最终提交锁撤销和源能量事务保持。无外部模型/支付或私有内容外发，无浏览器工具、不绕过既往受管loopback，不擅公开部署。以下旧批为历史，阶段54缺clang/新ABI未验已由本批恢复，不修改旧证据。

## 第五十四批：远程General最小观察到原Worker/FrameScene/Renderer（从main a1be0c3继续）

**主体继续，整体未完成、未达正式发布质量。** 已核阶段53交接/ACTIVE/规范7.1与干净PUBLIC远端 `a1be0c3fb2271a0930b467fc680b59ae52db2948`；不重做既有用户/规则链。完整边界和父代理验项见[GENERAL_REMOTE_RENDERER.md](GENERAL_REMOTE_RENDERER.md)。

- 新原权限GET general/frame，只传六字段元数据与每身体serial/Q32位置/半径；恢复后再准入、仅Private所有者、无宿主认领/控制或Tick。无快照/源码/账户/场网格/令牌给Worker。
- 原EngineHost/Dedicated Worker显式generalRemote能力握手→原EntityProvider圆体函数/FrameScene/WorldRenderer，不新造Canvas渲染器，不启动本地第二份World。禁本地STEP/START/GENERAL_WORLD，Arena绑定/原fog保持。坏帧清态并封闭Worker，晚帧不能复活。
- /general明确开启GPU只读显示，5秒只读复验、12秒授权失鲜/启动超时清除；断线/下次复验撤权/隐藏/换世界清私有画布、观察、双fence、计划/包清单并终止Worker。不是即时撤权推送或信息追回；不自动推进/续租/降级假画布。只在真实匹配观察回执+原FRAME_RENDERED后报告GPU提交，不称硬件实测。

130个不同定向用例通过，新增10：TS engine54（3新）、Web DOM/路由15（3新）、Rust engine17（2新）/wasm原生6/共享协议2、Arena表现13/General软件回读1新、server General11（1新）/私库11。真实HTTP→最小投影→原生场景，原Node用户链签名安装/维护/恢复仍执行；原生llvmpipe LLVM19.1.7 GL Cpu回读实际圆半径/清屏及原Arena fog，不是浏览器/硬件GPU。必要类型/tsc/Vite/原生检查与敏感筛查，不是集中全门禁。

**新wasm32构建未成功**：已用脚本原支持的既有官方wasm-bindgen0.2.130，锁定公开依赖正常下载后因缺clang失败；未生成新产物，未换依赖/网络绕过。Vite仅复用旧wasm，新握手会主动拒绝旧产物显示General，不能声称浏览器功能已可验收；先补正常工具链重编与新ABI实证。执行器无浏览器，不重试既往受管loopback拦截，父代理仅在已有获准可达部署验文档清单。

下一主体/发布阻塞仍有General协作自治/签名登录/分页编辑器，Agent/C#及全规范World等；Node22/C#本批重编/新wasm32 ABI/真实browser硬件GPU/实际PG多机断电长稳仍欠。原Store/Simulation/有源计价/提交锁零改动，保ABAC双fence/fog/失败原子性。主体齐后集中完整门禁/安全运行时/正式发布，不以本批标整体完成。以下旧批为历史。

## 第五十三批：General真实Web身份/世界/私有规则与明确实付控制（从main e495479继续）

**主体继续，整体未完成、未达正式发布质量。** 已核阶段52交接/ACTIVE/真实规范/干净远端 `e4954793989102a71f02bc47e417c4b5c8d61342`；不重复AST/私库/Actor/隔离链。完整流程与父代理浏览器验项见[GENERAL_WORLD_WEB.md](GENERAL_WORLD_WEB.md)。

- `/general`真实React工作台与Arena入口；原正式能力令牌→原认证Active本人主体/登记钥读回→RYWL明确创建、原worlds发现/精确ID选择→只读权威观察→明确连接宿主/取得原Sovereign→一个真实实付Tick。读取不偷lease、不启练习身份/本地World；无假成功或自动重试。
- 复用原PrivateRuleClient：版本发现、显式完整精确计划、可信下载验签、明确Epoch安装、接受逐身体维护报价后逐Tick隔离换装；只显示原Actor提交回执及原恢复状态。不改变阶段52维护语义、不弱化原ABAC/双fence/fog/Tick事务/有源计价/最终提交锁撤销，Store/Simulation零改动。
- 新GET auth/session沿原authenticate/subject/Active，不返回token；私有缓存no-store/Vary，原worlds发现同补。UI同origin、客户端HTTPS/本机开发地址，认证fetch禁止重定向/附带cookies，15秒/响应硬界、仅内存、退出中止、晚回执失效；拒绝观察整数截断。
- **没有远程General图形画布**：未适配原Renderer/Worker，不把本地载同包或Arena投影伪装远端。规范7.1 Worker独占GPU保留；下一主体优先真实远程General只读观察桥接，然后协作准入/自治调度；浏览器直接签名登录/注册、规则发布/映射编辑器、分页也仍欠。

58个不同定向通过，新增10，重跑不累计：Web DOM/路由12（8新）、SDK25、General10（1新）、私有规则11（1新）；真实AST/SDK/Web用例均显式执行。生产编译GeneralWorldSession经真实Node→Rust HTTP完成大u64本人/创建发现/双fence/安装4实付5/撤销拒绝/6维护→新会话隔离→7维护8安装9实付、新App私有CAS恢复同根。DOM与HTTP分别验证，**不称真实浏览器端到端**。Web typecheck/tsc/直接Vite打包通过，Vite仅复用既有wasm，未重编wasm32。

本执行器无浏览器工具；父代理仅可在已获准可达部署做文档清单实测，既往受管loopback拦截不重试/绕过。无新外部模型/支付授权，未调用真实provider或发送私有内容。Node24.22.1不是Node22；C#本批重编/wasm32/真实browser硬件GPU/实际PG多机断电24h长稳仍未验。其余真实主体与完整发布阻塞仍见下节/新文档；未跑全workspace/完整安全/soak/CI轮询，不以本批结束宣布整体完成。以下旧批为历史。

## 第五十二批：CLI/TS私有规则用户链与撤销后实付隔离到Epoch续跑（从main b9fad2b继续）

**主体继续，整体未完成、未达正式发布质量。** 已核交接/ACTIVE/远端从干净 `b9fad2bace0bac283cb3c98d2e3eb40c30426676` 续；不重复AST/私库/Actor。完整入口与成本语义见[PRIVATE_RULE_CLIENTS.md](PRIVATE_RULE_CLIENTS.md)。

- 用户可用CLI `rule publish/list/download/observe/connect/acquire/tick/install`，TS SDK `PrivateRuleClient` 发布发现可信验签、原Sovereign双fence逐Tick与显式安装。必须传既有可信登记公钥；下载每层签名/CID/精确DAG有界复验，不信自附钥/解析器自报列表。CLI共用唯一Rust RYPK/IR复验、全通过才wx写根包；当前极简HTTP仅明确回环，不明文带Bearer出站。TS不复制包/IR/AST编译器，不伪称本地IR等价证明。
- 补撤销停在非Epoch后的明确产品机制：只有已撤销或已经显式隔离的有源正价Tier0 World，才能 `recover` 一个真实维护Tick；旧图/新图均不执行，受控加速度清零、原物理正常滑行。每活控制身体从原来源账额外热耗 `4×原rule_cost_micro_per_gas` EnergyQ，单列maintenance_paid_q，非伪造规则gas/CPU能价；CLI/SDK必须明确接受观察报价。不能免费Tick、补能、健康世界廉价跳规则、自动降级或越Epoch；到紧邻Epoch仍须显式install激活新图并恢复原实付。
- 维护走原完整阶段/Tick/配置及状态根/私有v3日志、APM/幂等/恢复，保revision==tick、失败原子性和源闭包。历史旧签名只存档/历史重演；维护期原RulePermit守卫完整新待激活闭包，**原Store撤销/fence提交锁零修改**；待装包resolve后撤销仍最终拒绝，另一个有效计划必须用户明确请求。
- 原命令信封兼容无损u64字符串主体输入，原数字输入/序列化不变；TS实际大于2^53身份经原Actor，不截断权限主体。SDK拒绝不安全Tick/fence。原测试未决Promise仅补never类型，不改Provider。

87个不同定向通过，新增14，重跑不累计：SDK25（7新）、原General9/私有规则10（3新，真实AST和SDK前置用例均显式执行）、Rust客户端/HTTP2新、CLI真实子进程1新、仿真规则10/生命8、协议22（u64兼容1新）。真实CLI/Node双包签名→可信下载→实付位移、撤销停5→实付维护6/7→Epoch8安装/9实付、隔离中及安装后新App文件CAS恢复同根，原提交锁撤销竞争仍拒绝。必要SDK typecheck/build、server/CLI锁定编译、格式/diff与敏感筛查；不是完整门禁。

后续优先General浏览器真实用户流/协作主体准入/自治调度与Agent独立赛道真实缺口；C#规则消费、生产TLS、全规范跨种类锁/Schema迁移/组织信任、完整生命材料生长遗传、多Chunk/三维/任意World、presence/离线多主仍欠。本批不是任意rpack或全World完成；隔离仅全Tier0、有源正价且有活体能支付维护的明确剖面，无来源/欠费/无活控制身体不能强行续跑。

实际Node24.22.1/TS5.8.3；Node22/C#本批重编/wasm32/真实browser硬件GPU/实际PG并发SQL多机断电24h长稳仍须实证，MemoryStore+文件CAS不作替身。旧6个PvP_Custom Actor夹具补真实lease，不降生产fence。主体齐后集中完整门禁、安全运行时及正式发布打磨，未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询，不以本阶段宣布整体完成。以下旧批次为历史口径，以本节为当前状态。

## 第五十一批：私有签名精确规则包组到原权限Actor实付Tick/恢复（从main ef19014继续）

**主体继续，整体未完成、未达正式发布质量。** 已核阶段50交接/ACTIVE/远端均为干净 `ef19014a7be910f63fdef0ee516ac526648aa3e2`；不重做AST。实际入口/协议和明确边界见[PRIVATE_RULE_PACKAGES.md](PRIVATE_RULE_PACKAGES.md)。

- 原Tier0三条目叶子RYPK不放宽；新版本化 `rynvek-private-rule-release/1` **整体签名**owner/精确身份/实际制品CID/直接锁，每层重验登记公钥、原唯一包解析与图IR。依赖仅同owner，拒绝范围/缺失/撤销/错CID/冲突/环；闭包16包/深度8/总1MiB。不是未签外层锁，不是JS import或全规范跨种类锁/迁移已经完成。
- 原Agent/音频/视觉私库存取核心泛化、旧表/接口原位；复用原Active/HubPublish/WorldRead、私有CAS、不可变版本/配额/墓碑。真实HTTP发布/发现/逐级验签下载/撤销，不写公共Hub索引/CAS；他人404，不因知道CID即可下载，不在发布时隐式执行或拿控制权。
- 显式 `action: install` 经原ABAC/Active/人类Sovereign/宿主与控制双fence；完整闭包一对一映射已有绑定模块，下一Coupling Epoch与原Tick共同原子提交，不能拆包组或覆盖Tier1。保留组件动态态、生命/来源账、价格与身体，原Phase07计量/正价EnergyAccount实付；欠费/坏输出不发布规则、摄食、根或回执。失败保原帧速率扣费，但修复提前污染命令幂等键的问题；成功/恢复仍走原回执/APM。
- 安装完整签名证据CID进入原配置根/快照；私有v2安装WAL与原v1 Tick兼容，原锚点/重仿真及签名DAG恢复同根。最终撤销/pins复验在原Store提交锁/PG事务内与editor/Active/fence/前驱一起线性化，非只在路由预检。撤销不抹历史，但后续活Tick拒绝；非Epoch暂停后安全续跑流程仍欠，不假称任意时刻可换装。

主体提交：`ed70a2f` 私库核心复用/原提交守卫；`fd45c35` 耦合候选/来源根；`a75250e` 签名发布发现下载；`73dd19c` 原Actor实付在线安装/私有恢复及真实链测试。文档HEAD由交接/原Page/远端记录，不制造自引用；PUBLIC普通push main核远端，不force/no-verify/admin/改历史/保护/网络策略或删原资料。

### 快验与剩余主体/发布阻塞

46个不同定向通过，新增7，重跑不累计：私有规则7（含显式真实Node AST重复构建→双签名依赖→在线安装/实付位移/私有恢复；301Tick含300锚点后缀；锁/身份/隔离/撤销/菱形冲突/环/深度/Epoch/双fence/欠费原子性/Store提交锁撤销竞争）；原General权限9；原私库Agent2/音频3/视觉2共7（另一Agent ignore未计）；原仿真规则10/生命8；原命令门5。必要server/CLI原生锁定编译、PG代码仅类型、新增主体格式/diff/必要敏感筛查，不是全门禁安全审计。

有界Tier0共同安装链已真实贯通，**不是任意World/任意TS/全规范私有rpack所有剖面完成**。专用发布/安装CLI/TS/C#产品工作流、跨种类锁/Schema迁移/组织信任、撤销后非Epoch安全续跑、General浏览器流/协作主体准入/统一调度、完整生命材料/生长/自然遗传迁入、多Chunk/三维/任意World、Agent独立赛道和presence/离线多主仍欠。旧6个PvP_Custom Actor夹具补真实lease，不降生产fence。

实际Node24.22.1/TS5.8.3；Node22/C#本批重编/wasm32/真实browser硬件GPU/PG并发SQL多机断电24h长稳仍须实证，MemoryStore+文件CAS不是替身证明。未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询；主体齐后集中门禁安全运行时与正式发布打磨，不以本阶段宣布项目完成。以下旧批次保留为当时历史口径，以本节为当前状态。

## 第五十批：真实TS AST、可复验Tier0构建与原World实付链（从main d44e92a继续）

**主体继续，整体未完成、未达正式发布质量。** 已核阶段49交接/ACTIVE/远端均为干净 `d44e92af451b3fde1821f28357235388f03102da`，不重做统一权限入口。依最终规范08的AST/Builder→图→原三遍编译，先补私有规则链必要的真实源码/制品前置，非作者自附TS伪证；精确入口/限制见[TIER0_TS_AST_BUILD.md](TIER0_TS_AST_BUILD.md)。

- `@rynvek/rules/ast` 使用固定TypeScript5.8.3真实AST、纯内存解析、闭合白名单节点；只映射原KernelBuilder/RuleValue全部36出口与Phi/原有界loop，不eval/转译执行/加载作者模块。64KiB、8192节点/深度96；直接Kernel导入+export default、const受控值，非任意TS执行环境。原浏览器主入口不自动引入编译器。
- Node显式本地构建bin→原Graph；Rust `rule build/inspect` 复用唯一RYPK/RYIR与原类型/访问/循环编译。`rynvek.tier0-build.v1`固定三个条目，读取时图重新编译逐字节对实际IR；不带TS或自报来源等价，额外作者TS/换图/换IR拒绝。原Hub识别此剖面共用复验器，分别报告graph_rir等价和ts_source_equivalence=false，不虚报运行沙箱。
- 原 `world build --rule-pack module=path` 明确本地创世安装，只能已有绑定且不覆盖Tier1；再走原World规则/宿主/价格/RYWL校验。真实TS源码→两次同字节构建→原RYWL→原HTTP身份/双fence Actor付费规则实际0.5加速度→私有WAL恢复同根。保原Tick事务/ABAC/fog/fence/源能量，CPU额度不冒称逐指令Gas或生物能价。**不是私库签名依赖发现/验签/在线安装链**，不把本地嵌图冒充已安装注册表引用。

`726d0fe` 真实AST与固定解析器、`72886b2` 原RYPK/Hub逐字节复验、`e4da6e1` 显式本地安装/真实HTTP链；最终文档HEAD由交接及原Page记录，不在本文制造自引用。21个累计更改/新增文件必要敏感模式筛查无命中，提交身份同既有公开基线；PUBLIC普通push main核远端，不force/no-verify/admin、改历史/保护/网络策略或删除原资料。

### 快验与剩余主体/发布阻塞

64个不同定向通过，新增35，重跑不累计：规则SDK44（AST30新增、原Builder7/QuickJS7）、原RYPK构建复验2新增、CLI跨工具链2新增（ignore隔离前置，本批显式--include-ignored真实执行）、原CLI World3、原General权限9、Hub4（签名合法换图/自附TS仍拒绝1新增）。必要TS typecheck/build、原生CLI/server锁定编译（PG代码仅类型）、新增Rust/改动函数格式与diff、必要敏感筛查通过。离线全workspace锁重解算缺其他包元数据，只移动既有TS5.8.3锁记录后frozen一致性通过，未网络下载或绕过。

**下一优先完整私有rpack精确依赖/签名发布发现验签显式安装链**：复用原私库/Hub权限、私有CAS、不可变版本和WorldActor提交/恢复，把包ID/版本/CID/锁/签名/撤销绑定实际安装；本地包不带这些权威引用，不假称完成。TS AST当前仅有界Kernel语法剖面，不声称任意TS。General浏览器流/协作主体准入/统一调度、完整生命材料/生长/自然遗传迁入、多Chunk/三维/任意World、Agent独立赛道、presence/离线多主仍欠。旧6个PvP_Custom Actor夹具须补真实lease前提，禁止降低生产fence。

实际Node24.22.1不是Node22；C#本批重编、wasm32、真实browser/硬件GPU、PG并发SQL/多机/断电/24h长稳未验，llvmpipe仍仅阶段41软件回读。本批未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询；跨Node/Rust回归前置尚未编排CI。主体齐后集中完整门禁、安全与正式发布打磨，不以本批结束宣布整体完成。

## 第四十九批：通用World贯通原权限/双租约fence/私有提交恢复（从main 1581eb2继续）

**主体继续，整体未完成、未达正式发布验收。** 已先核交接、ACTIVE与远端均为干净 `1581eb27928ec7321d4c05243c202629500c3b59`，没有重做阶段48声明/计价主体。优先实现统一权限链的通用领域适配：原认证/Active/ABAC、LeaseManager/CommandGate/APM、原宿主持久租约和TickCommitter/Store提交机制共用，不复制第二套权限、租约或提交架构。详见[GENERAL_WORLD_SERVER_ACTOR.md](GENERAL_WORLD_SERVER_ACTOR.md)。

- 原服务器新增RYWL二进制创建与 `/worlds/{id}/general` 观察/控制；个人组织配额、Private所有者边界。受限World配置→原SimulationEngine真实付费组件/物理/账户→原私有CAS与Store WorldRow/CommitRow，不伪造Arena阵营/比赛。当前仅Human所有者Sovereign精确下一Tick显式调度，无自治30Hz/General浏览器流或协作共享fog；原Arena fog不动。
- 原ranked_host_leases仅扩展给四字段精确受控profile，原排位资格/过期校验不弱化；健康节点不抢占。每请求同时宿主fence与原控制token，重建不得在仍健康同fence下重置计数；执行与回执前重验正式准入/fence。有界消息邮箱、原UUIDv7/主体/时序门/APM；旧token、冒名、缺scope及撤销拒绝。
- 原候选Tick→私有输入日志/每300Tick锚点→**同一TickCommitter.publish_row/Store.append_commit_checked**原子复验ownerActive/fence/前驱→才发布内存。恢复原RYC2全部领域根与至多300Tick真实后缀，恢复1800Tick幂等回执/APM；同键换载荷拒绝，不凭客户端镜像恢复。生命World禁止外部Spawn/免费加速度，只许原回收，规则/欠费/非法命令整Tick不发布；CPU额度不冒称生物能量。
- `1610299` 原Store租约/共用提交点、`db2e07a` 真实服务器主体、`c25de3e` 十个新增定向证明；最终文档HEAD由交接与同一Page记录，避免本文自引用。PUBLIC普通push main并核远端，不force/no-verify/admin/改历史或网络策略，不删原资料。

### 本批必要快验与真实发布阻塞

14个不同定向用例通过，新增10，重跑不累计：真实HTTP general_authority 9、新MemoryStore过期/撤销提交1、原ranked_discovery 2、原过期宿主1、原PvP私有fog/恢复1。含临时Ed25519登录、scope/Active/控制级别/双fence、付费组件真实位移与能量闭包、欠费/坏命令不发布、私有CAS与损坏WAL拒绝、新AppState同MemoryStore+文件CAS恢复/去重、301Tick跨周期锚点+后缀。**不是PG实库/多机/断电证据**；过期测试只调内存夹具边界，未做30秒长稳。必要server原生编译（含PG代码类型）、新增Rust文件/模块rustfmt、diff与必要敏感模式筛查通过；SQL未连接PG执行，不等同完整安全审计。

TS AST与完整私有rpack签名依赖发现/验签/安装链仍欠；通用权限链已接入但General浏览器流/协作主体准入与统一调度仍欠，不称全部Actor完成。单Chunk受控组件不是任意World，完整材料/生长/自然遗传迁入、多Chunk/三维、Agent独立赛道与协作presence/离线多主仍欠。旧6个PvP_Custom Actor夹具须补真实lease前提，不能弱化生产fence。Node22、C#本批重编、wasm32、真实浏览器/硬件GPU、PG并发SQL/多机/断电/24h长稳未验；llvmpipe仍仅阶段41软件回读。本批未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询，主体齐后集中完整门禁、安全与正式发布打磨，不以本批宣布整体完成。

## 第四十八批：通用World声明组件、受控规则与有来源计价（从main 56982a0继续）

**主体继续，整体未完成、未达正式发布验收。** 已先核交接、ACTIVE与远端均为干净 `56982a017d35b49534c36577282f8d03498d8b93`，未重做阶段47。依最短真实依赖，将Arena原组件registry与Tier0/Tier1执行下沉rules共享核心，由原通用World原Phase07消费；不新造平行架构。完整入口/数学/限制见[GENERAL_WORLD_COMPONENT_RULES.md](GENERAL_WORLD_COMPONENT_RULES.md)。

- 原RYWL可选 `components` 声明模板/实例/受限RuleGraph与原RYPK，沿原CPU/QuickJS同旧态、单写者与有界预算执行。只映射两个Q8.8加速度组件到原真实刚体，不开任意身体/生命账户/场写入；按持久serial绑定，退役不复用稠密槽位。
- 有生命身体明确正micro/Gas价，Tier0按实际逐实体Gas、Tier1按原中断粗配额确定分摊后由同一有来源EnergyAccount实付转热；CPU配额本身不是生物能量，paid_q不二次计入守恒。LIF与身体规则控制互斥，无执行Gas不保留免费生命推进；规则故障/欠费/非法输出连同原场与摄取整Tick回滚。
- 原十域根绑定实际定义/动态态/计量，可选RULS进入原快照与文件WAL逐提交重仿真；纯物理四块及无组件LIFE/NEUR不变。只有创世显式安装；原GENERAL_WORLD本地/暂停/Tick0空世界准入及fog/权限/生产fence不弱化。原CLI真实子进程build/run/recover与原生Engine命令已实际消费。
- `cc748c9` 共享规则核心、`f8a5507` 原事务/定价/恢复、`1f10fa8` RYWL/CLI/Engine真实入口、`c72bcdb` 原FrameScene实际规则位移。最终文档后已推HEAD由交接与同一Page记录，不在本文制造自引用。

### 快验与仍存发布阻塞

56个不同定向用例通过，新增13，重跑不累计：simulation规则10/生命8/物理12，Arena Tier1 8/参数Lab2，World registry2/通用库4，Engine通用5，CLI3，原16阶段/10Hz1，原迷雾1。含Tier1粗配额精确分摊、同旧态、欠费/预算/坏输出/沙箱故障整帧回滚、原生命闭世界、serial不误指、合法包络RULS篡改拒绝、原WAL文件恢复与后续根一致。必要server与wasm宿主原生target检查、触及源码rustfmt、diff检查通过。JSON/schema仅语法检查，未运行JSON Schema语义验证。

必要累计凭据/私人联系/敏感文件名筛查无命中、身份同既有公开提交，不等同完整秘密/隐私审计。PUBLIC普通push，不force/no-verify/admin或改历史/保护/网络，不删原资料。未跑全workspace/Clippy/license/SBOM/安全/soak/CI轮询；Node22正式运行时、C#本批重编、wasm32、真实浏览器/硬件GPU、PG多机/并发SQL/断电/24h长稳未验，llvmpipe仍仅阶段41软件回读。

下一主体优先**统一权限Actor及通用World在线命令/观察/持久fence**；TS AST、完整私有rpack签名依赖发现/验签/安装链仍欠，本地内嵌原包不冒称私库链。单Chunk受控组件不是任意World；Neural Soup完整材料/生长/自然遗传迁入、多Chunk/三维、Agent独立赛道、协作presence/离线多主仍欠。旧6个PvP_Custom Actor夹具必须补真实lease前提，禁止弱化生产fence。主体齐后集中完整验收、安全与正式发布打磨，不以阶段完成宣布全项目完成。

## 第四十七批：通用World有来源生命场与可选付费LIF（从main a22fbe5继续）

**主体继续，整体未完成、未达正式发布验收。** 已核对手交、ACTIVE与远端均为干净 `a22fbe586241f11db1dd0f0ae20883943b33872f` 后再实现，未重做阶段46刚体链。选定最短真实端到端：复用原 `EnergyAccount` 与可选付费 LIF，把有来源营养摄取、维护、死亡/命令残骸返还接入通用单 Chunk World；不迁 Arena 生态/Tier1，不双记阵营 Biomass，不把本批宣称为任意组件 World 或项目完成。保持 PUBLIC、认证 gh 与本地持续授权，未改可见性/历史/保护/网络策略。完整来源与剖面边界见[GENERAL_WORLD_LIFE.md](GENERAL_WORLD_LIFE.md)。

- 活体创世空账，营养只来自明确场沉淀与随后「先收款再等额扣场」的摄取；固定体不摄取。维护热耗耗尽库存才死亡；无维护的空仓保持存活。残骸整 raw 返还当前格，亚 raw 或格满余量归热，不记 `carcass_reclaimed`，INV-03 只用摄取 raw 作为当帧 harvest 恒等。
- 闭世界把场衰变记入 `env_dissipated_q`、退役账户记入 `retired_closed_q`，与活体 `stored/dissipated/exported/transferred` 及当前场能对创世能量封闭；返回场的 `returned_q` 不再另加。反应扩散若在无来源时把营养总量抬高，则整 Tick 拒绝。世界级 Biomass/Flux/Neurogel 必须保持 0。
- 可选共享基因组驱动 LIF：仅当本 Tick 有脉冲且 `try_charge_spikes` 成功才写入加速度，否则清零，避免无成本沿用上帧意图。速率、回收比、脑规模与加速度比例是实现剖面 `rynvek.general-sourced-life.v1`，不是规范冻结原值；无可塑性，声明规则主体仍待后续。
- 纯物理快照仍为原四块，生命世界追加 LIFE（启用神经时带原 NEUR），不是旧格式静默回退。命令、摄取、场衰变或回收失败时，原候选 Tick 整事务回滚。CLI/RYWL 可选 `life` 为加法字段；缺沉淀却要求维护的活体在配置期拒绝。

### 实现提交、真实快验与未验

主体与测试提交的 SHA 由文档收尾后的已推最终 HEAD 与同一 Page 记录给出，不在本文制造自引用。

本批新增 10 个不同定向用例（重跑/过滤不累计）：有来源摄取/固定体/饥饿/命令回收/失败回滚/快照与WAL/付费神经/纯物理四块 8，RYWL 拒绝无沉淀维护 1，真实 CLI 子进程生命包构建-运行-文件恢复 1。另重跑并通过既有通用物理 12、世界库其余用例、Engine 通用安装 2、原物理 CLI 1、场库 7、原16阶段/10Hz 与空世界 10000 Tick 无发散。

已核对：无场步 Tick 上等额扣场；场衰变后闭世界仍成立；失败 Tick 不保留摄取或推进时钟；生命快照/WAL 恢复后连续根一致；命令回收按回收比把整 raw 返还当前格；欠费神经不保留加速度；纯物理示例与测试不被生命块破坏。未把「有脉冲时才转向」写成必现，权重是否在首两 Tick 形成可付费脉冲取决于 LIF 状态。未做真实掉电。

定向命令（重复运行不加计数）：

```sh
cargo test -p rynvek-simulation --locked --test general_life
cargo test -p rynvek-simulation --locked --test general_physics
cargo test -p rynvek-simulation --locked --lib
cargo test -p rynvek-world --locked --lib general::tests
cargo test -p rynvek-engine --locked --lib engine::tests::general_package
cargo test -p rynvek-cli --locked --test general_world
cargo test -p rynvek-field --locked --lib
```

必要累计敏感筛查无命中、提交身份与既有公开提交一致，不等同完整秘密/隐私审计。未跑全 workspace/Clippy/license/SBOM/soak/CI 轮询，没有外部部署/模型或支付。真实浏览器 WebGPU、硬件 GPU、真实 PG 多机断电/长稳未验；阶段41 llvmpipe 仍仅软件回读。当前若有 Node24 检查也不算 Node22 正式运行时验收；本批未重编 C#，也没有新的 wasm32 产物。完整全门禁留主体齐后集中执行。

### 下一真实主体与发布阻塞

仍待：通用 World 声明组件与受控规则（含 TS AST/规则计价接入）、统一权限 Actor、私有 rpack 签名发现/验签/安装链、多 Chunk/三维物理、Agent 独立赛道、协作 presence 与离线多主。旧 6 个 `PvP_Custom` Actor 夹具仍缺真实分级 lease 前提，不得弱化生产 fence 迁就。参数实体 CPU 配额仍不是生物能量；Pi 仅经 MIT 核对的设计参考，不引外部模型或支付。本批不是全项目完成，也不把单 Chunk 生命场称为任意 World 已交付。

## 第四十六批：非RTS World→原ECS物理→原场景与真实文件恢复（从main 83427c0继续）

**主体继续，整体未完成、未达正式发布验收。** 已核对实际规范§1.2/§2.1/§6.2与旧静止样例，未重做阶段45参数组件。原16阶段内核现在实际承载无阵营/无主脑的刚体World，不再默认放四个静止坐标与500/200/50资源。保持PUBLIC、认证gh与本地持续授权，未改可见性/历史/保护/网络策略。完整来源、契约与命令见[GENERAL_WORLD_PHYSICS.md](GENERAL_WORLD_PHYSICS.md)。

- 原Tier2 `rynvek-spatial::circle` 放置真正定点积分/碰撞核，复用原唯一 `Q32_32` 乘除语义；原 `EntityRegistry` + 分页刚体列承载身份/位置/速度/半径/质量/fixed，不是参数表模拟运动。Phase01命令→Phase09四个辛欧拉子步/质量冲量/壁反射/完整有界接触枚举→重建原CSR→原不变量/十域根，任一失败整个候选Tick不发布。每次接触立即落实速度界，不放任密集接触放大越界中间态。
- 原唯一RYWL解析/写入器→严格 `rynvek.general-world.v1` 与显式 `rynvek.general-circle-ecs.v1` 创世→原Engine/Worker明确安装→原STEP/START与 `EntityProvider/FrameScene/WorldRenderer` 实际消费身体位置/半径。新 `EngineHost.loadGeneralWorld` 只发明确包字节；只许本地、暂停、Tick0空世界，远程Arena/既有世界拒绝；没有关闭原迷雾或生产fence。
- 128体、半径0.25–2、质量1–1024、每轴速度±8/加速度±32、四子步/四轮求解是实现剖面，不冒充原规范值；不使用32邻居截断漏碰，不收敛/越界则整Tick拒绝。全权威整数，f32只读投影；非CCD/旋转/多Chunk/三维或完整物理能量定理。创世/本地结构命令是明确外部编辑输入，非自然生长或免费营养；原宏观INV、CPU额度与机械质量不冒充通用生物能量。
- 原RYC2与RYSS容器2、CMIT/COMP/FILD/RNGS保存原注册表分配器/墓碑、全部刚体、五场参数/当前缓冲/未来sources、时钟与宏观账户；逐实际领域重算根，不只验包络。恢复文件8MiB/解压16MiB/WAL32MiB硬界、世界/创世基线/顺序/保留位/精确提交核验；先原日志sync_data后发布，撕裂与未提交命令尾部不执行；I/O错必须废弃writer恢复。CLI输出/日志独占新建，不覆盖已有资料。
- 删除后原稠密swap-remove与组件列一致；接触按持久ID顺序。原GPU拾取消息改为 `entitySerial`，不再把稠密槽位当持久身份；Rust/TS当前双端同步，无旧字段fallback。保留原 `protocol_messages.json`，只消费新增v2当前夹具，同时补齐先前主题命令/事件覆盖遗漏。

### 实现提交、真实快验与未验

主体实现：`4d9da19`；物理归位/原恢复器硬界/身份及当前协议收口：`7a4fb1b05d63423f5a352b3656b2a34d75325d4d`。文档收尾后的已推最终HEAD由同一Page与远端核对记录给出，不制造文档自引用SHA。

61个不同定向用例通过（新增22，重跑/过滤不累计）：Tier2物理公开边界与原定点舍入2、物理ECS/事务/源项/完整语义快照/逐提交WAL/撕裂与未提交尾/容量速度/身份12、RYWL非RTS严格配置2、原Engine命令/安装及权限4、Rust协议4、原FrameScene提供者3、当前双端夹具2、真实CLI子进程1、原日志读取/恢复2、原快照读取2、原迷雾不泄露1、原16阶段/10Hz门控1、TS宿主/协议25。

实际CLI子进程从示例JSON打原RYWL→4 Tick真实积分/碰撞→创世快照与每步sync_data文件WAL→文件回读逐提交重仿真，恢复根、身体与原FrameScene完全相同；已有包/日志拒绝覆盖。另有八步恢复后连续根、质量冲量、静态体/壁、晚阶段不变量失败整Tick回滚、原注册表墓碑/单调序号、重打包合法容器但篡改COMP或FILD源项仍拒绝、尾随/解压界、128体与公开核极端i64输入证据。不是仅打印最终内存镜像，也不把文件截断模拟称真实断电。

必要CLI检查、`rynvek-wasm`宿主原生target类型检查、Engine TS build/typecheck与原web类型检查通过；没有本批wasm32产物或真实浏览器执行证据。当前Node24.22.1的类型/单元检查不是Node22正式运行时验收。本批必要累计敏感筛查无命中、身份与既有公开提交一致，不等同完整秘密/隐私审计。

定向命令（重复运行不加计数）：

```sh
cargo test -p rynvek-spatial --locked --lib circle::tests
cargo test -p rynvek-simulation --locked --test general_physics
cargo test -p rynvek-world --locked --lib general::tests
cargo test -p rynvek-engine --locked --lib engine::tests
cargo test -p rynvek-engine --locked --lib protocol::tests
cargo test -p rynvek-engine --locked --lib provider::tests
cargo test -p rynvek-engine --locked --test protocol_fixture
cargo test -p rynvek-engine --locked --test arena_render hidden_entities_never_enter_gpu_instances_even_if_fog_layer_disabled -- --exact
cargo test -p rynvek-cli --locked --test general_world
cargo test -p rynvek-journal --locked --lib reader::tests
cargo test -p rynvek-journal --locked --lib recovery::tests
cargo test -p rynvek-snapshot --locked --lib reader::tests
cargo test -p rynvek-simulation --locked --lib engine::tests::sixteen_phases_run_each_tick_and_field_steps_at_10hz -- --exact
cargo check -p rynvek-cli --locked
cargo check -p rynvek-wasm --locked
pnpm --filter @rynvek/engine typecheck
pnpm --filter @rynvek/engine exec vitest run test/host.test.ts test/protocol.test.ts
pnpm --filter @rynvek/engine build
pnpm --filter @rynvek/web-platform typecheck
```

### 剩余主体与发布阻塞

本段是真正无RTS实体World与物理ECS闭环，但仍仅单Chunk圆形刚体剖面，不是任意World、全物理生态或通用多机服务器已交付。通用World的声明组件/受控规则与神经/有来源生命场/统一权限Actor、TS AST与完整私有rpack精确签名依赖发现验签安装、多Chunk物理权威迁移、Agent独立赛道/实际受控智能、协作presence/离线多主仍欠。既有Stage43–45生命/Tier1在Arena，不伪称已迁入通用World。Pi仍仅MIT核对的设计参考，无外部模型/支付授权。

原6个旧PvP_Custom Actor夹具真实租约前置整改仍待集中门禁，不弱化生产fence。主体齐后集中完整workspace测试/Clippy/license/SBOM/安全/运行时/发布打磨；本批未跑这些全门禁/soak/CI轮询。Node22、C#本批重编、真实浏览器WebGPU/硬件GPU、真实PG多机断电长稳未验；llvmpipe仍仅阶段41软件回读。不得因本批或M0–M8标签标整体完成。

## 第四十五批：原RYPK Tier1组件执行与Studio/Lab真实消费（从main a426d11继续）

**主体继续，整体未完成。** 复用原容器/QuickJS与Phase07，原owner/editor只构建不执行→明确Epoch安装→真实组件变化/粗CPU回执→快照/WAL恢复与原Lab精确源捕获；不是文档占位或新建假World。能力、来源、限制与入口见[TIER1_COMPONENT_PACKAGES.md](TIER1_COMPONENT_PACKAGES.md)。PUBLIC普通续推授权保持，未改可见性/历史/保护/网络策略。

- core承载原唯一RYPK解析器，world复用；限定rule.json/rule.js/可选rule.ts，构建不执行顶层，不加载URL/模块/插件，TS只作可选来源，不伪证已自动转译。
- 原模板/实例的显式reads、精确有序整数写集；Tier0/Tier1读同一旧态并组件整组原子提交，声明冲突安装时拒绝，运行错或非法输出回滚参数候选。新鲜Context无跨Tick隐态，原500000共享CPU配额/QuickJS堆栈限制，回执不是生物能价。
- `/studio` 原工作台新增“构建受控RYPK（不执行）”与“显式提交Tier1制品”；原修订/权限/fence/撤销/重做/私有读回。身份断开或401/403/404清私有草稿和待提交包，不留假可写结果。原Lab参数模型v2带原包精确字节，不静默丢Tier1。
- Arena快照15、CURRENT_15、Lab kernel/8；线上Arena JSON仍/2、WAL仍2。不接旧格式、不删资料。仅有界参数组件，仍欠通用World/真实物理ECS、Tier0 TS AST/完整rpack依赖与私有Hub安装链、Agent独立赛道/实际智能、协作presence/离线多主等主体。

### 定向验证与未验

实现提交：`b97cf008336339890f1d202bb68bfcfae99592c9`（原容器/QuickJS权威组件链、私有构建、源捕获与恢复实证）、`dcd3fbe9aca9877b366a986aacaa38e635640c7d`（原Studio/SDK明确应用、制品摘要及身份清理）。

55个不同定向用例通过（新增13，重跑不累计）：Tier1原包/访问/整数类型/同Tick事务/沙箱边界/真实Lab扫描8、原Lab参数模型/四表3、原QuickJS7、原唯一包解析器4、原World参数registry2、真实本机私有HTTP/WS Actor与WAL1、Studio远端/协作/参数SDK11、Lab TS模型4、原React Studio/Lab15。

`examples/rules/tier1-counter.ts` 经当前tsc编译为单文件全局JS，真实构建API只出RYPK、不安装；非所有者404，显式Epoch4提交，Tick6组件值精确为9，Epoch8删除后冻结12，私有源码读回与完整源捕获一致。原初始快照文件回读→全部真实已提交WAL文件副本逐条重演（含安装/执行/删除）→逐根核对Actor成功；另含原快照恢复后继续Tick、4组seed/参数轨迹与实际四表Parquet，不拿内存镜像当恢复。Store/CAS仍为本机内存夹具，文件回读不等于PG/进程崩溃/物理断电验收。

必要RTS库检查、真实server测试编译、Studio/Lab及被修复box清掉的必要依赖build、web类型与diff检查通过。开发中box曾超时并恢复较早快照，部分未提交测试/产物丢失；已在保留主体基础上恢复快验、固定lockfile恢复本地依赖，未重做已推阶段44。定向测试补上原registry测试遗漏的错误码import；旧PvP_Custom Actor测试夹具缺真实排位lease的前置问题曾触发正确fence退役，本批新增测试使用真实私有PvE单席位，不关闭或绕过fence。其余6个未选中旧Actor用例不计通过，夹具与真实租约前置对齐仍属集中门禁整改项。

本批必要更改/新增文件凭据、私人联系信息与敏感文件名筛查无命中，提交身份与既有公开提交一致；不是完整秘密审计。未做全workspace/Clippy/license/SBOM/soak/CI轮询或外部部署/模型/支付。

本批定向命令（重跑不累计，其他过滤用例不算通过）：

```sh
cargo check -p rynvek-rts --lib --offline
cargo test -p rynvek-server --test active_transactions tier1_private_build_explicit_epoch_tick_and_wal_restore --offline
cargo test -p rynvek-rts --features arrow --test tier1_rules --test lab_parameters --offline
cargo test -p rynvek-rules --features quickjs --test sandbox_quickjs --offline
cargo test -p rynvek-core package::tests --offline
cargo test -p rynvek-world --test parameter_registry --offline
pnpm --filter @rynvek/studio exec vitest run test/remote.test.ts test/collaboration.test.ts test/parameter_registry.test.ts
pnpm --filter @rynvek/lab exec vitest run test/parameters.test.ts
pnpm --filter @rynvek/web-platform exec vitest run test/studio_workspace.test.tsx test/lab_workspace.test.tsx
```

Node24检查不是Node22运行时验收；本批C#重编、真实浏览器WebGPU/硬件GPU、真实PG多机断电长稳仍未验，llvmpipe仍只是阶段41软件回读。完整workspace/Clippy/license/SBOM/安全/运行时/发布验收留主体齐后集中执行，不以本批或M0–M8宣布项目完成。

## 第四十四批：野生有来源供能与原生命因果链（从main 56231b5继续）

**主体继续，整体未完成。** 保持PUBLIC普通续推；沿用本地checkout与认证gh持续授权，未改可见性、历史、保护或网络策略。来源、完整契约、实际入口、格式与未验边界见[WILD_SOURCE_ECOLOGY.md](WILD_SOURCE_ECOLOGY.md)。

- 严格区分原规范§1的自然选择与§5.5的RTS虫潮导演。新增显式 `ecology.wild` 创世profile；角色非空时不创建免费波次导演、不记野生伤害适应度/死亡精英，不抹掉原RTS玩法。仍是PvE容器和原五场/玩家/胜负，不伪称一般无阵营World已完成。
- 野生任一配置兵种：空内能/空结构种子→当前格Nutrient实际扣款→原同一身体池→维护/脉冲或连续脑/原ORCA实际位移/EMIT/规则Gas实付。显式野生自主脑移动不再被玩家默认Hold封死，不借阵营Biomass，不重复Harvest，也不靠导演攻击路标。
- 从同一身体化学预算完整milli转材料并另付合成热耗；新增 `converted_u` 与真正外源 `external_u` 分列并核对原energy exported，不把同份营养再领一次。长满→亲本付费/材料与HP盾真实拆分→Philox单亲变异/邻格/真实脑→子代再次摄食；野生不要求玩家建筑，以显式独立存活上限约束。死亡走原同一遗骸能量/材料账，饱和/亚raw余量保留，重试不重复返还。
- 创世位置按实际地图校验，越界/堵塞/初始主脑/重复野生格拒绝；1–16种子、1–128存活、0–256 raw/Tick摄食是计算/规模边界，不冒充规范冻结平衡。无食物/0摄食即按真实代谢死亡，不从原兵种造价生成免费尸体营养。野生来源/种子/上限本批创世固定，普通参数补丁不能重播种。
- `/studio` 原JSON创建与 `StudioClient.create` 接入，原owner/editor私有只读 `ecology_contract` 回执→原已有世界Arena运行/重连；`/lab` 原W3扫描/0‰消融同时消费野生复制参数，显式JSON建议不加营养。原四表/Decimal128/BigInt/React新增内部材料转入，纯野生身体对照可关闭玩家角色，但容器仍有玩家主脑。
- 快照Arena/14、CURRENT_14、Lab kernel/7；线上Arena JSON仍/2、WAL仍2；当前生态契约必须完整给出wild，无旧格式fallback，不删原资料/历史。单材料二维面积/原HP盾/环境沼泽再生边界如实保留，非完整化学/三维物理或全世界封闭能量定理。

### 定向验证与未验

实现提交：`5021737`（野生权威来源链/材料转账与必要当前夹具）、`329649e965fa8cdd23486982c91321378e8e3d2e`（原Lab/Studio/SDK/工作台真实入口与精确分析）。90个不同快速用例通过（新增10，重跑不累计）：野生来源/生长/复制/死亡/收费/恢复5、真实Lab四表4、真实本机私有创世HTTP/拒绝/读回1、既有生命材料规则14、既有首摄食3、Lab TS42、原React Lab/Studio14、Studio远端/协作7。零能量初始快照、真实Tick、文件WAL重演根一致，非Forager摄食、源耗尽/无食死亡、原脑移动/EMIT/连续计算实付、野生规则欠费整组拒绝均有定向证据；不把重跑或未选中测试累计。

必要server库检查（含engine）及真实API测试编译通过，Lab/Studio build、web类型与diff检查通过。更改/新增文件必要凭据、私人联系信息、敏感文件名筛查无命中；已配置提交身份与既有公开提交相同，不是完整秘密/隐私审计。未跑全workspace/Clippy/license/SBOM/soak/CI轮询，没有外部部署/模型或支付。

真实浏览器WebGPU、硬件GPU、真实PG多机断电/长稳未验；阶段41 llvmpipe仍仅软件回读。当前Node24检查有项目Node22引擎警告，不算Node22正式运行时验收；本批未重编C#，此前同步源码的编译验收仍欠。完整全门禁、验收、安全/发布打磨留主体齐后集中执行。

### 下一真实主体与发布阻塞

继续依真实规范和实现核对推进一般World组件/规则、TS-rpack/物理ECS，独立Agent赛道/受控实际智能，协作presence/离线多主等欠项。参数实体CPU配额仍不是所有World插件的物理能量；Pi仅经MIT核对的设计参考，不引外部模型或支付。本批不是全项目完成，不等待逐小切片再授权。

## 第四十三批：有来源结构增长/真实体量消费与动作规则计价（从main 559a01a继续）

**主体继续，整体未完成。** 保持PUBLIC，不因公开状态停工；沿用本地checkout/已认证gh普通push授权，未改可见性/历史/保护。真实能力与实验量纲、迁移和所有未验边界见[LIFE_MATERIAL_RULE_COSTS.md](LIFE_MATERIAL_RULE_COSTS.md)。

- 原 `/lab` 六角色表新增显式结构容量/每Tick摄取/合成micro价与规则micro/Gas价。Biomass池实际扣款→身体实际合成热耗→结构账户→原ORCA/攻击边界与原FrameScene/Canvas2D真实半径，不只是多一个计费字段。零结构零截面且不能运动/攻击；初始Forager只有从未收能时按原预留从现存Biomass真实施工供能一次，不能无限续命。
- 自然分裂必须长满材料，亲子从同一材料账户按完整milli拆分，奇数余料留亲本，亲子均可重新摄取长满；死亡原唯一路径→同一个遗骸账→可容纳整数Nutrient raw回收，饱和/亚raw余量留遗骸，不凭空造料或截断消失。
- 原CPU核逐实体实际Gas→整个动作规则集合候选写集/全部身体费用一起提交；欠费/溢出/共享Gas耗尽拒绝该动作集合，不提交半套收费或动作。参数规则与动作规则共用原Tick硬预算；参数实体无生物身体，不虚造内部能量或支付授权。`rule_q`及五个材料Decimal128列进入原Lab/BigInt分析/React两套账。
- 单材料Biomass等价和相对额定容量的二维接触面积是显式实验选择，不冒称规范冻结、元素化学或三维物理ECS已齐。只覆盖显式启用玩家角色，野生供能仍欠；既有建筑/生产成本不是完整宇宙材料总账。
- 快照Arena/13、CURRENT_13、Lab kernel/6；新增必填半径的Arena JSON线上协议统一/2，Rust/TS/远端/C#源码与当前开局transcript同步，旧hello无fallback，WAL仍2，不删旧原始资料/快照/日志或历史。

### 定向验证与未验

实现提交：`b97cd93`（有源结构/逐实体Gas/原Lab聚合）、`9eba537`（真实半径与统一线上协议2）、`d755f59fda2ad5899dc28d980efc11312611d9c9`（原Lab精确材料/规则账）。56个不同定向用例通过（新增6，重跑不累计）：材料生命周期4、动作规则实际Gas与原子拒绝1、原场景半径与越界拒绝1、既有动作9、CPU规则后端3、真实本机授权WS1、Rust开局transcript1、TS同一开局前端1、真实Lab多代/四表Parquet1、Lab TS契约/精确分析25、原React/远端适配9。TS开局另1个未选中的实况录制测试不算通过；不补录原始资料冒充验收。必要server编译（含engine）、client/Lab build或类型、web类型、diff检查通过。敏感信息/私人联系信息/敏感文件名必要筛查未命中，不是完整秘密审计。没有全workspace/Clippy/license/SBOM/soak/CI轮询或外部部署。

真实浏览器WebGPU/硬件GPU/真实PG多机断电长稳未验；阶段41 llvmpipe仍仅软件回读。本批C#源码同步未重编译（当前PATH无dotnet）；Node22正式运行时验收留后续集中阶段，不把Node24警告算已验。全项目发布级完整测试/验收/安全/打磨仍待主体齐后执行。

### 下一真实主体

继续野生有来源供能/一般World组件与规则、TS-rpack/物理ECS、独立Agent赛道与受控实际智能、协作presence/离线多主等真实欠项。一般参数实体仅共享CPU硬预算，不伪称全部World/QuickJS/WASM扩展均已建立物理能量费。Pi仅MIT设计参考，无外部模型/支付授权。不得把本阶段或M0–M8标为全项目完成。

## 第四十二批：自主EMIT信号与连续脑计算实付（从main 4600175继续）

**主体继续，整体未完成。** t26u明确保持PUBLIC并继续普通push，取代PRIVATE-only；未改可见性。先对阶段41四提交逐提交差异作凭据/私人联系信息/敏感文件名筛查，未发现命中，定向夹具为合成数据；不是完整秘密审计。已用认证gh身份cysnies普通push阶段41，远端main与本地 `46001753a5a60628f3ad256b73d8515c67a4ed9e` 一致，未force/no-verify/admin。

本批实际能力与精确边界见[LIFE_SIGNAL_COMPUTE_COSTS.md](LIFE_SIGNAL_COMPUTE_COSTS.md)：

- 原第六动作 `a_emit` → 当前格有界有符号SignalA → 按实际可容纳/可支付raw实付原secretion价 → 热耗/原耗尽退役。只启用显式生态玩家角色，零量/死源/野生/越界无免费动作；不是全部分子或材料模型。
- 原GatedMLP/CTRNN每隐藏神经元每真实子步的必填实验价 `continuous_cost_micro_per_neuron_step`，候选脑/账户总费原子提交；欠费动作清零，LIF/Izhikevich保持真实脉冲价。原 `/lab` 六角色模板显式价格1。
- `continuous_q`贯通原活体/遗骸、私有快照、Lab Energy Decimal128 Parquet、TS BigInt守恒分析与浏览器四表。Arena/12、CURRENT_12、Lab kernel/5；无旧格式fallback，不删原数据历史，WAL仍2。

### 快验与尚未验

本批实现提交：`3674aed`（权威生命链/快照域/原Lab汇总与必要夹具）、`cf3db3cadc0f61fab443a9a8623134a9c38289c4`（原Lab价目/精确分析/浏览器入口）。49个不同定向用例通过（新增5，重跑不累计）：原动作与新增分泌/连续账9、原脑非脉冲原子收费1、原脉冲预算7、真实Lab多代/四表Parquet1、TS契约/谱系25、React原工作台6。必要server编译（含engine）、Lab build、web类型与diff检查通过。初次无arrow特性0用例不计通过；已补开arrow执行真实Lab用例。曾修正定向测试的u32→usize类型及精确耗尽先退役后检查点前置，并真实重跑通过。不做全workspace/Clippy/license/SBOM/soak/CI轮询。真实浏览器WebGPU/独立显卡/真实PG多机断电/长稳未验；llvmpipe仍只代表阶段41软件回读。Node24检查有项目Node22引擎警告，正式Node22运行时验收仍欠。

### 下一主体与发布阻塞

通用生长/材料体量（有来源摄取/合成/分裂/死亡去向）、一般规则计算计价仍欠；自主SignalA不是全部分泌系统，野生供能等继续。一般TS/rpack/物理ECS、独立Agent赛道/真实LLM、协作presence/离线多主等仍保留。主体齐后必须完整测试/验收/安全与正式发布打磨，不称本段或整体已发行。Pi仅MIT设计参考，无外部模型/支付授权。

## 第四十一批：私有受限.rview主题与原renderer真实消费（从main 2082bd3继续）

**主体继续，整体未完成。** 已闭合原签名RYVW私有主题 → 有界扫描/精确library依赖 → SDK发现下载验签 → `/arena`显式应用 → 原FrameScene/WorldRenderer真实八色参数，不是未消费容器或工业美术。入口/格式/限制见[PRIVATE_VISUAL_THEMES.md](PRIVATE_VISUAL_THEMES.md)。

### 实际入口与安全范围

- `/arena`自己的真实控制席位 → 私有受限视觉主题 / RVIEW：原SDK签名JSON显式发布到本人私库、发现精确版本、校验下载/应用、查看许可证署名；应用只改世界背景与七阵营不透明色，须显式点击。原API/鉴权/Store/CAS/签名/精确依赖/owner隔离复用，新表`private_visual_versions`，无外部发行或部署。
- 私有profile `rynvek.private-visual.v1` / `opaque-palette/1`：只许palette.json/theme.json，包≤64KiB、令牌≤8、仅`--rv-world-bg`与七阵营`#RRGGBB`。拒shader/纹理/脚本/应用层令牌/透明色；通用.rview着色器不在本链加载或执行。精确依赖与撤销边界同既有私库。只许CC0/CC-BY，CC-BY须署名。
- 原WorldRenderer/FrameScene消费打包色：单位/建筑用主题阵营色，未知格用主题背景遮挡地形；fog可见性规则不变，隐藏实体不进场景。Canvas2D回退同步同一主题；GPU路径须Worker回执才确认。换世界/断连/撤销恢复内置，不补播。
- 不冒称HRTF无关；亦不伪装最终工业美术、任意危险shader或完整视觉编辑器。Arena/11、CURRENT_11、WAL/2、Lab kernel/4不变。

### 必要验证与未验

8个不同直接用例通过（新增8，重跑不累计）：主题解析/拒绝2、面板显式应用与换世界隔离2、协议1、原FrameScene主题/隐藏实体1（本机llvmpipe软件GPU离线回读未知格背景在同一用例内实际执行）、Rust扫描1、本机认证HTTP→Node SDK签名根/library→发现/下载/验签→实际`privateVisualTheme`1。必要包build、web类型、engine/server编译与diff检查通过。

**llvmpipe离线回读不是真实浏览器WebGPU或独立显卡听验式验收**；真实浏览器/硬件GPU/PG实库/断电多机长稳未验。不把MemoryStore/文件CAS当PG灾备。未跑workspace/Clippy/license/SBOM/soak/CI轮询，无额外运行时依赖/公共发行/外部部署/外采美术或网络浏览器策略绕过。

### 下一主体

优先收拢完整生命成本仍欠项：通用生长/材料体量、EMIT、连续神经/规则计价。一般TS/rpack/物理ECS、独立Agent赛道/真实LLM、协作presence/离线多主等仍保留；Pi仍MIT设计借鉴未引代码。主体齐后集中全面验收，不宣称全产品完成。

## 第四十批：正式PCM采样与原Arena二维空间化（从main 9bf6584继续）

**主体继续，整体未完成。** 已闭合原签名私有RYPK真实PCM采样 → 有界扫描/精确library依赖 → SDK发现下载/校验 → 原Arena手势采样图 → 当前可见实体位置/真实renderer相机声像与距离衰减，不是未消费容器。视觉registry/.rview主题与音频无直接模型依赖，留作下一完整主体，不塞静态视觉空壳。入口/生成示例/数学/格式及限制见[PCM_SPATIAL_AUDIO.md](PCM_SPATIAL_AUDIO.md)。

### 实际入口与安全范围

- `/arena`自己的真实控制席位 → 私有采样/程序化音效制品：原SDK签名JSON显式发布到本人私库、发现精确版本、校验下载/应用、查看真实清单/许可证/署名；应用不创图，仍需上方手势启用。原API/鉴权/Store/CAS/签名/精确依赖/owner隔离复用，无新表/外部发行或部署。
- 新音频profile `rynvek.audio-pack.v2` / `pcm16-recipes/2`：四JSON+最多8个内容地址PCM数据文件，包512KiB；严格规范头PCM16 little-endian单声道、白名单8–48kHz、clip≤2秒，解析图≤8采样/192000帧，原有界依赖深度/节点/资源保留。无压缩/未知块/任意路径/脚本/远端src/平台codec；缺精确样本/撤销/限额违规拒绝。只有自生成CC0短技术样本，无外采音乐音色。
- 原Director真正创建非零PCM AudioBuffer/BufferSource，接原mixer总线/增益/冷却/声部配额，结束显式回收source/gain/panner。纯CPU严格PCM解码与Base64预限量；不调用decodeAudioData或fetch音源。CC-BY素材须有署名，UI展示；签名/白名单不代表版权背书。
- 只定位当前观察中本人且fog=1的实体：原Q32单位位置/实际建筑origin，不用ghost/前帧/消失推死亡位置。相机直接共用原browser view()/viewScale()/有效GPU缩放与实际视口，每cue触发时算二维pan与距离衰减；无有效源/相机不伪造坐标。不冒称HRTF/3D遮挡、持续移动声源或观战/回放音频。
- 原手势/静音/隐藏挂起/3秒过期/终局/换世界身份隔离保留；预载等待不补播，迟到resume/preload有epoch防复活。旧v1私有音频不加运行适配，数据/CAS/Git保留，现行发现只列v2；Arena/11、CURRENT_11、WAL/2、Lab kernel/4不变。

### 必要验证与未验

13个不同直接用例通过（新增5，重跑不累计）：PCM/实际Director图3、协议一致性1、原Arena生命周期及真实源/相机/fog隔离6、Rust扫描2、本机认证HTTP→Node SDK真实签名根/library→私有发现/下载/解析→原Arena WS真实建造→采样图消费1。实际非零1280帧/16000Hz、真实建造origin与受控测试相机pan=.25、距离衰减/原混音增益、结束回收及依赖撤销/owner隔离均断言。生产相机用原renderer共用view；Node测试不冒称操作浏览器。Audio/SDK必要tsc build、web类型、server必要编译及diff检查通过。

录制AudioContext仅节点/参数/生命周期验证，**真实浏览器Web Audio DSP、OfflineAudioContext声学渲染、耳机听验、实际GPU/PG实库/断电多机长稳未验**；不把本机MemoryStore/文件CAS当PG灾备。未跑workspace/Clippy/license/SBOM/soak/CI轮询，无额外依赖/公共发行/外部部署/外采媒体/第三方模型或网络浏览器策略绕过。

### 下一主体

优先视觉registry/签名受限.rview主题→私有发现下载→原renderer实际消费，不执行任意危险shader/脚本，不伪装最终工业美术；随后持续补实质产品欠项。生命成本通用生长/材料体量、EMIT、连续神经/规则计价，一般TS/rpack/物理ECS、独立Agent赛道/真实LLM、协作presence/离线多主等仍保留；Pi仍MIT设计借鉴未引代码。主体齐后集中全面验收，不宣称全产品完成。

## 第三十九批：已有权威动作的真实生命成本（从main 5da6ee1继续）

**主体继续，整体未完成。** 已实现发射、护盾/Weaver治疗、Spitter真实危害场分泌、设施出生/自然分裂合成四类已有动作的实付链，而不是只添加收费项。设计值显式可改，不冒称原规范冻结；通用生长/物质体量、EMIT一般分泌、规则/连续神经计算及野生供能仍未实现。详细价目、原子性、数学与入口见[LIFE_ACTIVITY_COSTS.md](LIFE_ACTIVITY_COSTS.md)。下一优先正式音频采样/空间化与视觉制品，不无限磨生命治理。

主体代码HEAD：`af54aacbfb861c2bb119047605394defe4b46c7b`（核心`d6cc63b`、SDK`390f8d3`、工作台`af54aac`）；其后文档提交不改变可执行行为，最终推送HEAD见同一项目Page/本次交付回执。

### 真实入口与范围

- `/studio`创建私有PvE的完整`ecology.roles`或对已配置角色提交原参数补丁 → 原Actor/Epoch修订 →「在Arena运行当前世界」：原生产/真实战斗/修复/分泌执行。`/lab`自然生命W3表单显式启用角色、编辑五项新参数 → 原持久实验及四表分析。TS/Python严格合约同步；标准无生态RTS/排位和未配置/野生角色不强启生命收费。C#可连接已有世界，生态创建参数/类型化Lab仍欠，本批未扩展或重跑.NET。
- 每发身体费只在有有效活目标、可发射且弹药可付时收，闪避不退真实发射费；欠费不启动新冷却/伤害。护盾按自付、治疗按真实活供体付费，均以实际恢复milli计，满量/死源不收；环境伤害先统一结算，稳定序供体不叠加超原光环速率。
- 分泌实际受原Hazard量程与余额限制，按容纳raw收费，饱和不收、缺钱减量/留残余；不把Hazard价格说成Nutrient能量含量，也不称通用EMIT已实现。设施出生一次实扣birth投入+合成费，合成热耗不变成启动池；自然分裂由亲本付合成并转入子体原投入，HP/护盾拆分不免费复制体量。全部预检后执行，不足不发生/不计账。
- 真实动作热耗子账`attack_q/repair_q/secretion_q/synthesis_q`已包含在原总dissipated内，不双算；活体/遗骸均保留并入ECOL规范根/快照/WAL。后半Tick恰好耗尽也唯一退役，不留下零池幽灵。Lab四个Decimal128精确列与UI/BigInt实际消费，出生冻结合成费有恢复交叉校验。
- Arena/11、CURRENT_11、Lab kernel/4；WAL/2框架不变。旧快照/不完整旧合约拒绝、旧kernel run检查点不混算，无自动兼容层或历史数据删除。参数实体profile仍无原生身体/谱系/生命账，不臆造指标。

### 必要验证与未验

39个不同直接用例通过（新增6，重复运行不累计）：原Engine动作5、真实建设/出生队列合成1、600 Tick原Lab谱系/合成热耗/确定Parquet四表1、本机认证HTTP完整价目创建/补丁/私有恢复及owner隔离1、受影响TS规格/精确列25、原React Lab组件6。包含真实文件快照/WAL撕裂尾恢复与死亡后热账；HTTP是协议/恢复快验，不声称本批又跑了Node SDK→Worker完整链或真人浏览器。

rts受影响tests及server tests必要类型编译、Lab build、web tsc --noEmit、Python包装器语法和diff检查通过。未跑workspace/Clippy/license/SBOM/soak/CI轮询；文件撕裂尾/MemoryStore HTTP不是硬杀/PG多机/断电灾备。真实浏览器/GPU/硬件听验/PG实库/并发SQL/长稳、Python扩展/pyarrow及.NET本批未验；无新依赖/公共发行/外部部署/策略绕过，Pi仍仅既有设计借鉴未引源码。

## 第三十八批：一般参数模型持久Lab/W3与真实四表（从main cd34b4e继续）

**主体继续，整体未完成。** 已闭合真实Studio源捕获 → 不可变有界参数模型/精确单轴×seed扫描 → 原Engine Phase07 → 原持久Lab作业/fenced完整run恢复 → 私有四表 → 原UI/TS SDK真实消费。不是toy runner、只有schema的面板或任意W3/TS/rpack/物理ECS。入口/规格/命令/边界详见[LAB_PARAMETER_EXPERIMENTS.md](LAB_PARAMETER_EXPERIMENTS.md)。

### 实际入口与当前语义

- `/lab` → 本人LabSubmit/LabRead → 实验模型「Studio一般参数实体」→ 输入已在`/studio`真实创建模型的世界ID → 显式导入（额外WorldEdit及所有者/当前定向编辑准入）→ 核对源/outputs → param原始整数扫描、多seed/Tick/间隔 → 原UUID幂等提交/发现/取消/重试/下载。未知响应保留原规格对账；断连清私有草稿/结果及迟到源，不等于取消服务端。
- 新GET `worlds/{id}/authoring/lab-model`只复制参数源码，不开放WorldRead、完整快照/归档或Arena控制；复用Studio准入并在返回前重验。模板初值从Tick 0算，不复制当前动态态；撤权阻止新导出，不承诺追回曾授权源副本。新作业绑定不可变模型、原spec_digest及模型digest，后续编辑源不改既有实验。
- parameters profile明确0阵营/无原生身体，原ArenaWorld.step执行原Phase07/参数暂存写/总gas与规范Tick，不运行RTS经济/战斗/胜负。state/param与I32/Q8_8/Bool完全复用原结构/编译器/权限/资源；256KiB模型、1–16输出、原256run/108000Tick及总200000参数采样行，坏模型/写冲突/越界拒绝。
- 真四表：runs有最终Tick/root/model_digest/真实故障；metrics长行给实际实例/组件/类型/原始整数，非参数模型的RTS指标为null；lineage空（无自然出生事实）；energy.enabled=false不是零耗/守恒证明。UI按runs选run、绘制实际参数轨迹、不画假零能量或伪谱系图；原分析5000行/表上限保留，完整原分页/Parquet可取。
- 复用原owner隔离、租约/fence、取消/重试代次与私有四CID原子completed，无新DB治理框架。完整run检查点恢复，未完成run从种子重算，不是Tick WAL断点续跑。Arena/10、CURRENT_10、WAL/2不变；Lab kernel/3明确结果/执行语义升级，旧kernel检查点拒绝混算，无自动迁移，旧数据/CAS/历史不删。

### 必要快验与未验

**38个不同直接用例通过（新增9，其余受影响规格/列/原入口定向回归，重跑不累计）**：Engine/快照/真实四表3；真正本机认证HTTP→Node Studio/TS SDK→私有源/真实模型扫描→原run检查点→同Store重建→四表下载/owner隔离1；SDK28；React JSDOM6。真实服务的total轨迹2/4与6/12源于rate=1/3，不造出生/能量。最终源取消/显示细化后仅2个参数UI用例再次复核。server必要check、rts受影响tests仅类型编译、Studio/Lab tsc build、web tsc --noEmit、Python包装器语法及diff检查通过；未跑全workspace/Clippy/license/SBOM/soak/CI轮询。

恢复为同MemoryStore/真实私有磁盘CAS、新AppState/HTTP服务，首个真实run检查点手工登记再释放旧租约；不是实际硬杀worker、Memory跨OS进程、PG多机/断电灾备。生产仍需原PG/持久共享CAS/稳定鉴权。真实浏览器/GPU-WASM/PG实库/并发SQL/硬件/断电长稳未验；未重建Python扩展/pyarrow新场景、WASM/Vite，不对其报通过。无公共发行/外部部署/新依赖/外部模型收费或策略绕过；Pi仍设计借鉴、未引源码。

### 下一主体继续

**下一优先完整生命成本：攻击/修复/分泌/生长合成与仍缺计算计价**，显式契约/来源→原池实付→真实行动/死亡/根与恢复，不无限磨Lab。随后正式音频采样/空间化、视觉registry/制品；一般规则世界/TS-rpack/.rview、独立Agent赛道/真实LLM及协作presence/离线多主等仍保留。主体全齐后集中全面验收，不宣称全项目完成。

## 第三十七批：定向私有Studio协作、原日志同步与撤权恢复（从main dc7af6c继续）

**主体继续，整体未完成。** 选择先闭合协作高价值段：真实所有者授权正式多编辑者 → 自己的鉴权/不可变grant版本 → 原Studio/CRDT选择性撤销 → 原Actor/规范修订/原日志同步 → 撤权/断线失效清理及新连接恢复。直接编辑上一批一般参数模型，不是本地多窗口假同步，不联系第三方、不公开世界。一般模型Lab/W3/四表消费尚未接，下一优先补；详细API/真实命令与边界见[STUDIO_COLLABORATION.md](STUDIO_COLLABORATION.md)。

### 真实入口与治理

- `/studio` → 所有者连接Private世界 →「私有Studio协作 / 原日志同步」：显式刷新准入、填正式主体ID、勾选读取全部Studio源码/编辑授权、授予/重授、按当前版本撤销。编辑者用自己的WorldEdit凭据连接同世界；不授予控制/开局/归档/其它私有世界或管理他人准入。清单不是在线成员，不发站外邀请。
- 原authoring返回实际principal/role/grant版本，SDK后续请求绑定X-Rynvek-Studio-Grant。撤销/重授换UUID，旧会话不得静默复活。16有效/64累计准入，墓碑保留；原Memory锁/PG同世界锁事务与新幂等0009迁移，不改原DDL或通用权限。
- 原Store提交点与Actor入队/执行重验授权，防HTTP排队后撤权穿透。普通Epoch待执行补丁拒绝，不终止所有者局；极窄的提交竞争失效时整Tick不发布、保守仅退役本Actor而非写持久终局，所有者重新连接原已提交世界。已经提交/已经合法看到的信息不能追溯收回。
- 显式每秒自动同步或手动同步原CommitRow/CAS/TickRecord的补丁，64提交/页、SDK最多8页/轮，不造SSE/WS或本地新世界。在线CRDT投影按真实revision定序，不信作者未来墙钟；保留原patch ID与逆操作历史、不同步未确认写。过期并发仍409，不自动重发；他人后写选择性跳过，跨模型依赖的undo仍须通过权威整包校验。
- 同步保留本人撤销栈；断线/撤权/错误清会话/身份/私有画面，重新连接当前基线并重置undo，不承诺离线编辑合并。客户端历史512补丁/8MiB上限；服务原list_commits仍全元数据扫描，非大历史索引性能认证。

### 验证与边界

- **16个不同直接用例通过**（真实Rust服务2、SDK协作2+原remote5、原Studio JSDOM7），重跑不重复计数；server必要编译、Studio SDK tsc build、web tsc --noEmit及diff检查通过。最终真实服务用例于2026-11-18 03:26:09中国标准时间结束；不运行全workspace/Clippy/license/SBOM/soak/CI轮询，不把JSDOM称实际浏览器验收。
- 真实所有者/编辑者HTTP身份、独立SDK会话与原WS/Actor；同MemoryStore+私有磁盘CAS重建服务上下文/读原授权与日志，不是PG实库、多机、硬杀或断电灾备。PG迁移/SQL仅Rust编译；真实浏览器/GPU/PG/大历史容量/长稳未验，不绕网络策略。
- Arena/10、CURRENT_10、WAL/2、Lab kernel/2不变；旧格式数据保留但无自动迁移。一般模型仍为有界I32/Q8_8/Bool RuleGraph，不冒称任意TS/rpack或物理ECS；无外部依赖/模型/发行/部署，Pi仍仅MIT设计借鉴。

### 下一主体继续

**下一优先一般参数模型进入真实Lab持久作业/W3变体与四表消费**，复用本批协作批准后的权威模型/原Store治理；随后完整生命成本、一般规则世界/TS-rpack/.rview、音频采样/空间化与视觉制品按依赖推进。协作光标/在线presence、离线多主合并、历史索引容量认证仍欠，不无限磨协作小框架。主体全齐后集中全面验收，全项目持续开发任务不关闭。

## 第三十六批：一般参数实体模板与权威规则绑定（从main 4320917继续）

**主体继续，整体未完成。** 新增非基因、非既有角色的参数实体：创建/编辑模板、实例和规则绑定 → 原Epoch补丁/CRDT → 原Engine Phase07执行 → config_root定义/entity_root动态态 → 原COMP/WAL恢复 → 原Studio与TS SDK消费。范围是独立的有界数值模型，不是RTS物理身体、任意ECS/TS/rpack或Conway/Lenia世界完成。详细契约与可复现命令见[PARAMETER_ENTITY_RULES.md](PARAMETER_ENTITY_RULES.md)。

### 真实入口与模型

- `/studio` → 正式WorldEdit身份连接本人世界 →「RuleGraph动作规则」→「载入一般参数实体规则草稿」；编译并提交模块`counter`，再在「一般参数实体registry」创建模板`model.counter`、实例`counter.one`和绑定`counter.step`。可将这些操作通过SDK合为一个原子补丁；UI分步需先创建引用目标。原「读取权威参数实体状态」实际请求服务，不用本地预览伪装运行。
- 参数模板声明I32/Q8_8/Bool的`state.*`与只读`param.*`；积分器读取`param.rate`并累加`state.total`。修改rate保留真实状态/单调实体序号；撤销定义不回滚仿真时间。删除/重建明确新生命周期，不复制脑、身体、HP或隐含能量。
- 原SetWorldParam的`rules.templates.*`/`rules.instances.*`/`rules.bindings.*`，JSON null是幂等墓碑；原RemoveEntityTemplate的`parameter/{id}`也操作同一CRDT参数寄存器，须Epoch，缺失原生删除仍拒绝。复用原HLC/LWW/选择性undo，不覆盖别人的胜出写。
- 原编译器/沙箱/RIR摘要复验：参数profile不接原六动作/HP/能量/场/邻居；不可混用两profile。最多16模板/128实例/16组件/16绑定，registry值32KiB、规则总源1MiB；新profile共享500000 gas/Tick并预检实例总成本。所有绑定读同一Tick快照，写暂存后整组发布；失败无部分参数写，真实故障入动态根。不同绑定重复写同模板同组件拒绝。
- 引用/类型/资源投影整包预检，失败不涂地或部分改状态。活实例换模板或有状态时变更组件形状/类型拒绝；参数/default编辑不隐式重置活state。权威状态沿原WorldEdit私有基线，不进入对手fog/公共Hub或音频。

### 当前格式与诚实边界

- 唯一当前Arena格式 **rynvek-arena/10、CURRENT_10**；动态ParameterState为必需COMP，定义留config_root，动态值/序号/故障入entity_root。原Arena/9数据与Git历史保留，但新内核明确拒绝自动打开/迁移，旧快照/WAL不能假称新内核可无缝继续。
- Lab内核契约同步升 **rynvek-lab-kernel/2**，避免旧检查点混算；未实现旧checkpoint迁移或旧作业自动续算。一般参数模型导入Lab变体/统计四表尚未接，不能因版本升级称Lab已消费该模型。
- 实际快验10个不同用例通过（world2、真正认证HTTP/Node SDK/WS/Actor/原WAL逐条回放1、CRDT2、原Studio JSDOM5），重跑不重复累计。server必要编译、Studio SDK tsc build、web tsc --noEmit及diff检查通过；真实服务最终于2026-11-18 03:14:01中国标准时间完成。详见上述文档。实际服务为回环HTTP/WS/原Actor及MemoryStore/原私有CAS/WAL，不是PG实库、多机、服务端硬杀或掉电灾备。UI为JSDOM传输夹具，真实浏览器、GPU/WASM通用规则、硬件/长稳未验。默认根改变后的历史golden/全workspace验收未运行，不能声称整套通过。
- 无全workspace/Clippy/license/SBOM/soak/CI轮询，无外部依赖安装、公开发行/部署或网络浏览器绕过。Pi仍为MIT设计借鉴，未引入外部代码/模型。

### 下一主体继续

优先接真实多人Studio协作/版本冲突与重新同步、一般参数模型进入Lab/W3消费，随后继续一般规则世界/TS-rpack/.rview及攻击/修复/分泌/生长合成等完整生命成本。正式音频采样/分轨/空间化、独立Agent赛道/真实LLM仍欠；不无限扩模板小框架，不把本批当全项目完成。主体全齐后集中全面验收。

## 第三十五批：私有程序化音效制品与真实Arena消费（从main 8bfad5a继续）

**主体继续，整体未完成。** 已闭合可用的**程序化音色/包络/混音制品**段，而非只存manifest：SDK构建/签名RYPK → 原Store私有registry/有界扫描/精确依赖/私有CAS → 私有发现/验签下载/解析 → 原Arena真实己方事件 → 下载的配方参数实际进入原Director/mixer/synth。没有生成音乐或第三方音色，无公共发行/外部部署；本profile明确不接`.ogg`/压缩媒体/任意网络音源，正式采样/分轨/空间化仍欠，不能把该完整程序化段泛称全部音频资产完工。

### 真实入口 / SDK

- 原`/arena`连接真实席位 → 新「私有程序化音效制品」：显式上传SDK已签名JSON、发现本人版本、校验下载并应用、下载原签名RYPK、撤销、恢复内置配方。不自动发布示例、生成世界、造对手或拿控制权；观战/回放/练习不挂该私有入口。
- **应用不创AudioContext也不播放**；替换先切断旧图，必须再点击原手势启用。仅之后新己方事件进入原调度，不补播；原fog/真实神经观测/静音/失焦/隐藏/过期/终局/断线/换世界治理保留。世界/身份变化取消下载并拒绝迟到结果，换世界清包。制品没有观察/事件源/网络/工具权限，不泄隐藏状态、不编造spikes或相干度。
- 原`PrivateAgentHubClient.audio()`返回`PrivateAudioClient`，复用原已认证transport，不另收凭据。`buildPrivateAudioPackage/signPrivateAudioPackage`（显式签名回调，不收私钥）、list/get/publish/download/load/revoke。原始下载有界流读取、BLAKE3+Ed25519复验；load校验根/所有锁定制品再消费受信私有服务的解析，不冒称独立CA/独立客户端全解析。

### 容器扫描 / 私有持久治理

- 原RYPK v1/rpack，`engine_compat=rynvek-audio-recipes/1`，≤64KiB；恰四个data/JSON：audio.json、manifest.json、recipes.json、dependencies.lock；所有路径/排序/偏移/填充/整包与条目哈希继续原权威校验，未知/源码/媒体blob/URL入口不接受。新`audio-pack.schema.json`给出profile及recipe/manifestPatch定义。
- 24规范事件/原总线固定；gain0–1、cooldown0–60000ms、voice整数1–16。包独立覆写原合成器音色：波形白名单、频率20–8000Hz、attack1–50ms、decay/release各1–500ms、sustain0–1、Q0.1–5、noise0–0.8；缺省冻结配方不改，客户端/服务端均拒绝未知/原型字段和越界。原总线/事件预算继续限并发，Arena只触发已接的真实事件子集，不谎称24类都有权威来源。
- 精确同owner音频library依赖：4直接、8传递唯一、4层、依赖原制品总量≤256KiB（含根最多320KiB）；拒绝缺失/撤销/CID冲突/非library/跨owner/环。按锁序后序合并、共享节点一次、根最后覆写；配方逐字段、manifest逐事件。
- 新`/api/v1/private-hub/audio`发现，`/{id}/{version}` GET/PUT/DELETE，`/artifact`与`/resolved` GET。原WorldRead/HubPublish+owner隔离，未知/他人404；私有no-store、Vary Authorization，原始下载nosniff。Ed25519新私有audio签名域覆盖owner/ID/精确版本/整制品BLAKE3，与Agent/公共域分离。
- 复用原私有CAS与Store锁/PG事务；共享行形状准确命名PrivateArtifactRow（不改已有Agent序列化），独立0008音频表。事务重验active发布者/公钥/所有锁定依赖，主体锁保护配额/撤销竞态；每owner最多256不可变版本，含撤销墓碑不覆盖，CAS孤立预写可保留不删除。data-export包含本人音频元数据。无新执行框架。
- 撤销禁止后续解析/下载/新依赖发布，面板本地撤销会切图；已下载字节不能收回，不冒称跨标签即时DRM。Memory元数据不跨服务端进程持久化；真正跨进程仍需原PG/持久私有CAS/稳定鉴权，SQL仅Rust编译，未连实库，不是PG多机灾备。

### 必要直接快验 / 未验

**11个不同直接用例通过，重跑不累计**：恶意音频数据/容器1，真正本机认证HTTP→Node SDK构建/签名/私有库依赖→原Arena WS/租约/真实建造命令→权威building_placed→实际ArenaAudioSession消费下载音色1，音色/参数/原型字段2，原Arena音频生命周期5，面板显式应用/迟到隔离2。真实集成用录制型FakeAudioContext替代设备，断言733Hz/222Hz振荡器参数与0.17 trim来自实际下载制品，非伪造事件；重复版本、错签名/CID、匿名/他人、公共blob隔离、撤销依赖阻止解析/新发布、data-export均走真服务。仅必要server编译、audio/SDK tsc build、web tsc --noEmit与diff --check；命令及示例见[私有音效制品说明](PRIVATE_AUDIO_PACKAGES.md)。

**未验**真实浏览器/手势策略、声卡/耳听或DSP离线波形，服务端重启/PG实库多连接多机/断电/长稳；夹具主动有界推进Tick，不称实时长稳或灾备。未全workspace/Clippy/license/SBOM/soak/CI轮询。未引Pi代码/外部LLM/密钥/收费，不绕网络/浏览器，不删原始资料/Git历史/无关数据。中文细提交、最终HEAD/一次PRIVATE普通push证据见同一Page/交接。

### 下一主体继续

转**一般规则/参数模板的真实Studio→权威应用→Lab消费**完整可用段，依据原模型/最终设计限制必要范围，不做装饰预设或只导出JSON。随后一般W3/TS/rpack/.rview/协作与未覆盖的攻击/修复/分泌/生长合成等完整生命成本继续按依赖推进；正式音频采样/分轨/空间化与真实LLM/独立Agent赛道也仍有明确欠项，未实施不冒称完成。主体齐后集中全面验收，不无限磨音频或Adapter，持续任务继续。

## 第三十四批：Pi参考受控Agent Adapter与独立持久执行治理（从main 634258a继续）

**主体持续开发，整体未完成。** 已接原私有Agent包/六工具/原Arena Actor，不是空适配层：真实fog → 白名单provider上下文 → 原无I/O QuickJS包或明确本地测试provider → 候选 → 持久派发意图/预算预留 → 原CommandGate/ToolBroker/Tick/WAL → 真工具结果/事件 → 取消和新控制进程续跑。中文细提交、完整最终HEAD及一次PRIVATE普通push证据见同一Page/交接。无公共发行/部署，无第三方模型/密钥/费用/私有数据外发。

### 实际入口 / SDK / API

- 原`/arena`连接本人现有Private单人PvE，显式取得控制租约 →「私有Agent包」选精确版本 → 新「独立持久运行 / Pi参考受控循环」。核对显示包/CID/声明工具和预算并勾授权后创建；可发现、读真实事件、执行一轮/明确启动预算内循环、暂停/取消、回收过期控制器、unknown对账、本人确认当前宿主/控制fence后续跑、关闭。刷新/断开不删除服务端记录，不自动造身份/开世界/接管席位。
- `hub.runs()`的`AgentRunClient`创建/发现/读取/context/control/step；`ControlledAgentAdapter`驱动顺序循环。默认`PrivatePackageProvider`调原服务端QuickJS；可注入`DecisionProvider`，本批真实测试用明确`ScriptedTestProvider`，它只给候选，工具回执仍由原Actor产生。没有配置真实LLM，不称生产自主智能。可注入函数是可信宿主代码，不把任意JS或远端provider谎称沙箱。
- 新`/api/v1/agent-runs` POST/GET、`/{id}` GET、`/{id}/context` GET、`/{id}/control` POST、`/{id}/step` POST；WorldRead/WorldConnect/MatchPlay+owner/Private单人PvE，未知/他人404。创建必需submission_id、锁包/CID、批准工具子集、预算、本人control_fence及consent=private-pve-tools/1。无第七工具。原个人data-export包含同一运行记录，原会话单步入口保留其明确边界，不做旧格式fallback。

### Pi实际借鉴 vs 代码复用

- 再次实际打开官方[earendil-works/pi](https://github.com/earendil-works/pi)的[README](https://github.com/earendil-works/pi/blob/main/packages/agent/README.md)、[manifest](https://github.com/earendil-works/pi/blob/main/packages/agent/package.json)、[LICENSE](https://github.com/earendil-works/pi/blob/main/LICENSE)：main manifest `@earendil-works/pi-agent-core` 1.5.2，MIT，Copyright (c) 2025 Mario Zechner，依赖pi-ai/pi-schema 1.5.2。不是声称已安装或核实npm最新。
- 本批**借鉴设计、原创实现，不直接依赖/复制/安装/执行Pi代码**。适配当前Rust事务/私有包与六工具没有必要为此引整套pi-ai/UI/coding-agent/bash/文件工具。现有许可不改；以后若直接用MIT代码须审查固定版本/传递依赖并保留版权和完整MIT通知。没有声称Pi SDK或API兼容。
- 实际实现了上下文分层/候选与执行分离/顺序循环/真结果回灌/可取消续跑/有序事件。provider白名单只有授权fog/批准schema、包/世界、预算/计数、memory/有界真实结果投影、最近16个事件seq/kind，**不传owner/控制fence/controller/操作表/凭据**。完整工具结果留私有事件；provider超过8KiB时明确truncated原tool/tick/error摘要，不造result。回调是观察性而非权限/持久化屏障，屏障在Store/WAL；不冒称完整Pi steering/follow-up/token stream已实现。

### 持久治理 / 取消与恢复

- 原Store Memory锁/PG事务及Principal active检查，新独立`0007_agent_runs.sql`；没有复制Lab worker或泛化新框架。每owner最多64记录，每owner/world一个未关闭运行；同owner/submission同规格同ID，异规格409。30秒控制器租约/fence、revision CAS、有界96操作回执不驱逐；只有Store首次接受的候选才真正派发，并发/迟到重放也不再执行。
- 轮次/工具次数各1–32，墙钟10–900秒；派发前预留，Broker实际拒绝也消耗尝试，不因取消/续跑重置。SDK真实Tick间隔等待，不伪造Tick；最后仍由原Broker控制等级/租约/fence/schema/30Tick/600APM裁决，不以模型输出直接写仿真或发外部消息。
- `tool_dispatch_intent`先事务落盘，再经原Actor CommandGate和Tick队列执行；原WAL提交后才给真回执。所有结果只由服务端原Actor写入，不接受客户端“已执行”声明。取消/暂停撤后续控制，已派发可能完成且保留真结果；不能撤回已执行工具。provider取消/超时不再派发候选，但不承诺杀死任意恶意宿主JS或撤销已发网络请求。
- 进程/响应丢失后不确定调用保留pending；过期recover→unknown，禁止盲重发，真实状态/WAL/事件对账后显式ack_unknown只是放弃自动判定、不是重试/撤回。mutation有原命令键可查WAL，查询/拒绝不保证有world命令记录；世界提交与结果事务的崩溃缝隙保守unknown，不造exactly-once保证。
- 新控制进程从Store发现记忆/真结果/预算。本人重连原世界取得当前控制租约后，resume显式确认当前host_fence/control_fence，保留原工具/包锁/预算并记新绑定；包撤销阻止新求值/续跑，不妨碍取消/关闭/查历史。不是恢复半条token流、独立注册Agent/排位或无人值守服务端provider。

### 必要直接验证 / 未验

8个不同直接用例通过，重跑不累计：状态机3、真HTTP/WS/两次独立Node控制进程/原Actor1、SDK中断不合作测试provider不派发1、原私有包面板JSDOM3。真实回环验证默认QuickJS包provider实际消费上次真结果、无I/O、查询和战略HoldPosition实际作用己方单位/ack；明确本地测试provider也走真工具，取消后第一个Node退出，第二个Node发现原记录/记忆/计数、取新控制fence显式续跑；重复候选不重派。必要server编译、agent-sdk tsc build、web tsc --noEmit及diff --check；详命令/消费示例见[原私有包说明](PRIVATE_AGENT_PACKAGES.md)。

**不是**真实LLM、真实浏览器、服务端进程重启/硬杀或PG灾备测试。服务端元数据在本机同一MemoryStore，真正跨服务端OS进程需原PG、私有持久CAS和稳定鉴权；PG迁移/查询仅Rust编译，未接实库。PG多连接多机/故障切换/断电/长稳、GPU/音频听验未验；无全workspace/Clippy/license/SBOM/soak/CI轮询，无WASM/Vite重编，不绕网络/浏览器策略，不删原始资料/历史/无关缓存。

### 下一主体继续

切到**音频制品真实容器/校验/私有发现下载与实际消费**，在原程序化音频基础闭合一个可用资产段，不生成歌/不拿静态manifest充完成。其后一般Studio模板/规则/TS/rpack/.rview/协作、一般W3与未覆盖生命成本按依赖继续；真实LLM/独立Agent注册赛道仅在明确新授权与实际依赖具备时推进，别无限磨Adapter。主体齐后集中全面验收，整体任务继续。

## 第三十三批：Lab持久实验作业治理（从main 53b7e0e继续）

**主体持续开发，整体未完成。** 本批用原Store事务与私有CAS替换Lab进程内事实源：真实规格 → 幂等持久提交/发现 → 有界worker租约/fencing → 原RTS逐Tick实验 → run检查点 → 四张真实Parquet一次发布 → 原`/lab`和TS SDK消费。没有新影子仿真器，不公开Hub/NuGet、不外部部署。实际代码提交/最终HEAD与PRIVATE一次普通push证据见同一Page和交接。

### 实际入口与协议

- 原`/lab` → 使用本人已有`lab.submit/lab.read`能力令牌 →「提交真实实验」。新增「发现我的持久作业」分页找回任务、载入/刷新结构化状态、取消/显式重试；仍可看真实四表/谱系/能量账和下载原Parquet。令牌只在内存、不自动造身份；断开清空本地结果**不取消服务端**。
- `POST /api/v1/lab/experiments` **必需**`submission_id: UUID`，同owner/键+同规范化规格返回同作业，异规格409。规格仍是原name/seeds/ticks/sample_every/variants。SDK `submit(spec, submissionId, signal?)`不盲造第二个键；工作台不确定响应保留原键与原规格，再次提交原样对账；显式放弃本地键不会撤销服务端，先发现对账以免重复计算。
- `GET /api/v1/lab/experiments?offset=0&limit=25`，limit 1–100、offset≤512；`GET .../{id}`；`POST .../{id}/control`请求`{operation_id:UUID,generation:u32,action:"cancel"|"retry"}`。同操作键绑相同动作/代次，重放返回**当前**状态；不同请求同键409，新操作旧代次409，旧cancel回执重放不得取消新retry代次。retry仅failed/cancelled；completed不重做。同owner活动最多8、总量512、操作回执64且不驱逐、每代认领最多5，达到后failed，需所有者显式重试，检查点保留。
- 状态`queued/running/completed/failed/cancelled`，新增generation/attempt/lease_expires_at_unix_s/submission_id/spec_digest/kernel_profile；owner_principal_id改为规范十进制**字符串**。SDK终态判定直接读状态，不再解析404错误文字。私有请求no-store、omit credentials、error redirects；工作台不自动取消远程作业，旧身份迟到响应不挂回页面。
- 原`.../{id}/rows?table=…`、`.../{id}/parquet?table=…`保持真实四表消费，但结果改存**原私有world-checkpoints CAS**，不是公共Hub Blob；非所有者与未知作业均404。data-export读取同一Store作业，不漏重建后的历史。

### 执行/恢复和原子性

- 新增Lab专用`0006_lab_jobs.sql`及原Store Memory/PG分支。PG owner行锁串行提交配额/重试，唯一(owner,submission_id)；认领`FOR UPDATE OF job SKIP LOCKED`，数据库时钟，30s租约、2s续约，每节点1–16个有界worker（原lab_max_concurrent配置）。没有泛化作业框架或外部队列。内存后端仅同一Store对象保留元数据；真正跨进程需原Postgres Store、持久私有CAS和稳定签名鉴权，多个节点必须可见同一私有CAS。
- 每次认领、取消/重试、回收均前进fence；心跳/检查点/失败/释放/最终提交都核对owner active、node、fence、running及未过期租约。迟到/取消/过期worker不可发布或误改新代状态。优雅shutdown等待原计算在Tick边界协作停止后释放回queued；崩溃需租约到期重新认领，CPU执行允许至少一次，不声称exactly-once。
- 原`run_one`语义保留，新interruptible入口只加Tick边界/首尾检查，不加新状态/随机数。每个**完整run**先写私有CAS检查点，再fenced事务登记CID；检查点绑定显式`rynvek-lab-kernel/1`执行契约、不可变spec_digest、run编号/种子。直接typed JSON序列化/反序列化保留u128能量，**不经serde_json::Value**。恢复复用已完成run，未完成run从原种子重算；不是中途Tick/进程内存快照。执行语义变化须审查并升级kernel契约/明确迁移，不能拿Git文档HEAD当内核版本，也不隐式跨契约重算旧队列。
- 原Arrow/Parquet生成runs/metrics/lineage/energy，四个私有CAS写入全部成功后，在一次Store锁/PG事务更新结果四CID和completed。取消/过期期间可有**未引用私有CAS孤立写入**，不自动清理；读者从未看见半套结果，不能用公共CID路由取回私有表。当前结果汇总仍在单作业内存中，沿用原规格上限，非流式无限量导出。

### 必要快验与未验边界

**18个不同直接用例通过，重跑不累计**：状态/fencing/代次/typed检查点4、真实本机认证HTTP→Node TS SDK治理→真实run检查点→同MemoryStore/文件CAS重建→四表消费1、SDK单文件9、原Lab React工作台4；server必要编译与TS类型通过；没有整workspace/Clippy/license/SBOM/soak/CI轮询。该重建集成**手动登记真实run检查点再释放租约**，不称硬杀进程或完整worker崩溃恢复测试。PG迁移/查询路径仅Rust编译，未接实库，多连接/多机/数据库故障切换/断电/长稳未验。React是JSDOM/传输夹具，不是实际浏览器；硬件/GPU/音频听验仍未验。没有删除原始资料、Git历史或无关缓存。

实际执行命令：
```sh
cargo check -p rynvek-server
cargo test -p rynvek-server --lib lab_jobs -- --nocapture
pnpm --dir packages/lab exec tsc -p tsconfig.build.json
cargo test -p rynvek-server --test lab_jobs -- --nocapture
pnpm --dir packages/lab exec vitest run test/client.test.ts
pnpm --dir apps/web-platform exec vitest run test/lab_workspace.test.tsx --config vite.config.mjs
pnpm --dir apps/web-platform exec tsc --noEmit
git diff --check
```
SDK真实回环测试通过stdin接收本机一次性测试身份，stdout只回非凭据收据；不把测试令牌输出或写进脚本。

### 下一主体继续

Lab这段已闭合后转**Pi参考的受控Agent Adapter/独立Agent执行治理**，严守原六工具ToolBroker权限/预算、持久执行/中断状态边界，外部LLM不进权威Tick；Pi仍仅已核对的设计参考，**未引入/复制/执行其代码**。随后按依赖补音频制品真实容器/校验/私有消费、一般Studio模板/规则与生命成本、未覆盖的一般W3/协作等主体，不能宣称全项目完工或无限只磨Arena/Lab。所有主体齐后再集中全面验收。

## 第三十二批：真实Arena事件音频与仅己方脑态声化（从main 069ae49继续）

**主体持续开发，整体未完成。** 真实投影`9f19531`、声化内核`3c61c61`、实际Arena消费`74ec3a5`、直接验证`366dadf`；最终文档HEAD与一次普通push证据见同一Page/交接。不生成歌曲或媒体资产，不公开Hub/NuGet发行、无外部部署；Pi仍仅后续受控Adapter参考，**未引入/执行Pi代码**。

### 实际消费入口与可用段

- `/arena` → 连接正式本人已有PvE/排位/私有自创席位（或显式本地练习）→「音频 / LIVE FACTION AUDIO」→ **手势启用音频**。默认为关；真实脑态声化另行显式勾选。主音量及全部静音；音频不触发开局/命令、不占APM、不暂停世界。Canvas2D/WebGPU共用原实时客户端流，不另造演示或仿真。
- `ArenaAudioSession`直接订阅当前`ArenaClient`的权威阵营observation/events，使用原AudioDirector、五总线AudioMixer、程序化synth/recipes与小节调度网格。真实本人建筑放置/完成/摧毁、单位出生/死亡、科研与繁育事件进入原配方；同类合并、冷却/发声数限制、同Tick普通重复不重播，**观察性提示并非无损审计或逐spike重放**。单位死亡复用既有爆炸提示音色，不伪造权威爆炸；无武器发射事件就不编造开火声。
- 仅从前后同一己方主脑可见HP下降触发告警音，**不声称识别炮火或伤害来源**。不从敌方事件、ghost消失、全世界单位/场/战斗密度推断声音；不接全世界`extract_arena_audio_telemetry`。服务端原event_visible_to不放宽，客户端再拒绝非己方事件。
- 新必需`FactionObservation.neural`由原`ArenaWorld.observe(viewer)`只读计算**本阵营现有单位脑**：LIF/Izhikevich真实64位spike_ring的神经元数/置位总数；GatedMLP/CTRNN真实hidden_q8/y_q8绝对均值与神经元数。MLP/CTRNN不伪造spike；sample_tick绑定当前观测。它是各脑各自最近64个**已执行神经子步的重叠历史窗口**，出生/预算冻结会使窗口与墙钟不齐，**非每Tick新增脉冲、瞬时频率或Kuramoto相干度**。不导出敌方脑、个体膜电位/权重/基因组。无仿真状态/版本/随机数新增，投影不改权威根。
- 两条低电平载波分别映射真实环窗口占用率与连续态幅度；复用nextBarBoundary调音高、短平滑调电平，不把历史置位计数变成一串新spike。新字段同步TS/C#类型及现行跨语言夹具，**没有旧格式fallback**；原开局夹具仅插入真实会话生成的新字段，其余语义全量逐字段比对不变。空单位Worker夹具的聚合确为零。

### 用户手势与权限/生命周期

- AudioContext仅在启用按钮同步调用栈创建/请求resume，await前不做网络；没有自动解锁或策略绕过。音频不持凭据、不拉外部URL、不执行包；浏览器拒绝则静音报状态，真实仿真继续。
- 静音、神经声化关闭、页面隐藏/失焦、上下文挂起、超过3秒无新权威Tick、WS断开、服务端拒绝、终局（含**同Tick真实终局**）、换世界/身份及pagehide均切断相应旧图。先同步取消增益/断开输出，再关闭上下文；静音/挂起时继续更新观察基线但不发声，断开/换身份/世界则清基线并解绑旧流；迟到resume不能复活旧图。返回/重连需再次手势，新音图不补播静音期间历史；已到设备缓冲或已听到的音频不能撤回，物理停止延迟未测。
- 当前只对**实时控制席位授权的本人阵营**挂载；观战与签名回放不启用声音，避免seek历史补播/准入变化另开旁路。声音只是展示层，不改变原WorldRead/WorldConnect/席位/fence/ToolBroker与WAL。
- 本段用内置程序化配方/载波，**不直接依赖外部音色制品、.ogg或Hub包**，因此不为此扩展公共Hub/容器。真实音频包制品/校验/私有发现下载、正式采样/分轨音乐、空间化及设备体验仍是后续项，不把存在的AudioManifest或空音乐分轨称完整音频资产链。

### 必要快验与明确未验

- 必要编译：audio/client当前源码tsc build、web-platform `tsc --noEmit`、更新后C# .NET10 build（0警告/0错误）与`git diff --check`通过。无全workspace/Clippy/license/SBOM/soak/CI轮询，未WASM/Vite重编。
- **9个不同直接用例通过**（重跑不累计）：Rust己方真实脑核/环计数与敌方扰动隔离/不改根1；原Arena权威2431Tick开局转录逐字段1；更新必需投影的Worker命令夹具往返1；真正C#进程→本机HTTP/WS/原Actor完整消费1（证实当前新字段没有断原链）；前端纯映射/图状态/取消恢复4与实际Arena挂载JSDOM 1。
- 真实原会话Tick331测得己方32个spiking神经元、64子步环中24次置位、连续态绝对均值64，直接夹具验证映射，绝不从输出幅度伪造spikes。前端验证使用**录制型FakeAudioContext与JSDOM传输**：证明路由/节点/调度/生命周期，不是离线DSP波形渲染、实际受限浏览器自动播放策略或硬件听验。C#是真实本机服务，但不冒充浏览器音频端到端。
- 真实浏览器、物理声卡/扬声器/耳机/音质延迟与削波、GPU、PG实库/多机、断电/24h、Windows/macOS未验；网络/浏览器策略不规避。同Store/本机测试不称PG多机灾备。

### 下一主体继续（不磨音色、不收尾）

**下一从Arena切到Lab持久实验作业治理**：真实作业提交/排队/取消、进度/产物、重启对账与恢复和已有工作台消费；不以进程内重建冒充PG多机容灾。独立Agent/模型控制面与持久执行治理随后按依赖推进，Pi仍为权威Tick外、六工具原权限/收据/fence内的候选Adapter，不因参考建议强行替换。一般Studio参数/非基因模板/TS/rpack/.rview/协作、一般W3与未覆盖生命成本保留；全主体齐后集中全面验收，整体任务不关闭。

## 第三十一批：C# SDK真实身份、私有世界与原工具链（从main 7830fbc继续）

**主体持续开发，整体未完成。** API `2dcbfaf`、精确协议与schema `f850344`、真实消费 `e71c019`、跨语言端到端 `3dec289`；最终文档HEAD与一次普通push证据见同一项目Page/交接。无公共NuGet/Hub发行、外部部署、假对手或新工具；Arena `/9`、`CURRENT_9`、WAL `rynvek-tick/2`不变。

### 可用入口与完整场景

- `sdks/csharp/Rynvek.Client`（.NET10、纯BCL、IsPackable=false），用法及全部直接验证命令见`sdks/csharp/README.md`。本箱按Microsoft官方脚本正常安装SDK **10.0.404**至`/workspace/tooling/dotnet`，未改用户机器、未绕网络策略。
- `RegisterAsync/LoginAsync`通过调用者外部Ed25519签名回调真实注册/挑战取会话；私钥不进SDK/包/日志，不从文件/环境隐式取钥。Bearer只在内存、HTTP头及WS Hello，不进URL，不公开token getter。TLS正常校验、禁止重定向/Cookie/自动重试，非loopback强制HTTPS。
- `ListWorldsAsync/GetWorldAsync/CreatePrivatePveAsync/ConnectPrivatePveAsync` → 原WorldRun/正式JSON WS → 本人fog → 原控制租约/fencing → 命令移动、六工具查询与战略写入 → 原Actor/30Tick/ToolBroker/WAL/真实回执 → 交还与断开清理 → 同身份重登/同世界同match原位置。不使用practice或本地仿真，不自动造局/造人。
- 补真实缺失的`GET /api/v1/worlds/{world_id}`，复用原WorldRead/load_world鉴权；他人私有/不存在均404，成功private,no-store/Vary Authorization，不返回私有快照/源码/CID。不是SDK声称调用不存在的接口。
- Principal严格规范u64十进制字符串；Tick/seq/fence/seed/revision原生ulong、实体uint、资源/HP/Q32.32原值long；未知细分结果/事件保留JsonElement，2^96整数可转BigInteger，无double往返。嵌入唯一原`agent-tools.v1.json`，六构造器/严格字段枚举范围校验，无第七工具或advance旁路；命令当前明确IssueOrder/Surrender，其它不提供伪接口。
- 单reader/串行sender/32在途/1MiB消息，发送前重验fence，匹配seq/回执类型/工具名。超时或取消后的结果可未知：清fog/lease、结束连接与在途任务，不自动重试/不宣称撤回已发命令。128项丢旧notice仅观察通知，不是持久/无损日志；权威取具体receipt和服务端WAL。Dispose解绑子连接并释放生命周期。

### 实际必要验证与未验边界

- `.NET10`实际build **0警告/0错误**；Smoke自检**3组**通过：精确u64/i64/Q32.32/2^96与鉴权字节、六schema构造/拒绝、HTTP凭据/重定向/销毁传输夹具。
- `RYNVEK_DOTNET=/workspace/tooling/dotnet/dotnet cargo test -p rynvek-server --test csharp_sdk --offline -- --include-ignored --nocapture`：**2项通过**。第一项核对Rust权威鉴权域/六工具夹具；第二项真正启动编译后的C#进程，走本机真实HTTP/WS/Actor。最终生命周期改动后单独再次执行后者通过，不把重复运行计作新用例。
- 真实场景验证两正式测试身份注册/挑战（Rust只持临时测试签名器，C#自己取会话）、`9007199254740993` seed不失真、本人fog/租约、真实单位位移、查询及间隔30Tick战略HoldPosition落入真实order、交还、断开清理、同主体重新登录恢复原match/位置、他人私有404与真实终局。测试由原Actor显式advance驱动；**不是生产实时钟/长稳验收**。
- 未全workspace/Clippy/license/SBOM/soak/CI轮询，未WASM/Vite重编。未真实浏览器/GPU/音频设备/PG/多机/断电/24h、Windows/macOS；没公开NuGet。当前C#不是完整排位/多人/观战、二进制Link/WebTransport、私有包构建验签或全部Studio/Lab SDK；未把同Store测试冒称PG灾备。

### t24u：Pi Agent Core官方核对与适配选择（参考，尚未引入）

- 已从官方公开仓库/manifest/许可证/源码核对当前项目 [earendil-works/pi](https://github.com/earendil-works/pi)，包[`@earendil-works/pi-agent-core`](https://github.com/earendil-works/pi/blob/main/packages/agent/package.json)；本次核对main manifest为**1.5.2**（不是声称已安装/核实npm最新发布）。常见历史名称为`badlogic/pi-mono`/`@mariozechner/pi-agent-core`，勿混淆版本。仓库[LICENSE](https://github.com/earendil-works/pi/blob/main/LICENSE)及包声明均**MIT**，版权Mario Zechner；若后续复制必须保留许可/版权并审查固定版本及依赖。本阶段无复制、安装、执行Pi代码或添加依赖。
- 实际机制来源：[Agent README](https://github.com/earendil-works/pi/blob/main/packages/agent/README.md)、[agent.ts](https://github.com/earendil-works/pi/blob/main/packages/agent/src/agent.ts)、[agent-loop.ts](https://github.com/earendil-works/pi/blob/main/packages/agent/src/agent-loop.ts)。AgentMessage→transformContext→convertToLlm隔离应用上下文与provider输入；Agent先归约state再await事件监听，waitForIdle等运行清理，低层agentLoop EventStream只是观察，不把异步消费者当执行屏障。工具参数校验/beforeToolCall阻断/abort复查/execute/afterToolCall及start/update/end事件真实存在。
- 默认工具**并行**：前置检查顺序、完成事件按完成顺序，toolResult写回按assistant源顺序；可选sequential。steer在当前assistant轮后处理，followUp在循环本应停止时处理；abort走AbortController，continue可基于user/toolResult转录续跑，但不等于持久exactly-once恢复。已核对Core的state/messages/队列在内存，**不据此声称整个Pi仓库没有独立持久设施**。
- 适合下一Agent演进：在**权威Tick之外的可信控制面**借鉴循环/状态/事件/取消/steering，模型只看授权阵营fog投影，provider密钥不进包；六工具一律包原ToolBroker/租约/fence/APM，变更选**顺序执行+30Tick约束**，不用默认并行写。调用状态区分confirmed/rejected/unknown；不确定回执先与原WAL/状态对账才继续，abort不撤回已发工具。不得引入coding-agent文件/bash工具或任意npm插件，不把LLM非确定性塞进仿真或突破QuickJS沙箱。**这里只记录后续Adapter/依赖选择，未实施Pi复用，不推倒已成C#链。**

### 下一主体继续（不是收尾）

**下一优先音频真实事件消费、用户手势解锁/静音/断开生命周期与神经声化。** 不以placeholder或无声测试冒称硬件已验；保持现有迷雾/权限边界。随后在上述Pi参考边界内推进独立Agent/模型控制面与持久执行治理；一般规则/模板/TS/rpack/.rview/协作、Lab一般W3/持久作业、未覆盖生命成本仍欠。全主体齐后集中全面验收，整体不关闭、不无限只磨Arena。

## 第三十批：私有Agent包、受限运行与SDK真实消费链（从main 9ba04b8继续）

**主体持续开发，整体未完成。** 主要实现锚点 `9abdc3f87733cc3d0a2e3fa645328a19d7ae1fd0`（SDK `bacdcc0`，私有后端 `ec6b1a3`，产品入口 `9abdc3f`）；后续集成前提说明及本进度记录也是普通细提交，最终完整HEAD见同一项目Page/交付回执。未公共Hub发行、外部部署、创建假对手或独立虚构Agent主体。Arena `/9`、`CURRENT_9` 与唯一WAL `rynvek-tick/2` 不变。

### 完整可用段与真实入口

- `/arena` → 正式身份连接**本人已有Private单人PvE** →「私有Agent包 / 真实工具委托」：本人版本发现、SDK签名JSON扫描私有发布、验摘要/签名下载、撤销、显式单步真实工具、停止与断开清理。不是静态页面、空容器或浏览器eval；包始终在服务端无I/O QuickJS运行。签名私钥不进工作台/包；SDK外部sign回调产出上传JSON，Bearer只在内存/认证头。
- 复用RYPK `.rpack` 严格三文件profile（agent.json、dependencies.lock、main.js），engine_compat=rynvek-agent-tools/1；严格profile/未知字段/工具schema/容器与文件hash、凭据标记校验。私有签名域绑定owner、ID、版本及整个artifact BLAKE3，与公开Hub不同。程序只能请求声明的原六工具，无module/OS/网络宿主入口。
- owner-private registry独立于公共索引；制品存原私有world_snapshots CAS、不写public blobs。公开Agent发布或把agent.json容器伪装rpack均拒绝，历史数据不删除。版本与撤销墓碑不可覆盖、每主体最多256；Memory共用Store锁，PG新增0005_private_agents.sql并用Principal行锁/事务重验配额、公钥与整个精确依赖闭包。
- 仅同owner签名精确CID library依赖：每节点8项、图16库/深度4/组合源码256KiB；冻结词法deps实际调用纯函数，环/摘要冲突拒绝。撤销任一依赖阻断后续求值。100000 gas/16MiB堆/256KiB栈、每节点两并发且取消不提早释放阻塞名额；64个决策步扫描固定Trust 3，**不是1000世界Tick/Trust 2资格或长稳**。标记扫描不证明能识别所有编码秘密。
- 真实链：原Actor本人固定阵营fog → 有界memory/last_result → 沙箱候选 → SDK核对owner/世界/CID/阵营/模式/租约fence → 本人现有ArenaClient.callTool → 原ToolBroker限速/控制层/命令Tick/WAL/真实回执。evaluate永不执行工具或开局/夺控制。HTTP输入不接全世界/凭据/源码；求值后重验授权/撤销/世界fence。stop/换身份/换包/fence后丢迟到候选；已发工具不可撤回，未知回执停止且不自动重试。
- 此段是所有者明确委托**自己的当前PvE控制**，非独立注册Agent、Agent排位赛道、LLM服务、后台自动循环或持久运行记忆；不允许Human排位代打。刷新后runner记忆清空。当前已有六工具都可由声明profile请求，不只是查询；直接测试已验证战略工具真实写入己方单位HoldPosition。

### 必要快验与边界

- Rust定向3项通过：TS容器/签名域跨语言及无网络/死循环/越权/结构拒绝；owner/签名/未知字段/公共误发隔离；**实际Node22 SDK**私有发布→发现→BLAKE3/Ed25519下载→原Actor fog/精确依赖求值→ArenaClient真实WS/租约→查询→间隔30Tick→战略写入/己方真实order→依赖撤销。跨语言用例需先构建SDK/client并显式`--include-ignored`；本阶段确实执行通过，不把默认ignored算通过。
- SDK 5项、工作台JSDOM 2项，共**10个不同直接用例**，不累计重跑。SDK/client构建、SDK及web-platform源码类型与git diff --check通过；服务端必要编译与定向测试通过。仅格式化触及Rust文件，无全workspace/Clippy/license/SBOM/soak/CI轮询、无WASM/Vite重编。
- 真回环HTTP/WS/Node/原Actor不等于实际浏览器/多机。PG迁移与事务仅Rust编译，未接实库；同MemoryStore不跨OS进程持久化，更不是PG灾备。真实受限浏览器/物理GPU/音频设备/PG多连接多机/断电/24h未验，既有网络策略不绕过。私有registry持久化仍依赖既有持久Store、私有持久CAS与稳定鉴权；不自动清理预写孤立CAS、不宣称持久自主Agent恢复。

技术协议、可运行SDK组合示例及准确验证命令见 [PRIVATE_AGENT_PACKAGES.md](PRIVATE_AGENT_PACKAGES.md)。

### 下一主体继续

**优先C# SDK真实鉴权/HTTP/WS/租约与ToolBroker消费，再接现有音频模块到原事件、用户手势/静音/断开生命周期与神经声化。** 目前没有C#主体源码，音频已有模块但未接本真实入口，不把placeholder synth当真实硬件验收。Hub独立Agent/模型服务、多用户私有分享与后台执行治理仍欠；一般规则绑定/非基因模板/任意TS/rpack/.rview/协作、Lab一般W3/持久作业及完整生命成本继续保留。避免无限仅磨Arena，主体齐后集中全面验收，整体任务不关闭。

## 第二十九批：持久排位队列、唯一席位与宿主发现恢复（从main 9b7c5ce继续）

**主体持续开发，整体未完成。** 本批代码锚点 `889a902d08280f5edf629a8e33e8485765861e8f`（后端 `1c20fcc`，浏览器 `889a902`）；最终进度记录提交及普通push后的确切HEAD见同一项目Page/交付回执。Arena `/9`、`CURRENT_9` 和唯一WAL `rynvek-tick/2` 不变；未造对手、联系真人或对外部署。

### 真实入口与原子配对

- `/arena` → “真实人类排位 / HUMAN RANKED” → 正式身份验证/找回席位、显式入队、刷新/退出排队、连接已匹配席位/重连。`GET /api/v1/matches/queue` 读取持久队列/本人完整席位并对账过期预留，不入队、不配对、不创建宿主。`POST /matches/queue` 才推进真实注册主体的匹配；`poll:true` 不会给已离队/终局者自动重入。
- MemoryStore 和 PostgresStore 均以Store为唯一队列事实源，不再由进程内Matchmaker/Registry决定是否仍在队列。既有Matchmaker仅作为事务内纯匹配算法，保留入队时刻、规则集/赛道隔离与评分窗口。PG新增独立幂等迁移 `0004_ranked_discovery.sql`，不重写原DDL或旧材料。
- 显式入队先持久保存；需要配对时才在事务外准备私有CAS首快照。随后同一Memory互斥/PG事务内**消费双方队列、插入原WorldRow/首Commit、Created MatchRow及完整两席**。Created对局本身即持久配对预留，不留“出队成功、只建一席或未建局”的中间事实。CAS准备/发布失败不丢队列；未被发布的预写CAS块不公开、不自动删除。并发重复请求找回同一个match/world，不重复配对。
- PG固定协调行串行化队列/配对、普通insert_match与自创完整席位冻结；Memory共用原Store锁。已有活动局或排位队列的主体不能同时冻结另一模式的控制席位；私有邀请的确认不冒充已冻结席位。外部直接写数据库不在应用事务保证范围内，不声称新增了可跨表子查询的数据库唯一索引。
- 未入座的完整配对预留有从创建时计算的固定120秒期限，宿主重建不能重置预留时间。后续发现/入队对账会对尚未运行且无任何运行修订的超时预留执行持久Aborted与世界fence轮换，释放双方而不自动重新排队；真实已提交终局即使tick仍为0也不能误删。账户注销同步撤销队列及未终局排位席位，保留历史。

### 宿主、租约与真实恢复

- 新增 `POST /api/v1/matches/{match_id}/connect`：仅已认证本人参赛席位+MatchPlay/赛道授权可显式连接或恢复，WS仍必须WorldConnect/原主体/固定阵营/原租约。队列/席位成功回执为 `private,no-store` 与 `Vary: Authorization`。发现回执有 `host.state: local | remote | recoverable`、无凭据origin及到期秒数，不返回私有CID/源码/全知观察。
- 每个AppState有真实唯一node ID，Store保存30秒宿主租约、origin与世界fence。健康远端宿主不被其它节点抢占；大厅/Manual驱动也每5秒续租，输入及每Tick再次校验。PG认领/续租/提交按同一世界行锁串行，PG到期判定使用数据库时钟。认领必须匹配原修订与fence，轮换fence后才挂原Actor；普通HTTP/WS与状态查询不信任旧Registry句柄。
- 到期/被接管/持久化不确定的排位Actor只退役自己、断连接、收控制租约及待写输入，**不发全局Aborted/Completed或结算事件**。旧fence/过期租约的Tick提交、元数据与终局写均拒绝。正常停服显式drain自身宿主并轮换fence；非正常退出则等租约到期后由本人显式恢复。
- 从原私有CAS/已提交WAL恢复同一个match/world、原配置、原根/实际Tick及固定两席，不换模板、不伪造在线对手、不把控制租约当宿主租约。恢复双方仍要真实WS入座；旧控制fence不能继续提交命令。完整签名回放从原genesis覆盖接管前后全部WAL，不因快照不保存调试command_log而截断旧命令。
- **终局检查点已落盘、完成事件尚未送达**时，重连会从真实终局及原WAL补齐归档/结算，不重新运行或长期卡住席位。排位终局行、两方最终分数与Glicko评分在同一Store事务结算；相同终局重放幂等，不重复计分。私有自创PvP仍非排位，不误走排位宿主租约或计分。
- 浏览器连接前调用真实connect API并校验本人/对局/阵营/模式/宿主。健康远端origin只作为发现结果显示：用户核对可信服务地址后手动切换、重新验证身份；**不自动重定向、跨origin发送Bearer或把凭据放URL/日志/存储**。断开后的迟到恢复响应不创建WS或恢复身份。

### 必要快验与明确未验

- `cargo check -p rynvek-server --offline` 通过；最终服务端定向5项通过：1个可控时钟的到期/续租/写入/幽灵预留单测，2个新增持久排位集成，原排位真实入口1项，原私有双人PvP全链1项。集成使用实际回环HTTP/双WS/Actor、并发同主体请求、双服务健康远端发现、受控drain/fence接管、原根/三帧WAL恢复、真实投降与完整签名归档、一次性评分、半建局回滚、终局提交后丢完成事件对账。旧宿主写入拒绝日志是定向覆盖，不冒充无错误运行。
- web-platform源码 `tsc --noEmit` 通过；排位JSDOM定向6项通过，新增远端发现不转发凭据及断开后迟到宿主回执隔离。共11个不同直接用例，不累计重跑；`git diff --check` 通过，只格式化触及的Rust文件。未跑全workspace/Clippy/license/SBOM/soak/CI轮询，未重编WASM/Vite。
- **同MemoryStore + 真实共享磁盘CAS的新服务/AppState恢复，只证明上下文重建，不是Memory跨OS进程持久化，也不是PG多机灾备。** PG关系表/协调行锁/租约/终局事务代码已编译，但本批未接真实PG执行新SQL或并发多连接事务。真实部署需持久Store、所有候选节点可访问同一私有持久CAS、稳定签名/鉴权配置与正确可信origin；否则不能宣称恢复能力。30秒故障接管为租约语义，不是瞬时无损热备。
- 实际浏览器、物理GPU、真实PG/掉电/跨主机时钟与网络分区、24h长稳、后台扫尾时效和大队列/高频事务容量未验；过期预留当前随认证入口对账，不是独立后台控制平面。没有自动跨origin代理/入口负载均衡故障路由，不冒充完整多地域服务发现；不规避受限浏览器/网络策略。

**下一主体优先切换Hub/SDK/音频，而非无限细磨Arena。** 择现有Hub registry/Agent模型实际消费与SDK端到端接入的完整段推进，再安排C# SDK与音频/神经声化；一般规则绑定/参数/非基因模板、任意TS/rpack/.rview/协作流、一般W3/持久作业治理、攻击/修复/分泌/生长合成等完整生命成本继续保留。主体全齐后集中全面验收，当前不关闭全项目任务。

## 第二十八批：私有自创PvP双人控制席位完整生命周期（从main 04c2bb6继续）

**主体持续开发，整体未完成；Arena仍唯一`/9`、`CURRENT_9`，WAL仍唯一`rynvek-tick/2`。** 交付明确的两人私有自创段，不伪造对手/在线状态，不联系第三方、不公开部署；不把自创PvP计入排位。

### 可操作入口与真实权威链

- `/arena` → “私有自创双人PvP / 非排位”。已有Private `arena_duel`世界直接输入ID；也可显式输入标题/地图seed后“创建私有双人世界”（请求固定Private，不自动创建玩家）。使用上方服务origin和自己的正式凭据“验证身份/读取邀请与席位”。所有者是阵营0，输入另一正式Active人类主体ID后“定向邀请”；受邀者以自己的身份显式“确认接受我的控制席位”。清单/确认事实来自Store，不冒称在线成员，不发送站外邀请消息。
- 受邀者确认后，所有者显式“冻结完整席位/连接重连”。房间CAS与完整MatchRow/两份participants原子提交，然后原世界恢复/CAS认领fence、原MatchHost。另一个玩家读取同一房间，按真实回执进入同一match/world，不能凭URL、房间UUID或版本冒领席位。参数不允许改请求阵营，WS Hello仍由Actor逐席校验。
- 两条真实WS都在大厅入座后原Actor开局；大厅只允许拿/交原租约，**正式Running前UI与服务都禁止世界写命令**。玩家在大厅断开不继续算“已就绪”；普通运行中断线不暂停、不释放被冻结席位，按原租约过期/重连语义保留；原世界/已提交Tick/WAL恢复而不是模板替换。250ms只读房间状态对账在准入改变/关闭/错误时禁写并清空，不自动重试写入。
- DAG：`Private arena_duel → 正式所有者/受邀人类确认 → 房间版本CAS/冻结两席与MatchRow → 原Runtime/CAS世界fence/Actor → 原WS主体与席位/租约fencing → 原命令Tick WAL → 本阵营fog → 原Arena reducer/Canvas2D/WebGPU Renderer`。没有第二仿真器、本地演示状态、占位ACK或任意TS绑定。

### 所有者、玩家、观察者与私有边界

- 所有者仍拥有WorldEdit创作/完整私有归档权；受邀玩家仅得到**本人固定阵营控制席位**，另需有效WorldConnect+MatchPlay、人类赛道与Active身份，不获得WorldRead/WorldEdit、规则/基因源码、commits、完整归档或私有CID读取权。`mode: PvP_Ranked`只是既有仿真协议枚举；回执明确`custom:true`、持久MatchRow为`PvP_Custom`，UI标“私有自创/非排位”，正常终局不更新Glicko。
- 观察者继续使用第二十七批的独立准入，只见所有者当前所控阵营fog，不拿控制席位/租约；玩家不能用观战管理读取别人的准入，观察者不能借新房间绕过原控制授权。Binary/JSON WS共用同一房间/主体/席位检查，不能改hello阵营看对手；已知世界ID/版本都不是凭据。凭据仅内存/认证头/正式握手，不放URL、日志或本地存储。
- API新增 `GET/POST /api/v1/worlds/{id}/multiplayer`。POST只有严格的`invite{guest_id}`、`accept{version}`、`connect{version}`、`close{version}`四种当前结构。全部成功回执`private,no-store / Vary: Authorization`；非所有者/非受邀人404。接受/冻结/关闭用版本CAS，旧邀请不能覆盖新状态；真实双方身份与完整席位恢复均再校验。

### 主动离开/撤销、fence与持久恢复

- “撤销或离开并关闭自创局”对所有者/本局玩家均明确表示：**关闭这份被冻结的两席授权，中止尚未结束的整局**，不在进行中的仿真里热换人、不伪造胜负或免费重置。关闭后需显式fork/新建世界；原世界、日志、谱系、匹配历史和原始材料保留。已Completed的结果/胜方不会被改成Aborted。
- Memory/Postgres原Store新增当前`CustomRoom`，独立幂等应用迁移`0003_custom_rooms.sql`，不改原规范DDL、不加旧读路径。房间关闭与世界fence轮换、非终结MatchRow→Aborted同一互斥/数据库事务提交；在本节点等Actor确认清空排队输入、断开双方连接、解绑闸门、回收原租约后才确认。显式撤销即便已断线，也按权威阵营回收原保留租约；普通断线行为不变。
- 受房间治理的Actor每输入/只读查询与每Tick重新校验持久房间、两个Active主体及当前世界fence。旧节点失去fence不能提交新Tick；关闭的旧WS/lease/resume不能复活授权。错误或数据库不确定按失效处理，不把未提交输入算成功。
- 同Store/私有磁盘CAS的新AppState由任一已冻结参赛者“连接重连”恢复**同一match、两席、原配置/根/位置与Tick**。不自动恢复closed/终局房间，不生成新玩家/新世界；终局完整签名归档仍只给所有者，玩家无全量源码旁路。

### 必要快验与未验

- `cargo check -p rynvek-server --offline`、web-platform源码`tsc --noEmit`和`git diff --check`通过。Rust定向3项：engine显式撤销断线租约1；真实TCP HTTP+双WS原Actor的邀请/本人确认/旧版本冲突/越阵营与私有拒绝/fog/真实移动/WAL同根节点恢复/排队后主动离开回收1；所有者接受前撤销、正常投降Completed、私有归档隔离、不计排位、关闭后保留胜方1。
- web-platform定向14项：已有世界/观战与新自创入口10（含新增所有者完整UI、受邀玩家确认/大厅等待/撤销清屏、显式Private创建与迟到隔离3项）；既有人类排位4项。合计**17个不同直接用例**，不重复累计重跑；前端JSDOM传输夹具不是实际浏览器/硬件实测，真实服务链由上述Rust TCP/WS用例验证。
- 未全workspace/Clippy/license/SBOM/soak/CI轮询，未WASM重编或Vite打包。实际受限浏览器/GPU、真实PG迁移/事务、断电/24h与多客户端长稳未验，网络策略未规避。PG后端Rust路径编译通过不等于SQL实库验证；节点恢复用同MemoryStore和真实私有磁盘CAS，不冒称OS进程断电验收。

实现细提交：`6f16061`（断线租约显式撤销）、`030218a`（房间/席位/Actor/持久fence）、`4729930`（真实双人产品入口）。最终文档HEAD、普通push时间及PRIVATE证据记同一Page与交接，不将实现SHA冒充最终HEAD。

### 下一有界主体阶段

1. **排位队列与活动宿主持久发现/恢复**：目前排位队列/节点内发现仍进程内；自创房间持久化不等于统一跨节点发现。继续补真实身份的持久队列/取消与匹配对账、重启后真实席位发现、宿主恢复/路由及跨节点同主体活动局排他，不能重复配对/创建假对手。
2. 当前自创段是固定两席、确认后冻结、显式关闭整局，不支持活动局热换玩家、多人房间目录/服务端邀请通知或全知观战。跨节点协调/路由与容量、真实PG/长期性能仍留后续；不称整个多人平台完成。
3. 一般规则绑定/参数/非基因模板、任意TS/rpack/.rview/协作流、一般W3/持久作业治理、攻击修复分泌生长合成等完整生命成本、Hub registry/agent/audio、C# SDK、音频/神经声化继续保留。主体齐后集中全面验收，整体任务继续、不关闭。

## 第二十七批：私有自创世界定向观战准入与完整只读生命周期（从main 56d4ff2继续）

**主体持续开发，整体未完成；Arena仍唯一`/9`、`CURRENT_9`，WAL仍唯一`rynvek-tick/2`。** 依据现有单人自创运行链，选择能完整交付的“定向阵营观战”阶段，不把未完成的多人控制席位包装成大厅。无假对手/假在线成员，不联系第三方、不公开部署。

### 当前可用入口、身份与可见性

- `/arena`新增“私有世界·定向阵营观战”。所有者先用真实凭据“运行/重连此世界”，输入目标**正式主体ID**，点击“授予当前所控阵营观战”；可显式刷新准入清单、对每个主体按当前版本撤销。列表来自Store，不是在线人数。目标须为真实Active身份，不能是所有者或现有控制席位，单世界最多32个有效准入。
- 观察者在上方输入同一服务origin、世界ID及自己的WorldConnect凭据，点“只读观战此世界/重新确认”。认证取得实际主体/种类/准入版本/阵营后，把**原Actor产生的welcome与原`world.observe(faction)`投影**送入已有Arena reducer、Canvas2D/WebGPU Renderer；无浏览器仿真、无另建世界、无假welcome地图。HTTP每次完成后间隔250ms读取；没有事件历史/全知视角，不宣称WS推送或硬件验收。
- 控制者仍须原WorldConnect授权、原真实席位、原WS握手和Sovereign租约/fencing才可发命令。观察者**没有参赛席位、WS连接或控制租约**，不计入准备/入座、不启动Actor、不改变世界根；传输层拒绝除本地接收初始化外的全部写命令，UI不开放生产/移动/租约/投降。原Actor读消息沿同一已提交边界执行，不复制第二仿真器。
- 准入只共享**所有者当前所控阵营**的原fog可见投影（含该阵营本来可看的资源/单位）；禁止任意阵营参数和控制席位冒充观察者。每次读取前后重新验证Active主体、WorldConnect范围、私有世界与同一grant_id，原Actor排队期间撤销/重授则丢弃旧结果。宿主恢复后再次核对所有者所控阵营，不把旧准入带到新对手阵营。没有源码、基因字节、完整快照/归档或私有CID读取权；既有WorldRead/WorldEdit/Hub私有CAS封闭保持不变。

### 真实API、持久化、撤销与恢复

- 新增 `GET/POST /api/v1/worlds/{id}/observers`（仅Private世界真实所有者+WorldEdit：清单/定向授予）、`DELETE .../observers/{principal_id}`（所有者按grant_id条件撤销）、`GET/DELETE .../observation`（本人实时投影/按grant_id离开）。回执均`private,no-store / Vary: Authorization`；凭据只在内存与Authorization，不放URL/本地存储/日志。错误不返回其它主体/世界存在性，未授权私有访问404。
- 原Memory/Postgres Store共享`ObserverGrant`模型；新增独立幂等应用迁移`0002_observer_grants.sql`，不改写原规范DDL、不加旧格式读路径。PG世界行锁/事务与内存互斥保证32有效名额约束，grant_id为版本而非可转交的访问令牌。撤销/离开保留inactive对象，重授换新版本，审计保留授予/撤销/离开事实；旧版本不能撤销新授权，不删除原始资料/Git历史或无关数据。
- “离开并撤销我的观战准入”先停本地在途拉取，再按当前版本等待服务端回执；成功后需要所有者重授才能再加入。主动取消的旧拉取不会抢先把离开当成失败。所有者撤销后后续服务器读取立即拒绝，UI下一读取拒绝即停止并清空私有画面；网络超时/版本改变也清空且不自动重试写入。已合法送出的历史信息不能从人的记忆中撤回，不虚称回收已经看到的数据。
- “断开身份”或离页立即停止请求、隔离迟到结果、销毁内存凭据与画面，**不冒称已经撤销准入**。准入不以节点内连接表为真相；节点上下文重建从同Store恢复。原Actor不存在时返回409，不让观察者私自开局/认领宿主；所有者沿原入口恢复同一match/world后，观察者重新认证继续。

### 仅必要快验与交接

- `cargo check -p rynvek-server --offline`及web-platform源码`tsc --noEmit`通过，`git diff --check`通过。
- Rust真实TCP HTTP/WS/Actor定向1项通过：私有权限/非所有者拒绝、越阵营授予拒绝、query不能切阵营、投影与原Runtime fog逐值相同、观战不入座/不取租约/不启动、伪造控制WS拒绝、authoring/commits/准入清单/完整replay/已知私有快照CID均拒绝；版本化撤销/重授/离开；新AppState+同MemoryStore/私有磁盘CAS恢复准入与原match/tick。**不是OS进程断电或真实PG持久性验收**。
- web-platform定向11项通过：已有世界/归档挂载与新观战生命周期7项（含4项新增观战/所有者入口），既有人类排位挂载4项。撤销清屏、无WS/practice、禁止控制、离开与在途取消竞态、迟到结果隔离、按实际阵营授予与版本撤销均覆盖。合计**12个不同直接用例**，不重复累计重跑；JSDOM只模拟传输，产品不造状态。
- 没有重编WASM/Vite打包、全workspace门禁、Clippy/license/SBOM/soak/CI轮询。真实浏览器/GPU/PG/断电/24h未验，浏览器网络限制未规避。新增PG SQL已源码编译覆盖，未接真实PG执行；250ms全投影轮询的多观察者容量/长期性能尚未验。

实现细提交：`b25c52b`（持久准入/原Actor fog/API）、`3288f29`（真实管理/只读观战产品入口）。最终文档HEAD和一次普通push记录同一Page/交接，不把实现SHA冒充最终HEAD。

### 下一阶段仍继续主体实现

1. **自创PvP多人控制席位全生命周期仍未实现**：下一补所有者指定真实主体、受邀确认/加入、WorldConnect席位授权、完整参赛者冻结/开局、撤销/离开及时回收连接/租约fence、原Store/Actor恢复及产品入口。当前新准入仅观察者，不能拿来绕过控制授权或称多人大厅完成。
2. 按依赖继续排位队列/活动宿主持久发现、跨节点找回/恢复；现排位发现仍进程内。观战虽有持久准入，不等于全产品分布式发现完成。
3. 一般规则绑定/参数/非基因模板、任意TS/rpack/.rview/协作流、一般W3/持久作业治理、完整生命成本、Hub registry/agent/audio、C# SDK、音频/神经声化及主体齐后的集中全面验收均保留。整体任务不关闭。

## 第二十六批：Studio规则删除与首次创建的权威撤销重做（从main e11c97e继续）

**主体持续开发，整体未完成。** 唯一Arena快照仍为`/9`、`CURRENT_9`；当前Tick WAL明确升级为`rynvek-tick/2`。不增加依赖、不造本地世界/假回执、不扩成一般清障或全门禁。

### 可操作入口与完整权威链

- `/studio` → 连接真实WorldEdit世界（或显式创建Private当前格式世界）→ “RuleGraph动作规则”页签 → 输入模块ID/RuleGraph → “编译并提交规则”。首次创建后，“权威撤销”发送`RemoveRuleModule`，再“权威重做”发送原源与原RIR摘要的`UpdateRuleModule`。点击已提交规则加载后可“删除选中规则”，删除后同样支持权威撤销/重做。不存在的选择禁删，忙时禁写；**只有原服务器成功回执后才更新已提交清单/版本/根**。
- 复用原`RemoteStudioSession → AuthoringDocument/HLC/UndoManager → POST .../patches → 原Actor Epoch调度 → WorldPatch预检/执行 → AuthoringState规范根 → 原私有CAS/WAL → 既有Renderer/运行链`。删除与更新同为EpochBoundarySafe，整包按顺序预检实际规则ID集合；不存在/非法模块、重复删除、坏摘要或低分级使整包拒绝，不留下部分地形/规则修改。
- 直接删除真实`world.authoring.rules`成员；仅删除故障模块时清除该故障引用，不能掩盖其它模块故障。删除停止该模块后续消费，**不回写已执行动作、实体、资源或历史**；原脑在自己的Epoch继续计算。普通配置冻结、WorldEdit私有权限和修订冲突不放宽。活动Actor同patch_id仍返回既有幂等200，新patch_id过期revision仍409；非活动旧提交路径保持原409行为。
- CRDT更新/删除写同一规则LWW寄存器，显式墓碑不回落到基线；物化清单过滤墓碑。一次补丁对同模块多次写只产生一份逆操作；首建撤销是真删除，删除撤销恢复精确源/摘要，重做取逆补丁之逆。别人后来的胜出写被选择性跳过，不覆盖他人。HTTP失败/409/网络不确定仍使会话失效，须刷新，不自动重试或显示本地假成功。
- 成功回执新增必填`rules_removed`计数，因此TickRecord生产、恢复与重演只认`rynvek-tick/2`，拒绝旧`/1`，不加默认字段/双版本兼容。Arena快照结构未改；原`/1`日志/归档/原始资料全部保留、不自动转换、不保证含旧WAL的历史世界能按新协议继续运行。需要旧数据迁移时另行明确设计，不能默默伪造迁移成功。

### 必要快验与本阶段边界

- Rust定向6项通过：`rynvek-world --test studio_authority` 3项；server活动Actor规则创建→删除→重建+检查点/WAL逐根恢复1项；私有HTTP真实编译/删除权限/低分级/过期409/重复缺失422/权威恢复1项；普通配置冻结与新版WAL重演/旧版本拒绝1项。
- TypeScript定向23项通过：studio协议9、remote传输/规则首建删除逆操作5、既有选择性撤销5、web Studio挂载4。UI夹具仅模拟传输，产品不生成假状态；真实服务依赖另由上述Rust HTTP/Actor用例覆盖。总计**29个不同直接用例**，不重复累计重跑。
- 必要`cargo check -p rynvek-server --offline`、studio源码`tsc --noEmit`及build类型产物、web-platform源码类型通过，`git diff --check`通过。未重编WASM、未Vite打包、未全workspace/Clippy/license/SBOM/soak/CI轮询；真实受限浏览器/GPU/PG/断电/24h仍未验，无浏览器网络策略绕行。
- 快验中先纠正两处测试假设：删除不应立刻清空上一Tick动作；活动Actor重复同patch_id是原幂等200而非409。按真实原协议改用下一脑Epoch验证停止覆盖，并区分新ID过期409，没有改弱产品授权/冲突语义。

实现细提交：`40f51d0`（权威删除/新版WAL）、`f8e7143`（墓碑与逆操作）、`bf7a2af`（UI入口）。本文件随下一文档提交；最终完整HEAD、普通push时间及PRIVATE证据记录同一Page和交接报告，不将实现SHA冒充最终HEAD。

### 下一有界主体阶段（不是收尾）

1. 自创多人席位/实时观战准入：先接真实世界所有者授权、席位分配/身份绑定、加入/撤销/离开、重连及控制fence生命周期，再接已有观测/Renderer；不公开发布、不伪造在线成员、不联系第三方。按服务依赖选择能完整交付的一段。
2. 排位队列/活动宿主持久发现与恢复，不能继续把本节点进程内列表说成跨节点或重启可发现。
3. 一般规则读取/写入绑定、非基因实体模板及参数、协作流、rpack/.rview/任意TS仍欠；**本批只完成现有六动作绑定的规则增改删/逆操作**，没有借机开放HP/能量/场绕过真实权限与生命总账。
4. 持久作业治理/一般W3、攻击修复分泌生长合成等完整生命成本、Hub registry/agent/audio、C# SDK与音频/神经声化继续保留。主体齐后集中全面验收，不将阶段交付视为整个项目完成或关闭持续任务。

## 第二十五批：真实人类排位队列与席位控制入口（从main d880719继续）

**主体持续开发，整体未完成；唯一Arena/9、CURRENT_9不变。** 按已有服务依赖选择完整人类排位段，而不是伪造观战或多人大厅；所有实际对局验证均在本机双注册身份夹具，不联系真实第三方、不向外部服务入队/发布。

### 可操作入口与权威依赖

- `/arena` → 上方输入真实服务 origin / 正式凭据 → “验证排位身份 / 找回席位” → 显式“进入人类排位队列”。新增认证 `GET /api/v1/matches/queue` 只读取当前主体ID、当前规则集与本节点活动席位，不入队、不创建身份/对手；要求MatchPlay及Human赛道准入（Agent拒绝）。它是既有规范20路由之外的只读扩展，不改规范编号。请求及回执 `private,no-store / Vary: Authorization`。
- 入队、持续刷新、取消沿用原 `POST matches/queue`、评分窗口、配对器、MatchHost与Store。`poll=true` 只刷新已有成员，终局/离队后不得自动重入；无真实对手就等待，不填充机器人。配对→异步建局→宿主发布期间用异步闸门串行化队列操作，防止旧主体刷新重复建局或取消错误报告离队。若配对已提交，取消返回真实席位，不能撤销对局。错误/超时不自动重试写入、不算取消成功，须刷新/重新验证对账。
- “连接已匹配席位 / 重连”进入前重新读取服务端身份、world/match/faction/mode，使用同一内存凭据认证原JSON WS；校验welcome阵营/模式后取得原Sovereign租约，才开放原生产/移动/建造/投降和Canvas2D/WebGPU Renderer。双方连接由原宿主自动从Lobby运行，不造准备态。投降明确标为排位真实结果；终局继续原签名归档下载/只读重演入口。
- 新增“交还控制租约 / 重新申请控制”，走原release/acquire回执及当前fencing。原协议允许同身份断线后租约保留至过期，不冒称重连自动撤销；浏览器不保存旧租约，而是重新认证确认。真正交还后旧fencing拒绝。失去租约/连接立刻禁写，身份断开与离页终止请求/追踪、销毁私有身份并隔离迟到结果。**断开不保证离队、不暂停、不撤销已匹配对局**，界面明确要求先取消等回执；同身份可找回本节点席位。
- 排位仍沿用既有公共对局元数据/归档语义，页面明确提示，不把自创私有世界送入排位；本批没有向外部服务创建公开对局。队列与活动宿主发现仍在本节点进程内，不承诺重启/跨节点自动找回；没有实现实时无席位观战或自创多人邀请/准入。

### 就近修复与直接快验

- `client.ts` welcomeWaiter同步异常结清抽成最小私有方法，消除TypeScript跨同步回调错误窄化。`@rynvek/client` **源码**类型检查通过，连续两次同步失败均被真实拒绝而不挂起；不再绕源码报错或抑制类型。
- 排位实际生产的就近修复：每兵种独立基因组下拉/缺省值，不能让Striker沿用第一行Forager种子；高频观察不抢走正在编辑/展开的表单焦点，提交仍用最新权威视图。空ack不算成功。没有改账本或生产校验。
- 必要编译/类型：`cargo check -p rynvek-server`、client源码类型与声明构建、arena必要声明构建、web-platform源码/测试类型通过。Rust本机HTTP/双WS/真实Actor定向1项通过（1.44秒）；client生命周期3项、web排位挂载4项、已有世界挂载3项、结构化面板11项通过，共22个不同用例，不累计重复运行。覆盖认证拒绝、只读身份发现、显式队列/取消、并发匹配唯一宿主、双方真实席位、越席命令拒绝、重连、交还后旧fence拒绝、投降、终局poll不重入，以及内存身份/迟到隔离/租约拒绝。
- 测试编写曾错误假设同身份重连立即撤销旧fence，并在手动Actor未推进时等待ack超时；查明原协议保留租约且命令在Tick消费后回执，按真实语义修正夹具，**未修改服务授权/放宽断言来迁就伪成功**。不是产品行为先红后绿或实际浏览器验收。
- 未跑整workspace/Clippy/license/SBOM/soak/CI查询，未重编WASM或跑Vite。真实浏览器、硬件GPU、PG、断电/24h未验，不规避网络策略。没有发消息、邀请外部玩家或公开发布。

代码提交：`803665b`（最小握手修复）、`4c3093c`（认证匹配/取消防线与真实双席位快验）、`d3d50e1`（人类排位入口/控制生命周期）、`e9223c7`（生产基因组/编辑焦点）；最终HEAD、一次普通push与PRIVATE证据记同一Page及阶段报告。

下一有界段优先 **Studio规则删除及首建规则撤销/重做**，复用当前规则补丁/CRDT/编译/Actor，不伪删除元数据；其后继续多人自创席位/观战准入和排位持久发现。一般参数/实体模板、任意TS/rpack/.rview、协作流、一般W3/持久作业治理、完整生命成本、Hub registry/agent/audio、C# SDK、音频/神经声化及全面验收继续保留，非全项目完成。

## 第二十四批：Arena结构化生产/科技/信标操作面板（从main fd4b7ec继续）

**主体持续开发，整体未完成；当前仍只有Arena/9、CURRENT_9。** 本批不另建仿真器或本地演示账本，不恢复已拆除的JSON命令主入口，不加旧格式兼容层。复用原ArenaRuntime/Actor、`ArenaController.send`、阵营观测、Canvas2D/WebGPU Renderer 与现有 `--rv-*` 设计令牌。

### 真实可操作入口

- `/arena` 侧栏主操作区由权威观测驱动的结构化面板取代原先 JSON 文本入口。可从地图点选或建筑名册选中己方建筑：孵化槽排队/取消队尾（全额退款）、突触精炼厂研发 Tier 1、演化坩埚研发 Tier 2/3 与定向繁育、信标配置 `field.signal_a` / `field.signal_b`、拆除非主脑建筑（返还 50% 造价）。建造菜单补上精炼厂与坩埚。
- 造价、科技前置与队列上限只作提示，**不预扣资源、不乐观改队列**；只有 `ArenaController.send` 的真实回执记成功。回放、断开、终局或非己方建筑时写入控件禁用。信标表单将观测中的 Q8.8 整型换算为命令用毫单位预填（`milli = q8 * 1000 / 256`，向零截断）。
- 观测补上己方 `research_remaining_ticks` 与公开 `BreedingView`（仅兵种与剩余 tick）。`BreedJob.child` 基因组不进入观测。基因组下拉为观测 slug 加该兵种缺省种子；无法仅凭 slug 过滤原型，错配由服务器拒绝。

### 提交与快验边界

本批代码提交：`94a3796`（观测投影）、`22c2dcd`（协议对齐）、`3015ff7`（控制器公开方法）、`89e2ab3`（结构化面板）、`791790e`（定向快验）。最终 HEAD、一次普通 push 与 PRIVATE/干净证据记同一 Page。

必要快验：`cargo check -p rynvek-rts` 通过；`@rynvek/arena` 与 `@rynvek/web-platform` 的 `tsc --noEmit` 通过；web-platform 定向快验 13 项通过（面板视图/命令形状 10 项，已有世界挂载名册选中并 `cancel_queue`、回放只读 3 项）。不是实际受限浏览器、GPU 或仿真验收。未重编浏览器 WASM，未跑整 workspace / Clippy / license / SBOM / soak 或轮询 CI，未跑 Vite 打包。`@rynvek/client` 源码 `tsc` 当时有既有 `client.ts` welcomeWaiter 报错，第二十五批已就近修复并通过源码类型。真实 GPU/PG/断电/24h 仍未验。不绕既有浏览器/端口网络限制。

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
- CRDT/选择性撤销复用原AuthoringDocument/HLC/UndoManager。基因与`genome/`目录删除合并到同一LWW寄存器，可删除→恢复→重做，并跳过别人后来的写入；普通世界revision由真实提交回执校准，不拿本地补丁数代替。UI仅展示回执后的地形/版本/根，409/网络不确定使本地会话失效，刷新基线再操作，不自动重试或覆盖他人。该批当时首建规则无删除逆操作；第二十六批已完成首建/更新/删除的权威逆操作，以上述最新阶段为准。
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
  "unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":100,"repair_cost_micro_per_milli":1,"secretion_cost_micro_per_field_raw":1,"birth_synthesis_milli":10,"activity_reserve_milli":10,"replication_mutation_rate_permille":50},
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
 "roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":100,"repair_cost_micro_per_milli":1,"secretion_cost_micro_per_field_raw":1,"birth_synthesis_milli":10,"activity_reserve_milli":10,"replication_mutation_rate_permille":50}}}}]}
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
| 已推进，仍有边界 | 第二十/三十九批已统一六角色配置与真实供给/运动/维护及发射/修复/分泌/出生合成实付 | 通用生长/材料体量、EMIT、野生与非脉冲/规则计算仍欠；不冒充完整生态或物质总守恒 |
| 已推进，仍有边界 | 第二十一批已挂载Lab真实工作台并消费W3自然突变规则；第三十三批补持久作业治理，一般W3规则仍欠 | 真四表、自然谱系图、BigInt能量曲线、seed/网格/消融、私有进度/下载已接通；规模上限/控制器干预明确 |
| 已推进，仍有边界 | 第二十二批已挂载Studio参数/共享基因/动作规则/目录删除与权威撤销；一般TS/rpack/模板/协作流仍欠 | 当前只支持显式绑定范围，不伪称所有参数/编辑器完成 |
| 已推进，仍有边界 | 第二十三批已有Studio世界运行/重连/节点上下文恢复、完整运行WAL私有签名重演 | 真实单人PvE/原Actor；前置编辑过程/多人席位/客户端独立验签/持久抽帧索引仍欠 |
| 已推进，仍有边界 | 第二十四批已用结构化生产/科技/信标面板替换 JSON 命令主入口 | 真实观测与 ack，不预扣资源；排位/观战/多人席位、Studio 规则删除与一般绑定仍欠 |
| 已推进，仍有边界 | 第二十五批真实人类排位队列/取消/双席位连接重连/控制交还重取 | 当前本节点Human赛道；无假对手，不称跨节点持久发现、无席位观战或自创多人准入已完成 |
| 已推进，仍有边界 | 第二十六批规则真实删除、首建撤销/重做及显式删除恢复 | 原CRDT/Actor/规范根/WAL及私有权限；一般规则绑定/协作流仍欠 |
| 已推进，仍有边界 | 第二十七批自创私有世界定向阵营观战：持久准入、撤销/离开、原Actor fog与产品入口 | 无控制席位或全知视角，原私有CID/源码/完整归档保持封闭；多观察者性能/真实PG未验 |
| 已推进，仍有边界 | 第二十八批私有自创双人控制席位邀请/本人确认/冻结/原Actor开局重连与恢复/整局回收 | 固定两席，不热换人、不公开发布、不计排位；原fog与私有源码/CID隔离，PG实库/长稳未验 |
| 已推进，仍有边界 | 第二十九批持久排位队列/完整配对预留、唯一席位、宿主租约与原CAS/WAL恢复、终局一次性对账 | 同Store双服务/真实WS通过；健康远端只发现并手动切换，不自动转发Bearer；PG实库多机/掉电未验 |
| 已推进，仍有边界 | 第三十批owner-private签名Agent三文件包/精确依赖/受限QuickJS/原ToolBroker与SDK真实端到端 | Node真实私有发布下载与战略写入通过；非独立持久Agent/公共Hub发行，PG未实库 |
| 已推进，仍有边界 | 第三十一批C#真实注册/鉴权、私有HTTP/WS/fog/租约/原命令与六工具消费、同局重连 | .NET10编译、自检3组与Rust2直接用例通过；真实C#进程/原Actor，非全域SDK或多机灾备 |
| 已推进，仍有边界 | 第三十二批原Arena真实阵营事件、手势/静音/断线换世界生命周期与仅己方真实脑态声化 | 必要编译与9个直接用例；节点/状态验证非硬件听验，无Hub音色资产链、观战/回放音频 |
| 已推进，仍有边界 | 第三十三批Lab持久提交/发现、租约fencing、取消/重试幂等、run检查点恢复、私有四表原子发布与原工作台/TS SDK | 本机HTTP/Node与同Store重建通过，非PG实库灾备/硬杀恢复；未完成run从种子重算，单作业汇总仍在内存 |
| 已推进，仍有边界 | 第三十四批Pi参考受控Adapter、独立持久运行/事件/预算、真工具收据、双控制进程取消续跑 | 设计借鉴非Pi代码复用；同源私有包/明确本地测试provider，不冒称LLM/独立注册Agent或PG灾备 |
| 已推进，仍有边界 | 第三十五/四十批私有配方与受限PCM采样、实际Arena声源/原相机二维空间化 | 设备DSP/耳机/真实浏览器未验；HRTF/3D/持续声源、分轨音乐/观战回放音频仍欠 |
| 已推进，仍有边界 | 第三十六批非既有角色参数实体模板/实例/绑定、原Engine执行、规范root/COMP/WAL与Studio/SDK | 有界I32/Q8_8/Bool RuleGraph，不是任意TS/rpack或物理ECS；Arena/10旧格式拒绝；第三十八批已接有界模型Lab，仍非任意脚本 |
| 已推进，仍有边界 | 第三十七批所有者定向源码编辑准入、原日志协作/选择性撤销、撤权与断线重连 | 单权威链/显式同步非离线多主；PG/浏览器未验，presence/光标/大历史索引仍欠 |
| 已推进，仍有边界 | 第三十八批一般参数模型→持久Lab/W3扫描→原四表/UI SDK真实消费 | 无原生身体/自然谱系/生命账；bounded RuleGraph不是任意TS/rpack，真HTTP/同Store重建非PG灾备 |
| 已推进，仍有边界 | 第三十九批真实发射、护盾/治疗、Spitter分泌与出生合成实付、规范根/WAL与Lab精确子账 | 显式玩家角色；不是一般生长/材料合成或EMIT/连续计算计价，真实PG/硬件仍未验 |
| 已推进，仍有边界 | 第四十批签名私有受限PCM→精确依赖→原Arena手势采样与可见实体/真实相机二维空间化 | 真HTTP/SDK/WS/PCM节点路径，录制设备不是DSP听验；旧v1数据保留不加适配 |
| 已推进，仍有边界 | 第四十一批签名私有受限.rview主题→精确依赖→原FrameScene/WorldRenderer八色消费 | 真HTTP/SDK/场景与llvmpipe离线回读，不是浏览器/独立显卡；不伪装工业美术 |
| 下一主体优先 | 完整生命成本仍欠的通用生长/材料体量、EMIT、连续神经/规则计价 | 按已有权威动作链实付，不造免费能量/体量，不把收费项名当完整生态 |
| 主体完成后集中 | 集成门禁、长期种群/资源稳定、性能、浏览器、硬件GPU、真实PG、断电、24小时 | 届时真实执行记录，不复用旧CI截图；现有网络策略不绕过 |

本阶段交付签名私有受限.rview主题与原renderer真实八色消费链；下一优先收拢生命成本仍欠的通用生长/材料体量、EMIT与连续计算计价。其它全产品欠项持续保留，不把视觉链完成当全项目完成。每个可交付阶段普通commit/push后继续主体，只有真实阻塞才请求最小用户动作。
