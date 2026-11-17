// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
using System.Collections.Concurrent;
using System.Net.WebSockets;
using System.Text.Json;
using System.Threading.Channels;
namespace Rynvek;

/// <summary>One real authenticated rynvek-arena/1 JSON WS, one reader and serialized writes. No local simulation/reconnect/retry.</summary>
public sealed class ArenaConnection : IAsyncDisposable
{
    private readonly RynvekClient _client;
    private readonly ClientWebSocket _socket = new();
    private readonly CancellationTokenSource _lifetime;
    private readonly SemaphoreSlim _send = new(1, 1);
    private readonly object _state = new();
    private readonly ConcurrentDictionary<ulong, Pending> _pending = new();
    private readonly Channel<ArenaNotice> _notices = Channel.CreateBounded<ArenaNotice>(new BoundedChannelOptions(128) { FullMode = BoundedChannelFullMode.DropOldest });
    private readonly TaskCompletionSource<Welcome> _welcome = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private TaskCompletionSource _changed = Pulse();
    private Task? _reader;
    private Exception? _terminal;
    private ulong _sequence;
    private ulong _maxFence;
    private int _disposed;
    private FactionObservation? _observation;
    private ControlLease? _lease;
    private Welcome? _info;
    private JsonElement[] _events = [];
    private sealed record Pending(string ExpectedType, TaskCompletionSource<JsonElement> Completion);
    private static TaskCompletionSource Pulse() => new(TaskCreationOptions.RunContinuationsAsynchronously);
    public WorldRun Run { get; }
    public Welcome? Welcome { get { lock (_state) return _info; } }
    public ControlLease? Lease { get { lock (_state) return _lease; } }
    public FactionObservation? Observation { get { lock (_state) return _observation; } }
    public IReadOnlyList<JsonElement> LatestEvents { get { lock (_state) return Array.AsReadOnly(_events); } }
    internal ArenaConnection(RynvekClient client, WorldRun run) { _client = client; Run = run; _lifetime = CancellationTokenSource.CreateLinkedTokenSource(client.Lifetime); }
    internal async Task ConnectAsync(CancellationToken ct)
    {
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, _lifetime.Token); linked.CancelAfter(TimeSpan.FromSeconds(10));
        var uri = new UriBuilder(_client.Origin) { Scheme = _client.Origin.Scheme == "https" ? "wss" : "ws", Path = $"/api/v1/worlds/{Run.WorldId:D}/stream", Query = "format=arena-json" }.Uri;
        await _socket.ConnectAsync(uri, _client.Http, linked.Token); _reader = ReceiveLoopAsync();
        await SendAsync(new { type = "hello", protocol = "rynvek-arena/1", principal_id = Run.PrincipalId.ToString(), kind = PrincipalKind.Human, faction = Run.Faction, capability_token = _client.Capability() }, linked.Token);
        await _welcome.Task.WaitAsync(linked.Token);
    }
    private async Task SendAsync(object value, CancellationToken ct)
    {
        await _send.WaitAsync(ct);
        try { CheckLive(); var frame = Wire.Element(value);
            if (frame.TryGetProperty("fencing_token", out var token)) { lock (_state) { if (_lease is null || _lease.FencingToken != token.GetUInt64()) throw new InvalidOperationException("Control fence changed before dispatch"); } }
            var bytes = JsonSerializer.SerializeToUtf8Bytes(frame, Wire.Json); if (bytes.Length > 65536) throw new ArgumentException("Outbound frame too large"); await _socket.SendAsync(bytes, WebSocketMessageType.Text, true, ct); }
        finally { _send.Release(); }
    }
    private void CheckLive()
    { lock (_state) { if (_terminal is not null) throw new InvalidOperationException("Arena connection is terminal; reconcile explicitly"); } _client.Capability(); _lifetime.Token.ThrowIfCancellationRequested(); }
    private void End(Exception reason)
    {
        lock (_state)
        {
            if (_terminal is not null) return; _terminal = reason; _lease = null; _observation = null; _info = null; _events = []; _changed.TrySetException(reason);
        }
        _welcome.TrySetException(reason); foreach (var p in _pending.Values) p.Completion.TrySetException(reason); _pending.Clear(); _notices.Writer.TryComplete(); _lifetime.Cancel(); _socket.Abort();
    }
    private async Task ReceiveLoopAsync()
    {
        var chunk = new byte[8192];
        try
        {
            while (!_lifetime.IsCancellationRequested)
            {
                using var bytes = new MemoryStream(); WebSocketReceiveResult part;
                do {
                    part = await _socket.ReceiveAsync(chunk, _lifetime.Token);
                    if (part.MessageType == WebSocketMessageType.Close) throw new RynvekProtocolException("Arena stream closed");
                    if (part.MessageType != WebSocketMessageType.Text || bytes.Length + part.Count > 1048576) throw new RynvekProtocolException("Invalid/oversized current Arena frame"); bytes.Write(chunk, 0, part.Count);
                } while (!part.EndOfMessage);
                JsonElement message; try { using var doc = JsonDocument.Parse(bytes.ToArray(), new JsonDocumentOptions { MaxDepth = 48 }); message = doc.RootElement.Clone(); } catch (JsonException) { throw new RynvekProtocolException("Malformed Arena JSON (redacted)"); }
                Handle(message);
            }
        }
        catch (Exception e) { End(e is RynvekProtocolException or RynvekRequestException ? e : new RynvekProtocolException("Arena transport stopped; pending outcomes not confirmed")); }
    }
    private void Handle(JsonElement m)
    {
        var type = m.GetProperty("type").GetString()!;
        switch (type)
        {
            case "welcome":
                var welcome = Wire.Read<Welcome>(m);
                if (welcome.Protocol != "rynvek-arena/1" || welcome.Faction != Run.Faction || welcome.Mode != Run.Mode || welcome.Tools.Select(t => t.Name).Distinct().Count() != 6 || welcome.Tools.Any(t => !ToolCall.Names.Contains(t.Name))) throw new RynvekProtocolException("Welcome protocol/seat/tool set mismatch");
                lock (_state) { if (_info is not null) throw new RynvekProtocolException("Duplicate welcome"); _info = welcome; } _welcome.TrySetResult(welcome); break;
            case "observation":
                var observation = Wire.Read<FactionObservation>(m.GetProperty("observation"));
                if (observation.Faction != Run.Faction || observation.Supply.Length != 2 || _info is null || observation.VisibilityWidth != (_info.Map.Width + 3) / 4 || observation.Visibility.Count != (long)observation.VisibilityWidth * ((_info.Map.Height + 3) / 4) || observation.Units.Any(u => u.Modulators is not null && u.Modulators.Length != 4)) throw new RynvekProtocolException("Observation faction/shape mismatch");
                lock (_state)
                {
                    if (_info is null || (_observation is not null && observation.Tick < _observation.Tick)) throw new RynvekProtocolException("Observation ordering mismatch");
                    _observation = observation; _events = m.GetProperty("events").EnumerateArray().Select(e => e.Clone()).ToArray(); var changed = _changed; _changed = Pulse(); changed.TrySetResult();
                } break;
            case "lease_revoked":
                if (m.GetProperty("faction").GetUInt32() != Run.Faction) throw new RynvekProtocolException("Lease faction mismatch"); lock (_state) _lease = null; break;
            case "lease": case "ack": case "tool_result":
                Pending? pending = null;
                if (m.TryGetProperty("seq", out var sequence) && sequence.ValueKind == JsonValueKind.Number)
                {
                    if (!_pending.TryGetValue(sequence.GetUInt64(), out pending)) break;
                    if (type != pending.ExpectedType) throw new RynvekProtocolException("Receipt kind mismatch");
                }
                else if (type != "lease") throw new RynvekProtocolException("Receipt sequence missing");
                // Current protocol permits an unsolicited lease grant (seq:null), never an unsolicited command/tool receipt.
                if (type == "lease" && (!m.TryGetProperty("error", out var error) || error.ValueKind == JsonValueKind.Null))
                {
                    ControlLease? lease = m.TryGetProperty("lease", out var l) && l.ValueKind != JsonValueKind.Null ? Wire.Read<ControlLease>(l) : null;
                    if (lease is not null && (lease.Holder.Id != Run.PrincipalId.ToString() || lease.Holder.Kind != PrincipalKind.Human || lease.Faction != Run.Faction || lease.ExpiresTick <= lease.IssuedTick)) throw new RynvekProtocolException("Lease owner/faction/range mismatch");
                    lock (_state) { if (lease is not null) { if (lease.FencingToken < _maxFence) throw new RynvekProtocolException("Lease fence rollback"); _maxFence = lease.FencingToken; } _lease = lease; }
                }
                pending?.Completion.TrySetResult(m); break;
            case "error": throw new RynvekRequestException(0, Wire.Read<ErrorBody>(m.GetProperty("error")).Code);
            default: throw new RynvekProtocolException("Unknown current Arena message");
        }
        _notices.Writer.TryWrite(new(type, Observation?.Tick ?? 0));
    }
    private async Task<JsonElement> RequestAsync(string expected, Func<ulong, object> build, CancellationToken ct)
    {
        CheckLive(); ulong seq; lock (_state) { seq = checked(++_sequence); if (_pending.Count >= 32) throw new InvalidOperationException("Too many pending Arena requests"); }
        var p = new Pending(expected, new(TaskCreationOptions.RunContinuationsAsynchronously)); if (!_pending.TryAdd(seq, p)) throw new InvalidOperationException("Sequence collision");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, _lifetime.Token); linked.CancelAfter(TimeSpan.FromSeconds(8)); var started = false;
        try { var value = build(seq); linked.Token.ThrowIfCancellationRequested(); started = true; await SendAsync(value, linked.Token); return await p.Completion.Task.WaitAsync(linked.Token); }
        catch (OperationCanceledException) when (started) { var unknown = new RynvekOutcomeUnknownException(); End(unknown); throw unknown; }
        finally { _pending.TryRemove(seq, out _); }
    }
    private ulong Fence(ControlLevel minimum)
    {
        CheckLive(); lock (_state)
        {
            if (_lease is null || _lease.Level < minimum || _lease.Faction != Run.Faction || (_observation?.Tick ?? _info?.Tick ?? 0) >= _lease.ExpiresTick) throw new InvalidOperationException("No current sufficient control lease"); return _lease.FencingToken;
        }
    }
    public async Task<ControlLease> AcquireLeaseAsync(ControlLevel level, CancellationToken ct = default)
    {
        if (!Enum.IsDefined(level)) throw new ArgumentException("Unknown control level"); var m = await RequestAsync("lease", seq => new { type = "acquire_lease", seq, level }, ct);
        if (m.TryGetProperty("error", out var error) && error.ValueKind != JsonValueKind.Null) throw new RynvekRequestException(0, Wire.Read<ErrorBody>(error).Code); return Lease ?? throw new RynvekProtocolException("Lease grant missing");
    }
    public async Task ReleaseLeaseAsync(CancellationToken ct = default)
    {
        var fence = Fence(ControlLevel.Spectate); var m = await RequestAsync("lease", seq => new { type = "release_lease", seq, fencing_token = fence }, ct);
        if (m.TryGetProperty("error", out var error) && error.ValueKind != JsonValueKind.Null) throw new RynvekRequestException(0, Wire.Read<ErrorBody>(error).Code);
        if (Lease is not null) throw new RynvekProtocolException("Release receipt retained a lease");
    }
    public async Task<ToolReceipt> CallToolAsync(ToolCall call, CancellationToken ct = default)
    {
        var need = call.Name is "query_visible_clusters" or "sample_regional_fields" ? ControlLevel.Perceive : call.Name is "issue_strategic_intent" or "configure_signal_beacon" ? ControlLevel.Modulate : ControlLevel.Produce;
        var fence = Fence(need); var m = await RequestAsync("tool_result", seq => new { type = "tool_call", seq, fencing_token = fence, tool = call.Name, arguments = call.Arguments }, ct);
        var receipt = Wire.Read<ToolReceipt>(m.GetProperty("response")); if (receipt.Tool != call.Name) { End(new RynvekProtocolException("Tool receipt mismatch")); throw new RynvekProtocolException("Tool receipt mismatch"); } return receipt;
    }
    public async Task<CommandReceipt> SendCommandAsync(RtsCommand command, CancellationToken ct = default)
    {
        var need = command.Value.GetProperty("type").GetString() == "surrender" ? ControlLevel.Sovereign : ControlLevel.Modulate; var fence = Fence(need);
        var m = await RequestAsync("ack", seq => new { type = "command", seq, fencing_token = fence, command = command.Value }, ct);
        return new(m.GetProperty("tick").GetUInt64(), m.TryGetProperty("ack", out var a) ? a.Clone() : null, m.TryGetProperty("error", out var e) ? Wire.Read<ErrorBody>(e) : null);
    }
    public async Task<FactionObservation> WaitForObservationAsync(ulong minimumTick = 0, CancellationToken ct = default)
    {
        while (true) { Task wait; lock (_state) { if (_terminal is not null) throw new InvalidOperationException("Disconnected observations cleared"); if (_observation is not null && _observation.Tick >= minimumTick) return _observation; wait = _changed.Task; } await wait.WaitAsync(ct); }
    }
    /// <summary>Bounded observational notices, oldest may be dropped for slow consumers. Not an audit log or receipt source.</summary>
    public IAsyncEnumerable<ArenaNotice> ReadNoticesAsync(CancellationToken ct = default) => _notices.Reader.ReadAllAsync(ct);
    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        End(new RynvekProtocolException("Arena disposed; credentials/observations cleared")); if (_reader is not null) await _reader; _socket.Dispose(); _lifetime.Dispose(); _client.Remove(this);
    }
}
