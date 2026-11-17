// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
using System.Collections.Concurrent;
using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
namespace Rynvek;

/// <summary>Caller-owned Ed25519 signer; SDK never loads, logs, or persists a signing private key.</summary>
public delegate ValueTask<byte[]> SignMessage(ReadOnlyMemory<byte> message, CancellationToken cancellationToken);
public sealed class RynvekClient : IAsyncDisposable
{
    public Uri Origin { get; }
    public PrincipalId? PrincipalId { get; private set; }
    public bool IsAuthenticated => _token is not null && !_lifetime.IsCancellationRequested;
    private string? _token;
    private ulong _expires;
    private readonly HttpClient _http;
    private readonly CancellationTokenSource _lifetime = new();
    private readonly SemaphoreSlim _authentication = new(1, 1);
    private readonly ConcurrentDictionary<ArenaConnection, byte> _arenas = new();
    internal HttpMessageInvoker Http => _http;
    internal CancellationToken Lifetime => _lifetime.Token;
    public RynvekClient(Uri origin) : this(origin, new SocketsHttpHandler { AllowAutoRedirect = false, UseCookies = false, Credentials = null, AutomaticDecompression = DecompressionMethods.None }) { }
    internal RynvekClient(Uri origin, HttpMessageHandler handler)
    {
        if (!origin.IsAbsoluteUri || (origin.Scheme != "http" && origin.Scheme != "https") || origin.UserInfo.Length != 0 || origin.AbsolutePath != "/" || origin.Query.Length != 0 || origin.Fragment.Length != 0) throw new ArgumentException("Expected credential-free HTTP(S) origin");
        if (origin.Scheme == "http" && !origin.IsLoopback) throw new ArgumentException("Remote authenticated transport requires HTTPS");
        Origin = origin; _http = new HttpClient(handler, true) { Timeout = Timeout.InfiniteTimeSpan };
    }
    internal string Capability()
    {
        ObjectDisposedException.ThrowIf(_lifetime.IsCancellationRequested, this);
        if (_token is null || _expires <= (ulong)DateTimeOffset.UtcNow.ToUnixTimeSeconds()) throw new InvalidOperationException("No current authenticated capability"); return _token;
    }
    internal void Remove(ArenaConnection arena) => _arenas.TryRemove(arena, out _);
    public async Task LoginAsync(PrincipalId principal, SignMessage sign, CancellationToken ct = default) => await AuthenticateAsync(new { principal_id = principal }, principal, sign, ct);
    public async Task RegisterAsync(string handle, PrincipalKind kind, ReadOnlyMemory<byte> publicKey, SignMessage sign, CancellationToken ct = default)
    {
        if (IsAuthenticated) throw new InvalidOperationException("Already authenticated");
        if (kind is not (PrincipalKind.Human or PrincipalKind.Agent) || publicKey.Length != 32 || string.IsNullOrWhiteSpace(handle) || handle.Length > 64) throw new ArgumentException("Invalid explicit registration");
        var prefix = Encoding.UTF8.GetBytes("rynvek-register/1\0" + handle + "\0"); var message = new byte[prefix.Length + 32]; prefix.CopyTo(message, 0); publicKey.Span.CopyTo(message.AsSpan(prefix.Length));
        var signature = await SignatureAsync(sign, message, ct);
        await AuthenticateAsync(new { handle, kind = kind.ToString().ToLowerInvariant(), ed25519_pubkey = Convert.ToHexStringLower(publicKey.Span), registration_signature = Convert.ToHexStringLower(signature) }, null, sign, ct);
    }
    private static async Task<byte[]> SignatureAsync(SignMessage sign, byte[] message, CancellationToken ct)
    { var bytes = await sign(message, ct); ct.ThrowIfCancellationRequested(); if (bytes.Length != 64) throw new ArgumentException("Ed25519 signer must return 64 bytes"); return bytes; }
    private async Task AuthenticateAsync(object first, PrincipalId? expected, SignMessage sign, CancellationToken ct)
    {
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, _lifetime.Token); linked.CancelAfter(TimeSpan.FromSeconds(15));
        await _authentication.WaitAsync(linked.Token);
        try
        {
            if (IsAuthenticated) throw new InvalidOperationException("Already authenticated; use a new client for a different identity");
            var challenge = await RequestAsync("/api/v1/auth/session", HttpMethod.Post, first, false, linked.Token);
            var pid = Wire.Read<PrincipalId>(challenge.GetProperty("principal_id")); if (expected is not null && pid != expected) throw new RynvekProtocolException("Challenge principal mismatch");
            var encoded = challenge.GetProperty("challenge").GetString()!;
            if (encoded.Length != 64 || !encoded.All(char.IsAsciiHexDigitLower)) throw new RynvekProtocolException("Invalid challenge bytes");
            var signed = await SignatureAsync(sign, Wire.AuthenticationMessage(pid, Convert.FromHexString(encoded)), linked.Token);
            var reply = await RequestAsync("/api/v1/auth/session", HttpMethod.Post, new { principal_id = pid, challenge = encoded, signature = Convert.ToHexStringLower(signed) }, false, linked.Token);
            if (Wire.Read<PrincipalId>(reply.GetProperty("principal_id")) != pid) throw new RynvekProtocolException("Session principal mismatch");
            var token = reply.GetProperty("token").GetString()!; var expires = reply.GetProperty("expires_at_unix_s").GetUInt64();
            if (token.Length != 128 || !token.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_') || expires <= (ulong)DateTimeOffset.UtcNow.ToUnixTimeSeconds()) throw new RynvekProtocolException("Invalid current capability shape/expiry");
            linked.Token.ThrowIfCancellationRequested(); PrincipalId = pid; _expires = expires; _token = token;
        }
        finally { _authentication.Release(); }
    }
    internal async Task<JsonElement> RequestAsync(string path, HttpMethod method, object? body, bool authenticated, CancellationToken ct)
    {
        if (!path.StartsWith("/api/v1/", StringComparison.Ordinal) || path.StartsWith("//", StringComparison.Ordinal)) throw new ArgumentException("Invalid Rynvek path");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, _lifetime.Token); linked.CancelAfter(TimeSpan.FromSeconds(10));
        using var request = new HttpRequestMessage(method, new Uri(Origin, path));
        request.Headers.CacheControl = new CacheControlHeaderValue { NoStore = true }; request.Headers.Accept.ParseAdd("application/json");
        if (authenticated) request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", Capability());
        if (body is not null) request.Content = new StringContent(JsonSerializer.Serialize(body, Wire.Json), Encoding.UTF8, "application/json");
        using var response = await _http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, linked.Token);
        if ((int)response.StatusCode is >= 300 and < 400) throw new RynvekRequestException((int)response.StatusCode);
        if (response.Content.Headers.ContentLength > 1048576) throw new RynvekProtocolException("Response exceeds current SDK bound");
        using var stream = await response.Content.ReadAsStreamAsync(linked.Token); using var buffer = new MemoryStream(); var bytes = new byte[8192];
        while (true) { var n = await stream.ReadAsync(bytes, linked.Token); if (n == 0) break; if (buffer.Length + n > 1048576) throw new RynvekProtocolException("Response exceeds current SDK bound"); buffer.Write(bytes, 0, n); }
        JsonElement value;
        try { using var document = JsonDocument.Parse(buffer.ToArray(), new JsonDocumentOptions { MaxDepth = 48 }); value = document.RootElement.Clone(); }
        catch (JsonException) { throw new RynvekProtocolException("Non-JSON Rynvek response (body redacted)"); }
        if (!response.IsSuccessStatusCode) { uint? code = value.TryGetProperty("code", out var c) && c.TryGetUInt32(out var v) ? v : null; throw new RynvekRequestException((int)response.StatusCode, code); }
        return value;
    }
    public async Task<WorldPage> ListWorldsAsync(uint limit = 20, uint offset = 0, CancellationToken ct = default)
    { if (limit is 0 or > 100) throw new ArgumentOutOfRangeException(nameof(limit)); return Wire.Read<WorldPage>(await RequestAsync($"/api/v1/worlds?limit={limit}&offset={offset}", HttpMethod.Get, null, true, ct)); }
    public async Task<WorldInfo> GetWorldAsync(Guid id, CancellationToken ct = default)
    { var value = Wire.Read<WorldInfo>(await RequestAsync($"/api/v1/worlds/{id:D}", HttpMethod.Get, null, true, ct)); if (value.WorldId != id) throw new RynvekProtocolException("World ID mismatch"); return value; }
    public async Task<WorldInfo> CreatePrivatePveAsync(string title, ulong mapSeed, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(title) || title.Length > 128) throw new ArgumentException("Explicit nonempty world title required");
        var world = Wire.Read<WorldInfo>(await RequestAsync("/api/v1/worlds", HttpMethod.Post, new { title, template = "arena_pve", visibility = "Private", map_seed = mapSeed }, true, ct));
        if (world.OwnerPrincipalId != PrincipalId || world.Visibility != "Private" || world.GameMode != "PvE_Survival") throw new RynvekProtocolException("Created world identity/profile mismatch"); return world;
    }
    public async Task<ArenaConnection> ConnectPrivatePveAsync(Guid worldId, CancellationToken ct = default)
    {
        var world = await GetWorldAsync(worldId, ct);
        if (world.OwnerPrincipalId != PrincipalId || world.Visibility != "Private" || world.GameMode != "PvE_Survival") throw new InvalidOperationException("Only caller-owned private PvE supported by this entry");
        var run = Wire.Read<WorldRun>(await RequestAsync($"/api/v1/worlds/{worldId:D}/arena", HttpMethod.Post, new { }, true, ct));
        if (run.WorldId != worldId || run.PrincipalId != PrincipalId || run.Faction != 0 || run.Mode != MatchMode.PvE_Survival || run.Ephemeral || run.MatchId == Guid.Empty) throw new RynvekProtocolException("Run receipt identity/seat mismatch");
        var arena = new ArenaConnection(this, run); _arenas.TryAdd(arena, 0);
        try { await arena.ConnectAsync(ct); return arena; } catch { await arena.DisposeAsync(); throw; }
    }
    public async ValueTask DisposeAsync()
    {
        if (_lifetime.IsCancellationRequested) return; _lifetime.Cancel(); _token = null; PrincipalId = null;
        foreach (var arena in _arenas.Keys) await arena.DisposeAsync(); _http.Dispose(); _lifetime.Dispose();
    }
    public override string ToString() => "RynvekClient(session credentials redacted)";
}
