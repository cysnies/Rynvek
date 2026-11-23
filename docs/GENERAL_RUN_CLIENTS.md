<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# General耐久运行：SDK、CLI、Web及C#（阶段59）

从干净PUBLIC `c86a97c16a320d05669761d63991d102837aebb0` 接续，核原ACTIVE及最终讨论D337可恢复任务/不重复应用、D386幂等、D570服务端撤权边界；**不重做阶段58调度**，复用[服务器耐久链](GENERAL_DURABLE_RUNS.md)。本轮每个客户端动作仅一次HTTP：意图由服务器后台推进，GET只读等待不是客户端Tick循环。

## 明确启动与丢失回执

- 正式Human owner、当前原Sovereign双fence、精确revision/root，以及显式RunSpec缺一不可。不自动connect/acquire/续租、变更Epoch、重试或换提交键。
- `ticks=1..128`、`wall_seconds=10..900`（包括暂停/排队/故障）、`interval_ms=200..5000`、规范正整数文字 `max_energy_q<=1000000000000`，以及精确consent `run_installed_rules_with_bounded_budget`。
- 费用只累计原rule_meter.paid_q之和，不承诺世界总能耗预算。自然代谢/物理仍受原有来源守恒及世界Tick/墙钟总上限约束。
- 发送前保留规范submission UUIDv7及完整规格；失回执用原键lookup，不重新认领宿主或生成新键假装幂等。GET ID/提交键、POST控制的真实API沿用58。
- pause/resume/cancel须显式operation UUIDv7、原generation，精确收据才确认；同键不能换动作/代次。恢复不延原预算/墙钟、不清费用或跳Epoch，取消不回滚已提交Tick。
- `storage_backend=memory`仅同进程治理；`postgres`元数据仍需原私有文件CAS和恢复链。客户端重建/重登录不是服务进程或数据库掉电实证。

## TypeScript SDK

`@rynvek/agent-sdk`导出 `GeneralRunClient(ownerId, request)`、`GeneralRunSpec`、`GeneralRunOperation`、`GeneralRun`及明确consent常量。request复用原RuleRequest/有界readRuleJson，传输负责可信origin/内存Bearer/取消；本轮Web复用既有安全raw。SDK不自行造token/信任公钥或网络循环。

```ts
const runs = new GeneralRunClient(ownerId, request);
const job = await runs.start(worldId, hostFence, controlFence, spec, signal);
const current = await runs.lookup(worldId, spec.submission_id, signal);
const paused = await runs.control(worldId, job.id, {
  operation_id: originalOperationKey, generation: current.generation, action: "pause"
}, signal);
```

start/control在await前固定全部原始字段，避免调用方中途修改规格。严格校验UUID/十六进制根、u64所有者文字、安全整数与边界、回执字段/归属/进度/预算/持久等级；start须精确同spec，get/lookup须原键，control须原operation收据。响应512KiB硬界，HTTP拒绝不回显正文；无自动重发。

## Rust SDK与CLI

原 `private_rule_client::PrivateRuleClient` 新增 `run_start` / `run_status(..., submission)` / `run_control`，复用原15秒有界传输、服务器RunSpec/GeneralRun/OwnerAction、归属/预算/原收据验证。CLI继续原Connection，`--trusted-key`沿用连接类型（耐久记录并非私包签名，不声称此字段签署job）；令牌可来自 `RYNVEK_TOKEN`，不放URL或回显。

```sh
rynvek rule run-start --server http://127.0.0.1:3000 --owner "$OWNER" --trusted-key "$PUBLIC_KEY" --world "$WORLD" --host-fence "$HOST" --control-fence "$CONTROL" --spec run-spec.json
rynvek rule run-lookup --server http://127.0.0.1:3000 --owner "$OWNER" --trusted-key "$PUBLIC_KEY" --world "$WORLD" --submission-id "$SUBMISSION"
rynvek rule run-status --server http://127.0.0.1:3000 --owner "$OWNER" --trusted-key "$PUBLIC_KEY" --world "$WORLD" --run "$RUN"
rynvek rule run-control --server http://127.0.0.1:3000 --owner "$OWNER" --trusted-key "$PUBLIC_KEY" --world "$WORLD" --run "$RUN" --operation original-operation.json
```

