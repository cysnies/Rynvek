// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
using System.Globalization;
using System.Text.Json;
namespace Rynvek;

/// <summary>只计规则paid_q；不是世界总能耗。完整原提交键/精确根/预算由调用者显式提供。</summary>
public sealed record GeneralRunSpec(string SubmissionId, ulong ExpectedRevision, string ExpectedRoot, uint Ticks, uint WallSeconds, uint IntervalMs, string MaxEnergyQ, string Consent)
{
    public const string ExplicitConsent = "run_installed_rules_with_bounded_budget";
    public void Validate()
    {
        GeneralWorldClient.Uuid7(SubmissionId);
        if (!GeneralWorldClient.IsRoot(ExpectedRoot) || Ticks is < 1 or > 128 || WallSeconds is < 10 or > 900 || IntervalMs is < 200 or > 5000 || Consent != ExplicitConsent || !ulong.TryParse(MaxEnergyQ, NumberStyles.None, CultureInfo.InvariantCulture, out var q) || q is < 1 or > 1000000000000UL || q.ToString(CultureInfo.InvariantCulture) != MaxEnergyQ) throw new ArgumentException("须规范有界规则paid_q预算与明确同意");
    }
}
public sealed record GeneralRunOperation(string OperationId, uint Generation, string Action)
{
    public void Validate() { GeneralWorldClient.Uuid7(OperationId); if (Action is not ("pause" or "resume" or "cancel")) throw new ArgumentException("须明确pause/resume/cancel"); }
}
public sealed record GeneralRun(string StorageBackend, string Id, Guid WorldId, PrincipalId Owner, GeneralRunSpec Spec, ulong BaseTick, string LastRoot, uint Done, ulong EnergyUsedQ, string Status, uint Generation, ulong Fence, uint Attempt, string? Node, ulong LeaseUntilS, ulong DeadlineS, string? Error, Dictionary<string, GeneralRunOperation> Operations);
public sealed record GeneralOwnerConnection(ulong HostFence, JsonElement Observation);
/// <summary>原认证传输/Actor的General所有者入口；每次一个HTTP动作，不自动获取控制、循环Tick、重试或保存令牌。</summary>
public sealed class GeneralWorldClient
{
    private readonly RynvekClient _client;
    public Guid WorldId { get; }
    private string Path => $"/api/v1/worlds/{WorldId:D}/general";
    public GeneralWorldClient(RynvekClient client, Guid worldId) { if (worldId == Guid.Empty) throw new ArgumentException("须世界ID"); _client = client; WorldId = worldId; }
    internal static bool IsRoot(string? s) => s is {Length:64} && s.All(c => c is >= '0' and <= '9' or >= 'a' and <= 'f');
    internal static void Uuid7(string s) { if (!Guid.TryParseExact(s, "D", out var g) || g.ToString("D") != s || s[14] != '7' || s[19] is not ('8' or '9' or 'a' or 'b')) throw new ArgumentException("须规范UUIDv7"); }
    private JsonElement Observation(JsonElement v)
    {
        if (v.GetProperty("world_id").GetGuid() != WorldId || v.GetProperty("visibility").GetString() != "private_owner_only" || v.GetProperty("execution").GetString() != "server-authoritative-single-chunk" || !IsRoot(v.GetProperty("commit").GetProperty("state_root").GetString())) throw new RynvekProtocolException("General私有观察不符");
        var commit=v.GetProperty("commit"); if(commit.GetProperty("tick").GetUInt64()!=commit.GetProperty("revision").GetUInt64()) throw new RynvekProtocolException("General观察时序不符"); return v;
    }
    public async Task<JsonElement> ObserveAsync(CancellationToken ct=default) => Observation(await _client.RequestAsync(Path,HttpMethod.Get,null,true,ct));
    public async Task<GeneralOwnerConnection> ConnectAsync(CancellationToken ct=default)
    {
        var v=await _client.RequestAsync(Path,HttpMethod.Post,new{action="connect"},true,ct);var h=v.GetProperty("host_fence").GetUInt64();if(h==0)throw new RynvekProtocolException("宿主fence非法");return new(h,Observation(v.GetProperty("observation")));
    }
    public async Task<ControlLease> AcquireAsync(ulong hostFence,CancellationToken ct=default)
    {
        if(hostFence==0)throw new ArgumentException("须原宿主fence");var v=await _client.RequestAsync(Path,HttpMethod.Post,new{action="acquire",host_fence=hostFence,level="Sovereign"},true,ct);var lease=Wire.Read<ControlLease>(v.GetProperty("lease"));
        if(v.GetProperty("host_fence").GetUInt64()!=hostFence || lease.Level!=ControlLevel.Sovereign || lease.Holder.Id!=_client.PrincipalId?.ToString() || lease.Holder.Kind!=PrincipalKind.Human || lease.FencingToken==0)throw new RynvekProtocolException("原Sovereign控制回执不符");return lease;
    }
    private GeneralRun Receipt(JsonElement v)
    {
        var r=Wire.Read<GeneralRun>(v);r.Spec.Validate();Uuid7(r.Id);
        if(r.Owner!=_client.PrincipalId || r.WorldId!=WorldId || r.StorageBackend is not ("memory" or "postgres") || r.Status is not ("queued" or "running" or "paused" or "blocked" or "completed" or "cancelled" or "expired") || r.Done>r.Spec.Ticks || r.EnergyUsedQ>ulong.Parse(r.Spec.MaxEnergyQ,CultureInfo.InvariantCulture) || !IsRoot(r.LastRoot) || r.Attempt>5 || r.DeadlineS==0 || r.Operations is null || r.Operations.Count>64 || r.Status=="completed"&&r.Done!=r.Spec.Ticks || r.Done==0&&(r.EnergyUsedQ!=0||r.LastRoot!=r.Spec.ExpectedRoot) || ulong.MaxValue-r.Done<r.BaseTick || ulong.MaxValue-r.Done<r.Spec.ExpectedRevision)throw new RynvekProtocolException("运行归属/进度/持久等级不符");
        foreach(var (id,op) in r.Operations){op.Validate();if(id!=op.OperationId||op.Generation>=r.Generation)throw new RynvekProtocolException("运行控制收据代次不符");}return r;
    }
    public async Task<GeneralRun> StartAsync(ulong hostFence,ulong controlFence,GeneralRunSpec spec,CancellationToken ct=default)
    {
        spec.Validate();if(hostFence==0||controlFence==0)throw new ArgumentException("须原双fence");var r=Receipt(await _client.RequestAsync(Path,HttpMethod.Post,new{action="start_run",host_fence=hostFence,control_fence=controlFence,spec},true,ct));if(r.Spec!=spec)throw new RynvekProtocolException("原提交精确规格不符");return r;
    }
    public async Task<GeneralRun> GetAsync(string id,CancellationToken ct=default)
    {
        Uuid7(id);var r=Receipt(await _client.RequestAsync($"{Path}/runs/{id}",HttpMethod.Get,null,true,ct));if(r.Id!=id)throw new RynvekProtocolException("运行ID不符");return r;
    }
    public async Task<GeneralRun> LookupAsync(string submissionId,CancellationToken ct=default)
    {
        Uuid7(submissionId);var r=Receipt(await _client.RequestAsync($"{Path}/runs?submission_id={submissionId}",HttpMethod.Get,null,true,ct));if(r.Spec.SubmissionId!=submissionId)throw new RynvekProtocolException("原提交键不符");return r;
    }
    public async Task<GeneralRun> ControlAsync(string id,GeneralRunOperation op,CancellationToken ct=default)
    {
        Uuid7(id);op.Validate();var r=Receipt(await _client.RequestAsync($"{Path}/runs/{id}",HttpMethod.Post,op,true,ct));if(r.Id!=id||!r.Operations.TryGetValue(op.OperationId,out var receipt)||receipt!=op)throw new RynvekProtocolException("原操作收据不符；只读对账，勿自动重发");return r;
    }
}
