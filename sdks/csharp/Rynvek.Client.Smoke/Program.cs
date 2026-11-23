// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
using System.Net;
using System.Numerics;
using System.Text;
using System.Text.Json;
using Rynvek;

internal static class Program
{
    private static string _stage = "start";
    private static void Assert(bool yes, string message) { if (!yes) throw new Exception(message); }
    private static void Reject(Action action) { try { action(); } catch (Exception e) when (e is ArgumentException or RynvekProtocolException) { return; } throw new Exception("Expected strict rejection"); }
    private static void Emit(object value) { Console.WriteLine(JsonSerializer.Serialize(value, Wire.Json)); }
    public static async Task<int> Main(string[] args)
    {
        try { if (args.Contains("--self-test")) await SelfTest(); else if(args.Contains("--general-run")) await GeneralRunScenario(); else await RealScenario(); return 0; }
        catch (Exception e) { Console.Error.WriteLine($"C# direct validation failed at {_stage} ({e.GetType().Name}); response/session/private contents redacted"); if(e is RynvekRequestException request) Console.Error.WriteLine($"HTTP={request.Status} code={request.Code:x4}"); return 1; }
    }
    private static async Task<byte[]> Sign(string label, ReadOnlyMemory<byte> message, CancellationToken ct)
    {
        // Test driver owns ephemeral test keys; only public domain message goes to stdout, never capability/private key/signature.
        Emit(new { sign = Convert.ToHexStringLower(message.Span), key = label });
        var line = await Console.In.ReadLineAsync(ct) ?? throw new Exception("Signer disconnected");
        using var document = JsonDocument.Parse(line); return Convert.FromHexString(document.RootElement.GetProperty("signature").GetString()!);
    }
    private static async Task RealScenario()
    {
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(40)); var ct = deadline.Token;
        using var config = JsonDocument.Parse(await Console.In.ReadLineAsync(ct) ?? throw new Exception("Missing test configuration")); var c = config.RootElement;
        var origin = new Uri(c.GetProperty("base").GetString()!); var publicKey = Convert.FromHexString(c.GetProperty("public_key").GetString()!);
        await using var first = new RynvekClient(origin);
        _stage = "register"; await first.RegisterAsync("csharp-owner", PrincipalKind.Human, publicKey, (m,t) => new(Sign("owner",m,t)), ct);
        var principal = first.PrincipalId!.Value;
        _stage = "world-create"; const ulong seed = 9007199254740993UL; var world = await first.CreatePrivatePveAsync("C#真实权威消费", seed, ct);
        _stage = "world-list"; var list = await first.ListWorldsAsync(ct: ct); Assert(list.Worlds.Length == 1 && list.Worlds[0].WorldId == world.WorldId, "C# world discovery");
        _stage = "ws-connect"; await using var arena = await first.ConnectPrivatePveAsync(world.WorldId, ct); var match = arena.Run.MatchId;
        _stage = "observation"; var initial = await arena.WaitForObservationAsync(ct: ct); Assert(initial.Units.All(u => u.Faction == 0) && initial.Units.Length > 0, "Real fog cannot leak far enemies");
        Assert(arena.Welcome!.Map.Seed == seed, "Seed must remain exact above 2^53");
        _stage = "lease"; var lease = await arena.AcquireLeaseAsync(ControlLevel.Sovereign, ct); Assert(lease.Holder.Id == principal.ToString(), "Real own lease");
        var unit = initial.Units[0]; var target = new RawVec2(checked(unit.Position.X + (4L << 32)), unit.Position.Y);
        Emit(new { phase = "move", world_id = world.WorldId }); var command = await arena.SendCommandAsync(RtsCommand.IssueOrder([unit.Id], OrderKind.Move, target), ct); Assert(command.Accepted, "Raw exact command must be accepted");
        var moved = await arena.WaitForObservationAsync(31, ct); Assert(moved.Units.Single(u => u.Id == unit.Id).Position != unit.Position, "Original Actor position must really move");
        Emit(new { phase = "query", world_id = world.WorldId }); var query = await arena.CallToolAsync(ToolCall.QueryVisibleClusters(false), ct); Assert(query.Accepted && query.Result!.Value.GetProperty("clusters").GetArrayLength() > 0, "Real ToolBroker query result");
        await arena.WaitForObservationAsync(query.Tick + 30, ct);
        Emit(new { phase = "strategic", world_id = world.WorldId }); var strategy = await arena.CallToolAsync(ToolCall.IssueStrategicIntent(UnitArchetype.All, OrderKind.HoldPosition, 1m, 1m), ct);
        Assert(strategy.Accepted && strategy.Result!.Value.GetProperty("ack").GetProperty("units").GetUInt32() > 0, "Real strategic mutation");
        _stage = "strategic-observation"; var held = await arena.WaitForObservationAsync(strategy.Tick, ct); Assert(held.Units.Any(u => u.Order == OrderKind.HoldPosition), "Authoritative unit order must change");
        await arena.ReleaseLeaseAsync(ct); Assert(arena.Lease is null, "Lease release confirmed");
        try { await arena.CallToolAsync(ToolCall.QueryVisibleClusters(), ct); throw new Exception("Write without lease"); } catch (InvalidOperationException) { }
        var heldPosition = held.Units.Single(u => u.Id == unit.Id).Position;
        _stage = "dispose"; await first.DisposeAsync(); Assert(!first.IsAuthenticated && arena.Observation is null && arena.Lease is null, "Disposed session must clear private view/control");
        _stage = "reconnect"; await using var resumed = new RynvekClient(origin); await resumed.LoginAsync(principal, (m,t) => new(Sign("owner",m,t)), ct);
        await using var reconnected = await resumed.ConnectPrivatePveAsync(world.WorldId, ct); var recovered = await reconnected.WaitForObservationAsync(strategy.Tick, ct);
        Assert(reconnected.Run.MatchId == match && recovered.Units.Single(u => u.Id == unit.Id).Position == heldPosition, "Reconnect must retain original match/position, not reset template");
        _stage = "outsider"; await using var outsider = new RynvekClient(origin); await outsider.RegisterAsync("csharp-outsider", PrincipalKind.Human, Convert.FromHexString(c.GetProperty("other_public_key").GetString()!), (m,t) => new(Sign("other",m,t)), ct);
        var denied = false; try { await outsider.GetWorldAsync(world.WorldId, ct); } catch (RynvekRequestException e) when (e.Status == 404) { denied = true; } Assert(denied, "Other identity cannot read owner private world");
        await outsider.DisposeAsync(); await reconnected.AcquireLeaseAsync(ControlLevel.Sovereign, ct);
        Emit(new { phase = "finish", world_id = world.WorldId }); var surrender = await reconnected.SendCommandAsync(RtsCommand.Surrender(), ct); Assert(surrender.Accepted, "Real finish command");
        var ended = await reconnected.WaitForObservationAsync(surrender.Tick, ct); Assert(ended.Outcome is not null, "Original Actor terminal observation");
        Emit(new { done = true, principal_id = principal, world_id = world.WorldId, match_id = match, exact_seed = seed, moved = true, query = query.Tool, strategy = strategy.Tool, reconnect = true, outsider_denied = true, completed = true });
    }
    private static async Task GeneralRunScenario()
    {
        using var deadline=new CancellationTokenSource(TimeSpan.FromSeconds(25));var ct=deadline.Token;
        using var config=JsonDocument.Parse(await Console.In.ReadLineAsync(ct)??throw new Exception("Missing configuration"));var p=config.RootElement;var origin=new Uri(p.GetProperty("base").GetString()!);var principal=PrincipalId.Parse(p.GetProperty("principal").GetString()!);var world=p.GetProperty("world").GetGuid();
        await using var client=new RynvekClient(origin);_stage="general-login";await client.LoginAsync(principal,(m,t)=>new(Sign("owner",m,t)),ct);
        var general=new GeneralWorldClient(client,world);var connected=await general.ConnectAsync(ct);var lease=await general.AcquireAsync(connected.HostFence,ct);var start=connected.Observation.GetProperty("commit");
        var spec=new GeneralRunSpec(Guid.CreateVersion7().ToString("D"),start.GetProperty("revision").GetUInt64(),start.GetProperty("state_root").GetString()!,128,120,500,"1000000000",GeneralRunSpec.ExplicitConsent);
        Reject(()=>(spec with{MaxEnergyQ="01"}).Validate());Reject(()=>(spec with{Consent=""}).Validate());Reject(()=>(spec with{Ticks=129}).Validate());
        _stage="general-start";var job=await general.StartAsync(connected.HostFence,lease.FencingToken,spec,ct);Assert(job.StorageBackend=="memory","Memory不可冒称PG耐久");Assert((await general.LookupAsync(spec.SubmissionId,ct)).Id==job.Id,"原提交键只读对账");var originalDeadline=job.DeadlineS;
        foreach(var action in new[]{"pause","resume","pause"}){var op=new GeneralRunOperation(Guid.CreateVersion7().ToString("D"),job.Generation,action);job=await general.ControlAsync(job.Id,op,ct);var replay=await general.ControlAsync(job.Id,op,ct);Assert(replay.Generation==job.Generation&&job.DeadlineS==originalDeadline,"原操作幂等且不延预算");if(action=="pause"){var before=job.Done;await Task.Delay(550,ct);Assert((await general.GetAsync(job.Id,ct)).Done==before,"暂停确认后不推进");}}
        _stage="general-reconnect";await client.DisposeAsync();await using var resumed=new RynvekClient(origin);await resumed.LoginAsync(principal,(m,t)=>new(Sign("owner",m,t)),ct);general=new GeneralWorldClient(resumed,world);job=await general.GetAsync(job.Id,ct);Assert(job.Status=="paused"&&job.DeadlineS==originalDeadline,"重登录不续预算");job=await general.ControlAsync(job.Id,new(Guid.CreateVersion7().ToString("D"),job.Generation,"cancel"),ct);Assert(job.Status=="cancelled","明确取消");
        connected=await general.ConnectAsync(ct);lease=await general.AcquireAsync(connected.HostFence,ct);start=connected.Observation.GetProperty("commit");spec=spec with{SubmissionId=Guid.CreateVersion7().ToString("D"),ExpectedRevision=start.GetProperty("revision").GetUInt64(),ExpectedRoot=start.GetProperty("state_root").GetString()!,Ticks=3,IntervalMs=200};job=await general.StartAsync(connected.HostFence,lease.FencingToken,spec,ct);
        // 测试只用GET等待后台；SDK自身不含Tick或轮询循环。
        while(job.Status!="completed"){Assert(job.Status is not ("blocked" or "expired"),"后台拒绝不能冒称成功");await Task.Delay(30,ct);job=await general.GetAsync(job.Id,ct);}
        var obs=await general.ObserveAsync(ct);Assert(job.Done==3&&job.EnergyUsedQ>0&&obs.GetProperty("commit").GetProperty("state_root").GetString()==job.LastRoot,"原后台实付同根");Emit(new{done=true,world_id=world,tick=obs.GetProperty("commit").GetProperty("tick").GetUInt64(),root=job.LastRoot,paid=job.EnergyUsedQ,storage=job.StorageBackend});
    }
    private static async Task SelfTest()
    {
        using var fixture = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(AppContext.BaseDirectory,"csharp-wire-v1.json")));
        var f = fixture.RootElement; var id = PrincipalId.Parse("18446744073709551615"); Assert(Wire.Element(id).GetRawText() == "\"18446744073709551615\"", "u64 text exact");
        var boundaryLease = Wire.Read<ControlLease>(Wire.Element(new { holder=new { id=id.ToString(),kind=PrincipalKind.Human },faction=0,level=ControlLevel.Sovereign,fencing_token=ulong.MaxValue,issued_tick=ulong.MaxValue-100,expires_tick=ulong.MaxValue })); Assert(boundaryLease.FencingToken==ulong.MaxValue && boundaryLease.ExpiresTick==ulong.MaxValue,"u64 fence/tick exact");
        var raw = Wire.Read<RawVec2>(JsonDocument.Parse("[-9223372036854775808,9223372036854775807]").RootElement); Assert(raw.X == long.MinValue && raw.Y == long.MaxValue && Wire.Element(raw).GetRawText() == "[-9223372036854775808,9223372036854775807]", "Q32.32 i64 exact");
        Assert(Wire.ExactInteger(Wire.Element("79228162514264337593543950336")) == BigInteger.One << 96, "Decimal128 integer text exact");
        Assert(Convert.ToHexStringLower(Wire.AuthenticationMessage(id, Enumerable.Range(0,32).Select(x=>(byte)x).ToArray())) == f.GetProperty("auth_message_hex").GetString(), "Cross-language auth message");
        Reject(()=>PrincipalId.Parse("01")); Reject(()=>Wire.Read<PrincipalId>(Wire.Element(42))); Reject(()=>Wire.Read<RawVec2>(Wire.Element(new long[]{1,2,3})));
        var calls = new[] { ToolCall.QueryVisibleClusters(), ToolCall.SampleRegionalFields(long.MinValue,long.MaxValue), ToolCall.IssueStrategicIntent(UnitArchetype.Forager,OrderKind.HoldPosition,0.1234567890123456789012345678m,1m), ToolCall.ConfigureSignalBeacon(4,SignalChannel.B,50m,new(1m,-1m,0m,0.5m)), ToolCall.QueueVatProduction("2,3","bld.spawning_vat","",1), ToolCall.TriggerCrucibleBreeding(3,CrucibleAction.BreedEliteGenome,"unit.forager",0.01m) };
        Assert(calls.Select(x=>x.Name).Order().SequenceEqual(f.GetProperty("tool_names").EnumerateArray().Select(x=>x.GetString()!).Order()),"Exactly canonical six tools");
        Reject(()=>ToolCall.FromJson("raw_command", Wire.Element(new{}))); Reject(()=>ToolCall.FromJson("query_visible_clusters", Wire.Element(new{include_enemy=true,extra=true}))); Reject(()=>ToolCall.QueryVisibleClusters(true,[1,2,3])); Reject(()=>ToolCall.QueueVatProduction("4","unit.forager","",6));
        var handler = new RecordingHandler(); await using var client = new RynvekClient(new Uri("https://private.invalid"),handler);
        await client.LoginAsync(new(42), (_,_)=>ValueTask.FromResult(new byte[64])); await client.ListWorldsAsync();
        Assert(handler.Seen.Count==3 && handler.Seen[2].Auth?.StartsWith("Bearer ",StringComparison.Ordinal)==true && handler.Seen.All(r=>!r.Url.Contains(handler.Token,StringComparison.Ordinal)) && !client.ToString().Contains(handler.Token,StringComparison.Ordinal), "Capability only in authenticated header, no diagnostics");
        handler.Redirect=true; var refused=false; try { await client.GetWorldAsync(Guid.NewGuid()); } catch(RynvekRequestException e) when(e.Status==302){refused=true;} Assert(refused,"No redirect credential forwarding");
        await client.DisposeAsync(); Assert(!client.IsAuthenticated,"Dispose clears session");
        Reject(()=>new RynvekClient(new Uri("https://user:secret@invalid"))); Reject(()=>new RynvekClient(new Uri("http://remote.invalid")));
        handler.Redirect=false;
        var spec=new GeneralRunSpec(Guid.CreateVersion7().ToString("D"),ulong.MaxValue,new string('0',64),3,120,200,"1000000000000",GeneralRunSpec.ExplicitConsent);
        Assert(Wire.Element(spec).GetProperty("expected_revision").GetUInt64()==ulong.MaxValue,"C#耐久规格u64无截断");spec.Validate();Reject(()=>(spec with{MaxEnergyQ="01"}).Validate());Reject(()=>(spec with{Ticks=129}).Validate());Reject(()=>(spec with{Consent=""}).Validate());Reject(()=>new GeneralRunOperation(Guid.CreateVersion7().ToString("D"),0,"tick").Validate());
        Emit(new{self_test_groups=4,general_run_boundaries=true,exact_integers=true,schema_six_tools=true,transport_boundaries=true});
    }
    private sealed class RecordingHandler : HttpMessageHandler
    {
        public string Token { get; }=new('A',128); public bool Redirect; public List<(string Url,string? Auth)> Seen { get; }=[];
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken ct)
        {
            Seen.Add((request.RequestUri!.ToString(),request.Headers.Authorization?.ToString()));
            if(Redirect)return Task.FromResult(new HttpResponseMessage(HttpStatusCode.Redirect){Headers={Location=new Uri("https://not-followed.invalid")}});
            object value = Seen.Count switch {1=>new{principal_id="42",challenge=new string('0',64)},2=>new{principal_id="42",token=Token,expires_at_unix_s=ulong.MaxValue},_=>new{worlds=Array.Empty<object>(),limit=20,offset=0}};
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(value),Encoding.UTF8,"application/json")});
        }
    }
}