spec文件为上列完整RunSpec（最多16KiB），operation文件为原三字段JSON（最多4KiB）。不自动选择动作/预算/起点或产生UUID。极简Rust传输仍仅接受带明确端口的literal loopback HTTP，**未放宽为带凭据明文外站**；远端TLS传输仍欠，不能把回环CLI称全网部署完成。

## Web实际用户闭环

原General所有者工作台新增独立“服务器耐久运行”面板；旧客户端有界逐Tick操作保留明确旧含义，绝不作为后台意图实现。

- 单独展示Tick/墙钟/间隔/规则paid_q预算；明确准备/保留提交UUIDv7、主动checkbox、原观察与双fence后才能启动。
- 失回执按原提交键只读对账，或手填run ID读取；展示状态/进度/费用/storage/deadline，pause/resume/cancel分开操作，须准备原operation键。
- 控制失败保留原键/动作/代次并禁止直接自动重发；GET精确收据才消除“未确认”。明确准备新控制键只放弃本地待确认记录，不撤销旧请求。
- 成功start/control清旧观察和双fence；run不是完整实时世界观察。WeakMap回执epoch绑定世界/会话，断开、隐藏、offline、身份/世界失效、组件卸载后迟到成功不能复活私有job或原观察。
- 私有失效清job与精确根规格，只保留本会话绑定世界的对账/操作键；无token/任务localStorage，没有自动HTTP推进或Tick计时器。
- 关闭页面/断开/隐藏只清本地显示，不是服务器cancel，已同意的有限意图仍可能继续。撤回用明确cancel。暂停/blocked保留世界，维护换装先cancel。

## C#继续真实主体

主链后实际新增 `GeneralWorldClient`，复用已认证RynvekClient、原所有者observe/connect/acquire和同一耐久start/get/lookup/control；不是空csproj或假接口。u64修订/fence/费用保持精确，spec/op为不可变record。真实.NET10.0.404编译及签名挑战/重登录/原后台同根实付通过，细节见[原C#说明](../sdks/csharp/README.md)。这不等于C#私包签名发布/完整依赖安装用户链完成，也不把Owner私有观察给driver。

## 实际验证、未验与下一主体

本批100个不同自动用例：SDK39、Web工作台15与driver/viewport/Studio21、CLI真实子进程2、General20（该文件Node SDK/Web ignored显式执行）、C#3（两个真实场景均显式执行），其中13个新增；重跑不累计。另C#4组自检单列。Node v22.23.5，.NET SDK10.0.404/运行时10.0.15；SDK/Web类型与构建通过，Web正常build-wasm实际重编wasm32与Vite生产包，但**没有操作真实浏览器或硬件GPU**。

真实CLI完成显式128步启动→原键查询→暂停/恢复/暂停/取消与操作重放，再新意图后台3步付费同根；真实Node SDK/Web包含关闭客户端后仅GET等待后台3步、只读重连、暂停/恢复/取消、新意图3步与世界有源守恒。C#自己两次挑战登录、完整同类后台链及原PvE fog回归，未借Rust Bearer。DOM同时测失败控制原收据对账、私有失效及迟到回执。

保留58真实PG17.12应用SIGKILL新独立进程3→8实证，**本批没有新跑PG**，不泛称PG/OS断电、多机网络分区或24h长稳。本批100与旧58的42不相加当覆盖率。

多节点亲和/公平/治理并发、协作者明确耐久委托、全平台QuickJS(WASM仍本地Tier1整组拒绝)、C#私包流程/全规范Agent与World、真实browser/GPU及PG多机断电长稳仍欠；完整workspace/Clippy/license/SBOM/安全发布门禁仍待主体齐后集中验收。旧6个PvP_Custom夹具须真实lease，不降低生产fence。未调用外部模型/支付或外发私有资料，未部署/绕loopback策略。**整体未完成，不标正式发布质量。**
