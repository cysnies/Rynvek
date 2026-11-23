<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception -->
# Rynvek C# SDK — actual authenticated private-world consumption

目标 `.NET 10 LTS`，本阶段本箱官方安装脚本安装SDK `10.0.404`。纯BCL，无NuGet运行依赖，项目`IsPackable=false`，**未公开NuGet发行**。源码许可沿用客户端SDK的`Apache-2.0 WITH LLVM-exception`；嵌入的六工具schema直接来自`protocols/schemas/agent-tools.v1.json`（原公开协议许可），不另外维护漂移副本。

## 已可用场景

正式Ed25519挑战会话 → 发现/读取本人世界 → 显式创建或选择已有Private PvE → 运行/重连原Actor → JSON WebSocket `rynvek-arena/1` welcome/本人fog → 当前控制租约/fencing → 真实命令移动与原ToolBroker → 回执/真实观测 → 交还租约/断开清理 → 以同一身份重新登录恢复同一match/world/位置。不是空csproj、假接口或C#本地仿真。

- `RynvekClient.RegisterAsync`需调用者明确注册；`LoginAsync`登录已有主体。签名回调只接收域分离消息并返回64字节Ed25519签名，私钥由调用者已有签名器/硬件/凭据管理持有，不从文件/环境隐式读取、不写入包。服务端实际验签，SDK不伪造签名。
- `ListWorldsAsync/GetWorldAsync/CreatePrivatePveAsync/ConnectPrivatePveAsync`调用真实HTTP。此阶段补齐`GET /api/v1/worlds/{id}`元数据端点，复用原`load_world`/WorldRead授权；不存在与他人私有世界均404，成功private,no-store/Vary Authorization，不含私有快照/源码/CID。不假装这个GET早已存在。
- `ArenaConnection`只支持**本人Private单人PvE**入口，校验运行回执/世界/正式主体/阵营/模式/非ephemeral；不调用practice，不自动创建玩家/对手，不伪装完整排位或多人SDK。
- `AcquireLeaseAsync/ReleaseLeaseAsync`及当前可空seq的租约通知、lease_revoked均按原协议；核对holder/阵营/类型/有界租期，拒绝fence回退。真正服务端仍最终鉴权与fencing；客户端本地检查不是授权证据。
- 六项`ToolCall`构造器：QueryVisibleClusters、SampleRegionalFields、IssueStrategicIntent、ConfigureSignalBeacon、QueueVatProduction、TriggerCrucibleBreeding。`FromJson`也只接受权威六工具schema；未知/重复字段、缺字段、枚举/长度/类型/范围错误拒绝。没有第七工具、底层工具或客户端advance旁路。局部schema通过不代替归属/迷雾/资源/控制层/30Tick/600APM等原ToolBroker裁决。
- `RtsCommand`当前明确支持精确原始Q32.32坐标的IssueOrder与Surrender；其它命令不提供伪接口，生产/信标/科技等可走已接通六工具。所有变更只在原服务端Actor/Tick/WAL上发生。

## 直接消费例子

```csharp
using Rynvek;

// signer由应用已有Ed25519签名器提供：不得把私钥或会话令牌放进Agent包。
public static async Task<ToolReceipt> ObserveOwnedWorld(
    Uri trustedOrigin, PrincipalId principal, Guid existingWorld,
    SignMessage signer, CancellationToken cancellationToken)
{
    await using var client = new RynvekClient(trustedOrigin);
    await client.LoginAsync(principal, signer, cancellationToken);
    var world = await client.GetWorldAsync(existingWorld, cancellationToken);
    await using var arena = await client.ConnectPrivatePveAsync(world.WorldId, cancellationToken);
    var fog = await arena.WaitForObservationAsync(ct: cancellationToken);
    await arena.AcquireLeaseAsync(ControlLevel.Sovereign, cancellationToken);
    var receipt = await arena.CallToolAsync(ToolCall.QueryVisibleClusters(false), cancellationToken);
    // 检查receipt.Accepted/Error；发送/本地预检并不代表成功。
    await arena.ReleaseLeaseAsync(cancellationToken);
    return receipt;
}
```

