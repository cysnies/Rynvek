<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 单Chunk通用World：原权限/租约/事务链的服务器入口

阶段49从干净 `1581eb27928ec7321d4c05243c202629500c3b59` 继续。**整体未完成，未达正式发布质量**。这是单Chunk圆形物理、有来源生命、创世声明规则的服务器领域适配，不是任意World、第二套权限服务或假Arena比赛。

## 唯一控制与持久链

原Ed25519挑战登录/CapabilityToken → 原Subject/Active/ABAC/WorldRead+Connect+Edit → 原个人组织配额/Private WorldRow → 有界Tokio通用领域Actor → 原LeaseManager/CommandGate/APM → 原SimulationEngine候选Tick/规则实付 → 私有CAS输入日志/周期快照 → **原TickCommitter.publish_row/Store.append_commit_checked** → Store确认后才发布Actor内存与回执。

- Arena仍用原Actor/可见性/事务机制；不复制其鉴权表、租约实现、幂等门、能量模型、规则解释器或Store提交事务。新增Actor只适配不同SimulationEngine领域，不伪造Faction/MatchRow。
- 原 `ranked_host_leases` 及同一MemoryStore表被精确扩展给 `General_Circle_v1 + General_Controlled + D0_Strict + Private`；排位原有非终局资格保留，其余World不能因此取得宿主。表名沿用，不增平行租约表。
- 30秒耐久宿主租约：原CAS认领增加fence，原续租检查真实过期，原Store提交再次检查fence/过期/前驱及owner/editor Active。健康远端不可抢占，不转发Bearer。旧宿主不能续租/提交/返回Actor观察。
- 控制域键0是单World控制槽，不创建RTS阵营。控制租约900 Tick，同人续租也换token，Sovereign仅原Human规则准许；每个控制请求绑定 `host_fence + control_fence`，避免Actor重建后计数从1复活旧控制令牌。
- 单节点最多64个通用Actor，每个有界64消息邮箱；命令帧64KiB、最多32条；复用原CommandGate UUIDv7/主体/时序/窗口与600动作/1800 Tick APM。空Tick是所有者显式调度，不按生物动作收费，也不把CPU额度叫生物能量。
- 本版没有自治30Hz调度；只有Sovereign所有者提交精确下一Tick。请求执行时及返回前重新验证原令牌/ABAC/Active，回执前再查宿主fence/过期。请求之间续租，不后台心跳；空闲25秒或close退役Actor，原30秒租约失效后才允许换宿主，不能在同一健康fence下重置控制token。正常服务停机仍用原drain失效全部本节点宿主。

## HTTP入口（使用原服务器，不是新服务）

| 路径 | 实际语义 |
|---|---|
| `POST /api/v1/worlds/general` | 原world.create及个人组织配额；二进制原RYWL，沿GeneralWorldConfig严格解析、128KiB上限。创建私有World/创世清单/完整快照，返回world_id。当前固定中文标题，无组织选择/公开可见性参数。 |
| `GET /api/v1/worlds/{id}/general` | 所有者Private准入，从原耐久提交恢复当前快照+WAL，输出真实身体serial/坐标、组件动态值、规则计量、源账户/闭世界摘要；不是读可能过期的Actor缓存，不抢占宿主。 |
| 同路径 `POST action=connect` | 显式恢复/认领或访问已有本地Actor，返回host_fence与当前观察；健康远端/退役中旧fence返回409，不代理到别服。 |
| `action=acquire` | `host_fence, level`；原ControlLevel名称Spectate/Perceive/Modulate/Produce/Sovereign。返回原lease.fencing_token，后续用作control_fence。低级控制不能Tick。 |
| `action=release` / `close` | `host_fence, control_fence`，先原租约authorize；close另需Sovereign且退役Actor，不删除世界/历史，不提前抹掉原宿主fence。 |
| `action=tick` | `input: {host_fence, control_fence, envelope}`；envelope沿原CommandEnvelope，含UUIDv7 idempotency_key、正式principal_id、issued_at_tick=当前、target_tick=下一Tick、payload物理命令数组。只确认真实已提交候选，不接受客户端快照/根/任意推进时间。 |

成功私有响应为 `Cache-Control: private, no-store; Vary: Authorization`。无认证401，他人私有World404，缺scope/低控制/冒名拒绝。Agent默认令牌没有world.create；本批不新增提权/代发令牌接口。General不进入原Arena快照恢复/分叉/WorldPatch，明确拒绝尚未适配的入口，不猜格式、不以假成功应答。

观察仍为**仅所有者的明确全权视图**，不是已实现通用World多人fog。原Arena fog完全不动；不把共享组织、Studio定向编辑者或观战者加入全知视图。本批未接通General WebSocket/浏览器SDK/UI/协作，多主体工具化接入仍须后续。

## 命令与有来源边界

无生命物理World的Spawn/Despawn/SetAcceleration是Sovereign外部编辑输入，不冒充自然产物。有生命World只允许显式Despawn并走原残骸回收；Spawn与外部加速度拒绝，防止免费增料/绕过原受控付费规则与LIF。空命令Tick仍实际执行已声明Tier0/Tier1规则、场摄取与有来源EnergyAccount扣款/热耗。欠费、规则故障、非法物理或结构命令由原候选Tick整帧回滚；Store拒绝后原Actor退役，不发布候选。

