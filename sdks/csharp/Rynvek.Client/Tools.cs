// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
using System.Globalization;
using System.Text.Json;
namespace Rynvek;
public enum UnitArchetype { All, Forager, Striker, Carapace, Spitter, Weaver, Leviathan }
public enum SignalChannel { A, B }
public enum CrucibleAction { ResearchTech, BreedEliteGenome }
public readonly record struct ModulatorVector(decimal Da, decimal Serotonin, decimal Ne, decimal Ach);
public sealed class ToolCall
{
    public string Name { get; }
    public JsonElement Arguments { get; }
    private ToolCall(string name, JsonElement args) { Name = name; Arguments = args.Clone(); }
    private static readonly Dictionary<string, JsonElement> Schemas = LoadSchemas();
    public static IReadOnlyList<string> Names { get; } = Array.AsReadOnly(Schemas.Keys.Order().ToArray());
    private static Dictionary<string, JsonElement> LoadSchemas()
    {
        using var stream = typeof(ToolCall).Assembly.GetManifestResourceStream("Rynvek.AgentTools.v1.json")!;
        using var doc = JsonDocument.Parse(stream); var result = doc.RootElement.EnumerateArray().ToDictionary(x => x.GetProperty("name").GetString()!, x => x.GetProperty("parameters").Clone());
        if (result.Count != 6) throw new RynvekProtocolException("Expected current six-tool schema"); return result;
    }
    public static ToolCall FromJson(string name, JsonElement arguments)
    {
        if (!Schemas.TryGetValue(name, out var schema)) throw new ArgumentException("Unknown Rynvek tool; no seventh tool exists"); Validate(arguments, schema); return new(name, arguments);
    }
    private static void Validate(JsonElement v, JsonElement s)
    {
        bool bad = s.GetProperty("type").GetString() switch {
            "object" => v.ValueKind != JsonValueKind.Object,
            "array" => v.ValueKind != JsonValueKind.Array,
            "boolean" => v.ValueKind is not (JsonValueKind.True or JsonValueKind.False),
            "integer" => v.ValueKind != JsonValueKind.Number || !v.TryGetInt64(out _),
            "number" => v.ValueKind != JsonValueKind.Number || !v.TryGetDecimal(out _),
            "string" => v.ValueKind != JsonValueKind.String,
            _ => true
        };
        if (bad) throw new ArgumentException("Tool argument type/range mismatch");
        if (s.TryGetProperty("enum", out var choices) && !choices.EnumerateArray().Any(c => JsonElement.DeepEquals(c, v))) throw new ArgumentException("Tool enum mismatch");
        if (v.ValueKind == JsonValueKind.Object)
        {
            var props = s.GetProperty("properties"); var seen = new HashSet<string>();
            foreach (var p in v.EnumerateObject()) { if (!seen.Add(p.Name) || !props.TryGetProperty(p.Name, out var child)) throw new ArgumentException("Unknown/duplicate tool field"); Validate(p.Value, child); }
            if (s.TryGetProperty("required", out var required) && required.EnumerateArray().Any(k => !seen.Contains(k.GetString()!))) throw new ArgumentException("Missing tool argument");
        }
        if (v.ValueKind == JsonValueKind.Array)
        {
            var n = v.GetArrayLength(); if ((s.TryGetProperty("minItems", out var min) && n < min.GetInt32()) || (s.TryGetProperty("maxItems", out var max) && n > max.GetInt32())) throw new ArgumentException("Tool array length mismatch");
            foreach (var x in v.EnumerateArray()) Validate(x, s.GetProperty("items"));
        }
        if (v.ValueKind == JsonValueKind.Number && ((s.TryGetProperty("minimum", out var lo) && v.GetDecimal() < lo.GetDecimal()) || (s.TryGetProperty("maximum", out var hi) && v.GetDecimal() > hi.GetDecimal()))) throw new ArgumentException("Tool range mismatch");
    }
    private static ToolCall Build(string name, object arguments) => FromJson(name, Wire.Element(arguments));
    public static ToolCall QueryVisibleClusters(bool includeEnemy = true, long[]? regionChunk = null)
    {
        var args = new Dictionary<string, object> { ["include_enemy"] = includeEnemy }; if (regionChunk is not null) args["region_chunk"] = regionChunk; return Build("query_visible_clusters", args);
    }
    public static ToolCall SampleRegionalFields(long chunkX, long chunkY) => Build("sample_regional_fields", new { chunk_x = chunkX, chunk_y = chunkY });
    public static ToolCall IssueStrategicIntent(UnitArchetype units, OrderKind order, decimal targetX, decimal targetY)
    {
        if (!Enum.IsDefined(units) || !Enum.IsDefined(order)) throw new ArgumentException("Unknown strategic enum");
        var archetype = units == UnitArchetype.All ? "all" : "unit." + units.ToString().ToLowerInvariant();
        return Build("issue_strategic_intent", new { unit_archetype = archetype, order_kind = order.ToString(), target_world_x = targetX, target_world_y = targetY });
    }
    public static ToolCall ConfigureSignalBeacon(uint beacon, SignalChannel channel, decimal strength, ModulatorVector modulators)
    {
        if (!Enum.IsDefined(channel)) throw new ArgumentException("Unknown signal channel");
        return Build("configure_signal_beacon", new { beacon_entity_id = beacon.ToString(CultureInfo.InvariantCulture), signal_channel = channel == SignalChannel.A ? "field.signal_a" : "field.signal_b", signal_strength = strength, neuromodulators = new[] { modulators.Da, modulators.Serotonin, modulators.Ne, modulators.Ach } });
    }
    public static ToolCall QueueVatProduction(string vatId, string archetype, string genomeSlug, byte count) => Build("queue_vat_production", new { vat_entity_id = vatId, unit_archetype = archetype, genome_slug = genomeSlug, count });
    public static ToolCall TriggerCrucibleBreeding(uint crucible, CrucibleAction action, string target, decimal? mutationRate = null)
    {
        if (!Enum.IsDefined(action)) throw new ArgumentException("Unknown crucible action");
        var args = new Dictionary<string, object> { ["crucible_entity_id"] = crucible.ToString(CultureInfo.InvariantCulture), ["action"] = action == CrucibleAction.ResearchTech ? "research_tech" : "breed_elite_genome", ["tech_or_archetype_id"] = target };
        if (mutationRate is not null) args["mutation_rate"] = mutationRate.Value; return Build("trigger_crucible_breeding", args);
    }
}