要创建新世界须显式`CreatePrivatePveAsync(title, seed)`，重连已有世界不要重新创建。要移动须从`FactionObservation.Units`选择本人单位，`RtsCommand.IssueOrder(ids, OrderKind.Move, new RawVec2(xRaw,yRaw))`→`SendCommandAsync`；不要把浮点tile坐标强转成原始Q32.32。

## 精确整数与生命周期

- Principal/Org/owner是`PrincipalId`（u64），JSON强制规范十进制**字符串**，拒绝用浮点/JSON数替代。Tick、seq、fencing、seed、revision是`ulong`原生JSON整数；实体序号`uint`，资源/生命值/Q32.32原始坐标`long`。`RawVec2`严格两元素i64数组，无隐式double或“显示换算后再写回”。
- 当前必需`FactionObservation.Neural`与服务端一致：仅己方真实LIF/Izhikevich的64子步重叠环窗口，以及MLP/CTRNN连续隐藏态均值；不把它当新增spikes/瞬时频率/相干度。C#这里只消费精确观测，不声称实现原生音频播放。
- 工具连续值用decimal构造JSON，服务端依原规则量化；未知/超界仍由原工具拒绝。`JsonElement`保留未细分结果/事件/终局的原始数字，不反序列化到object/double。Decimal128整数文本可用`Wire.ExactInteger`转BigInteger（含2^96），不声称SDK已实现Lab客户端。
- 所有HTTP默认不重定向、不使用Cookie、不自动重试，响应≤1MiB；只接受无用户名/密码/path/query/fragment的HTTP(S) origin。HTTP仅允许loopback，其余须HTTPS；TLS使用系统默认校验，无跳过证书或网络策略的选项。Bearer只在认证头和WS Hello JSON，绝不放URL；没有公开token getter，ToString/请求异常不打印会话/响应体/私钥。
- 单WS reader、串行sender、单调u64 seq、最多32个在途请求；文本分片消息整体≤1MiB。候选/回执按seq和消息类型关联；工具名不符/未知协议/错误阵营立即终止，不把观察或别人的回复当成功。
- 请求超时或取消一旦进入发送流程，结局可能未知，终止连接/清缓存/使租约失效，抛`RynvekOutcomeUnknownException`，**不自动重试**。它不证明服务端已执行，也不证明没执行；显式重连/对账后由调用者决定。断开不暂停世界、不撤回已提交命令，不声称网络exactly-once。
- Dispose/掉线/服务端错误清空本地fog、events和lease，结束观察等待并结清在途任务，解绑子会话；新身份用新client。重新登录/重连获得原当前状态及新租约，不保留旧fence。托管字符串释放引用不声称硬件级内存擦除。
- `ReadNoticesAsync`是128项丢最旧的**观察性通知**，慢消费者不能阻塞权威回执。它不是完整审计日志、模型会话或持久事件源；可信工具结果必须来自具体调用的receipt，持久权威来自服务端原WAL。

## 本阶段实际验证命令

```sh
# 如无工具链，只用官方普通安装方式；不需要管理员/第三方镜像。
curl -L https://dot.net/v1/dotnet-install.sh -o /workspace/tooling/dotnet-install.sh
bash /workspace/tooling/dotnet-install.sh --channel 10.0 --quality GA --install-dir /workspace/tooling/dotnet --no-path
export DOTNET_CLI_TELEMETRY_OPTOUT=1
/workspace/tooling/dotnet/dotnet build sdks/csharp/Rynvek.Client.Smoke --nologo
/workspace/tooling/dotnet/dotnet run --project sdks/csharp/Rynvek.Client.Smoke --no-build -- --self-test
RYNVEK_DOTNET=/workspace/tooling/dotnet/dotnet cargo test -p rynvek-server --test csharp_sdk --offline -- --include-ignored
```