规则计算费仍是阶段48的许可/热耗模型，不是严格机械功—化学能转换，机械质量不是生命材料；本批没有扩展生物能量或物质理论，也不向Arena生态/Tier1记第二笔账。

## 私有恢复与幂等

- 每个Tick `rynvek-general-tick/1` 记录原命令信封、提交当时双fence、前驱revision/root及原RYC2清单；只保存正式已认证主体ID，不写Bearer或凭据。命令输入与周期锚点只入私有world_snapshots，不进公共Hub CAS。
- 每300 Tick同一完整快照；原Store world_commits是唯一可见索引。恢复校验提交连续性、世界身份、CID摘要/大小与锚点，至多300步后缀逐Tick真实重仿真并比较全部领域清单/最终WorldRow根；无法验算拒绝，不使用客户端最终镜像代替输入重演。
- 恢复最近1800 Tick原幂等回执与物理动作APM；新宿主重取控制租约后，同键同完整信封返回原提交回执不重执行，同键换载荷拒绝。旧双fence即使信封相同也先拒绝。幂等窗口不是永久业务ID索引。
- 执行/存储失败可能留下不可达CAS对象，但不前移WorldRow/Actor。超时、断连或回执前授权/fence失效时，Tick可能已提交：先读取耐久观察对账，换宿主后以新双fence与原信封请求回执，**不能自动生成新key重发动作**。
- 查询当前仍沿原 `list_commits` 取索引，再限制重放/回执窗口；长期索引分页/吞吐未认证。缺省内存Store不跨进程持久化；要真实服务持久化须原PG+私有文件CAS，不能把本批MemoryStore复用说成PG断电实证。

## 已跑的必要快验与未验

14个不同定向用例通过，新增10，重复运行不累计：

- `cargo test -p rynvek-server --test general_authority --offline --locked -- --test-threads=2`：9个真实回环HTTP用例。临时测试Ed25519登录、原身份隔离/scope/Active、低级及Agent Sovereign拒绝、续租/释放旧token、冒名/坏时序/超量、真实付费组件驱动物理与闭世界、欠费/坏命令原子回滚、同键冲突与重启去重、健康对端不抢占/旧宿主拒绝、私有CAS、损坏WAL拒绝、301 Tick跨300锚点+后缀恢复、显式旧Arena入口拒绝。新AppState+同MemoryStore+私有文件CAS，不是真PG/断电。
- `cargo test -p rynvek-server --lib general_host_keeps_original_expiry_fence_and_checked_owner_commit --offline --locked`：1个新内存Store可控时钟边界测试，原租约过期不续租/不提交、换fence、owner撤销后的原子不发布；不等待实际30秒，更不是PG时钟试验。
- 原 `ranked_discovery` 2、`expired_ranked_host_cannot_renew_commit_or_finalize_and_unseated_reservation_expires` 1、`web_arena::private_custom_pvp_full_seats_fog_reconnect_recovery_and_leave` 1通过。
- 必要server原生编译包含PG代码类型检查；**SQL未连接真实PG执行**。新增Rust文件/测试模块rustfmt与diff检查、必要敏感模式筛查通过，不等同完整安全审计。

本批未跑全workspace/Clippy/license/SBOM/完整安全/soak/CI轮询。Node22正式运行时、C#重编、wasm32、真实浏览器硬件GPU、真实PG并发SQL/多机/断电/24h长稳依旧未验；llvmpipe仍只保留阶段41软件回读口径。旧6个PvP_Custom Actor夹具须补真实lease前提，禁止降低生产fence。

下一主体：TS AST、完整私有rpack签名依赖发现/验签/安装链；通用权限链虽已接入，还不是所有Actor单一实现/协作准入及General浏览器流。完整材料/生长/自然遗传迁入、多Chunk/三维/任意组件、Agent独立赛道、presence/离线多主等仍欠。主体齐后集中完整门禁、安全与正式发布打磨。


## 阶段51接续（保留上文当批历史边界）

现已接通有界Tier0私有签名精确包组的真实HTTP发布/发现/逐级下载验签、原权限Actor下一Coupling Epoch显式安装、原有源实付Tick及私有来源证据恢复，详见[PRIVATE_RULE_PACKAGES.md](PRIVATE_RULE_PACKAGES.md)。不修改本地叶子RYPK三条目fence，不把此共同安装剖面称全规范跨种类锁/任意World完成；历史撤销与活Tick分开，非Epoch暂停后安全续跑仍欠，PG多机/断电/真实硬件和完整发布门禁仍待实证。


## 阶段52接续（保留上文历史边界）

已接通CLI/TS可信私有规则用户链，及撤销后非Epoch的显式有源隔离维护逐Tick→Epoch才安装续跑；原Store撤销/fence提交锁零修改。隔离不执行旧/新图、不免费Tick/补能/自动降级，维护成本单列而非伪造gas；仅全Tier0正价有源存活控制身体剖面。详见[PRIVATE_RULE_CLIENTS.md](PRIVATE_RULE_CLIENTS.md)。原“尚无客户端/非Epoch续跑”属于阶段51历史，C#流程/General浏览器协作自治/真实PG及完整正式发布门禁仍欠，不称任意rpack/World完成。
