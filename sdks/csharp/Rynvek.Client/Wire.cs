// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
using System.Buffers.Binary;
using System.Globalization;
using System.Numerics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
namespace Rynvek;

public sealed class RynvekProtocolException(string message) : Exception(message);
public sealed class RynvekRequestException(int status, uint? code = null) : Exception($"Rynvek request rejected (HTTP {status}, code {code?.ToString("x4", CultureInfo.InvariantCulture) ?? "unknown"}); no success confirmed")
{ public int Status { get; } = status; public uint? Code { get; } = code; }
public sealed class RynvekOutcomeUnknownException() : Exception("Dispatch/receipt interrupted; authoritative outcome not confirmed. Connection stopped; reconcile before retrying.");

[JsonConverter(typeof(PrincipalIdConverter))]
public readonly record struct PrincipalId(ulong Value)
{
    public static PrincipalId Parse(string text)
    {
        if (!Wire.CanonicalInteger(text, false) || !ulong.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var value) || value == 0) throw new RynvekProtocolException("Invalid principal u64 string");
        return new(value);
    }
    public override string ToString() => Value.ToString(CultureInfo.InvariantCulture);
}
public sealed class PrincipalIdConverter : JsonConverter<PrincipalId>
{
    public override PrincipalId Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o) => r.TokenType == JsonTokenType.String ? PrincipalId.Parse(r.GetString()!) : throw new JsonException("Principal must be decimal string");
    public override void Write(Utf8JsonWriter w, PrincipalId v, JsonSerializerOptions o) { if (v.Value == 0) throw new JsonException("Principal zero"); w.WriteStringValue(v.ToString()); }
}
[JsonConverter(typeof(RawVec2Converter))]
public readonly record struct RawVec2(long X, long Y)
{
    public static RawVec2 FromRaw(long x, long y) => new(x, y);
}
public sealed class RawVec2Converter : JsonConverter<RawVec2>
{
    public override RawVec2 Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o)
    {
        if (r.TokenType != JsonTokenType.StartArray || !r.Read()) throw new JsonException(); var x = r.GetInt64();
        if (!r.Read()) throw new JsonException(); var y = r.GetInt64();
        if (!r.Read() || r.TokenType != JsonTokenType.EndArray) throw new JsonException(); return new(x, y);
    }
    public override void Write(Utf8JsonWriter w, RawVec2 v, JsonSerializerOptions o) { w.WriteStartArray(); w.WriteNumberValue(v.X); w.WriteNumberValue(v.Y); w.WriteEndArray(); }
}
public static class Wire
{
    public static JsonSerializerOptions Json { get; } = Options();
    private static JsonSerializerOptions Options()
    {
        var o = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower, RespectRequiredConstructorParameters = true, MaxDepth = 48 };
        o.Converters.Add(new JsonStringEnumConverter(allowIntegerValues: false)); o.MakeReadOnly(true); return o;
    }
    public static T Read<T>(JsonElement value)
    {
        try { return value.Deserialize<T>(Json) ?? throw new JsonException(); }
        catch (Exception e) when (e is JsonException or FormatException or OverflowException) { throw new RynvekProtocolException("Invalid current Rynvek wire shape/range"); }
    }
    public static JsonElement Element<T>(T value) => JsonSerializer.SerializeToElement(value, Json);
    internal static bool CanonicalInteger(string s, bool signed) => s.Length > 0 && (s == "0" || (signed && s[0] == '-' ? s.Length > 1 && s[1] is >= '1' and <= '9' && s[1..].All(char.IsAsciiDigit) : s[0] is >= '1' and <= '9' && s.All(char.IsAsciiDigit)));
    /// <summary>For APIs exposing Decimal128 as decimal text: never route through double/decimal.</summary>
    public static BigInteger ExactInteger(JsonElement v)
    {
        var s = v.ValueKind == JsonValueKind.String ? v.GetString()! : v.GetRawText();
        if (!CanonicalInteger(s, true)) throw new RynvekProtocolException("Expected exact canonical integer");
        return BigInteger.Parse(s, CultureInfo.InvariantCulture);
    }
    public static byte[] AuthenticationMessage(PrincipalId id, ReadOnlySpan<byte> challenge)
    {
        if (id.Value == 0 || challenge.Length != 32) throw new ArgumentException("Invalid challenge identity/size");
        var prefix = Encoding.UTF8.GetBytes("rynvek-auth/1\0"); var result = new byte[prefix.Length + 40]; prefix.CopyTo(result, 0); challenge.CopyTo(result.AsSpan(prefix.Length)); BinaryPrimitives.WriteUInt64LittleEndian(result.AsSpan(prefix.Length + 32), id.Value); return result;
    }
}
public enum ControlLevel { Spectate, Perceive, Modulate, Produce, Sovereign }
public enum PrincipalKind { Human, Script, Agent }
public enum MatchMode { PvP_Ranked, PvE_Survival }
public enum OrderKind { Move, AttackMove, Harvest, HoldPosition, Patrol }
public sealed record ErrorBody(uint Code, string Name, string Message) { public override string ToString() => $"Rynvek error 0x{Code:x4}"; }
public sealed record ResourceAmounts(long Biomass, long Flux, long Neurogel);
public sealed record ResourceLedger(ResourceAmounts Balance, ResourceAmounts Capacity, long BiomassHarvestedTotal);
public sealed record UnitView(uint Id, byte Faction, string Kind, RawVec2 Position, long RadiusMilli, long HpMilli, long MaxHpMilli, long ShieldMilli, OrderKind? Order, short[]? Modulators);
public sealed record BreedingView(string Unit, uint RemainingTicks);
public sealed record BeaconConfig(string Channel, short StrengthQ8, short[] Modulators);
public sealed record BuildingView(uint Id, byte Faction, string Kind, uint[] Origin, long HpMilli, long MaxHpMilli, bool Complete, bool Powered, uint BuildProgress, string[] Queue, string? Research, BeaconConfig? Beacon, uint? ResearchRemainingTicks = null, BreedingView? Breeding = null);
public sealed record BuildingGhost(uint EntityId, byte Faction, string Kind, uint[] Origin, ulong LastSeenTick);
public sealed record NeuralActivityView(ulong SampleTick, uint SpikeWindowSubsteps, ulong SpikingNeurons, ulong ContinuousNeurons, ulong SpikesInWindow, ushort ContinuousAbsMeanQ8);
public sealed record FactionObservation(ulong Tick, byte Faction, NeuralActivityView Neural, ResourceLedger Ledger, uint[] Supply, string[] Techs, string[] Genomes, UnitView[] Units, BuildingView[] Buildings, BuildingGhost[] Ghosts, IReadOnlyList<byte> Visibility, uint VisibilityWidth, long ScoreMilli, uint? PveWave, JsonElement? Outcome);
public sealed record MapInfo(string Size, uint Width, uint Height, ulong Seed, string TilesHash, string TerrainB64, uint[][] SpawnTiles);
public sealed record ToolSchema(string Name, string Description, JsonElement Parameters);
public sealed record Welcome(string Protocol, uint Faction, MatchMode Mode, ulong Tick, MapInfo Map, ToolSchema[] Tools);
public sealed record Principal(string Id, PrincipalKind Kind);
public sealed record ControlLease(Principal Holder, uint Faction, ControlLevel Level, ulong FencingToken, ulong IssuedTick, ulong ExpiresTick);
public sealed record ToolReceipt(string Tool, ulong Tick, JsonElement? Result = null, ErrorBody? Error = null) { public bool Accepted => Error is null; }
public sealed record CommandReceipt(ulong Tick, JsonElement? Ack, ErrorBody? Error) { public bool Accepted => Error is null && Ack is not null; }
public sealed record WorldInfo(Guid WorldId, PrincipalId OrgId, PrincipalId OwnerPrincipalId, string Title, string WorldProfile, string GameMode, string ExecutionProfile, string Visibility, Guid? ParentWorldId, ulong? ForkedAtRevision, ulong LatestTick, ulong LatestRevision, string LatestStateRoot);
public sealed record WorldPage(WorldInfo[] Worlds, uint Limit, uint Offset);
public sealed record WorldRun(Guid WorldId, Guid MatchId, PrincipalId PrincipalId, uint Faction, MatchMode Mode, string MatchState, ulong Tick, bool Recovered, bool Ephemeral);
public sealed record ArenaNotice(string Type, ulong Tick);

/// <summary>Current command subset, with exact Q32.32 i64 coordinates. No arbitrary command/advance escape hatch.</summary>
public sealed class RtsCommand
{
    internal JsonElement Value { get; }
    private RtsCommand(object value) => Value = Wire.Element(value);
    public static RtsCommand IssueOrder(IEnumerable<uint> ids, OrderKind order, RawVec2 target)
    {
        if (!Enum.IsDefined(order)) throw new ArgumentException("Unknown order"); var units = ids.Distinct().ToArray(); if (units.Length is 0 or > 1024) throw new ArgumentException("Explicit unit selection required");
        return new(new { type = "issue_order", units = new { kind = "ids", value = units }, order, target });
    }
    public static RtsCommand Surrender() => new(new { type = "surrender" });
}