自检3组：精确u64/i64/Q32.32/2^96与跨语言鉴权字节、六schema构造/严格拒绝、头内凭据/重定向拒绝/销毁边界（传输夹具）。Rust2项：权威鉴权/schema跨语言夹具；**实际编译C#进程**通过本机真实服务完成注册持有证明/挑战验签取会话、HTTP私有世界、超过2^53的真实seed、WS入座/租约/fog、真实单位位移、查询结果、间隔30Tick后战略HoldPosition/真实order、交还、断开清理、同主体重新登录/原位置同match、他人私有404及真实终局。后者外部前提是.NET10及已构建DLL，普通cargo默认ignored，本阶段显式启用并实际运行，不把跳过当通过。测试签名器由Rust持有临时测试key，经stdin回签公开协议消息，**C#自己取真实会话，未借Rust会话令牌**；不碰用户真实私钥。

未全workspace/Clippy/license/SBOM/soak/CI轮询，未重编WASM/Vite；真实浏览器/GPU/音频硬件/PG/多机/掉电/24h、Windows/macOS、发布NuGet未验/未做。本段未覆盖二进制Rynvek Link/WebTransport、排位/多人/观战、私有Agent包构建验签或全部Studio/Lab API，不冒称完整全域C# SDK。下一按依赖接音频主体；Pi Agent Core参考与后续Agent控制面选择记录同一ACTIVE_DEVELOPMENT/Page，不因参考建议推倒当前SDK。


## General服务器耐久规则主体（阶段59）

`GeneralWorldClient(client, worldId)` 复用同一个正式挑战认证/安全HTTP传输，提供 `ObserveAsync`、显式 `ConnectAsync`/`AcquireAsync`，以及 `StartAsync`、`GetAsync`、`LookupAsync`、`ControlAsync`。每次只发一个动作，无自动获取lease/循环Tick/重试。`GeneralRunSpec`、`GeneralRunOperation`为不可变record，u64修订/fence、规则费用与PrincipalId保持精确；查询检查世界/所有者/运行ID/预算/持久等级，启动检查精确规格，控制检查原operation收据。

调用者须显式保留原submission UUIDv7/双fence/精确观察根、全部预算与 `GeneralRunSpec.ExplicitConsent`；费用只累计原规则paid_q，非世界总能耗。控制动作仅pause/resume/cancel，调用者保留原operation UUIDv7/generation；失败先只读对账，不自动换键。Memory仍仅进程内，PG持久性仍依赖原服务器Store+私有CAS；C#对象重新登录不是PG或服务进程恢复证明。

本批真实.NET SDK10.0.404编译0警告、4组自检；`csharp_sdk`三项显式include-ignored通过，其中新C#进程自己两次真实挑战登录（只把公开挑战交给临时测试签名器，未借Rust Bearer），经真实HTTP启动128步有界意图、暂停/恢复/暂停/取消、操作重放、断开重登录只读对账，再新意图后台完成3实付Tick并与原Store同根。原PvE/fog/六工具/原Actor重连仍实际通过。

工具恢复：本箱旧SDK的SourceLink `Microsoft.Build.Tasks.Git/build`目录缺失。正常从微软10.0.404发布元数据获取官方Linux x64包，SHA512核验后解压到独立 `/workspace/rynvek-tools/dotnet-10.0.404`，保留旧目录与归档；未关闭SourceLink或保护。可将上文命令中的工具路径换为此目录。

C#尚未覆盖私有规则RYPK签名发布/完整依赖验签安装/隔离换装用户链；General所有者观察不是给driver的fog观察，也不新增协作者耐久授权。QuickJS(WASM)、真实浏览器/GPU、PG多机断电长稳、NuGet/跨OS及完整发布门禁依旧未完成。
