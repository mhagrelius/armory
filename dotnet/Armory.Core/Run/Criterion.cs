using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Provenance;

namespace Armory.Run;

/// <summary>
/// What a criterion is actually counting. Only the types below are claimed,
/// because a wrong mapping is worse than a missing one: a missing mapping
/// sends a goal to attestation, and a wrong one draws a confident progress
/// bar over a number that means something else. The list grows as types are
/// confirmed against real data, never by inference.
/// </summary>
public enum CriterionType
{
    /// <summary>Type 27. Asset is a quest id; the single most valuable mapping in the table.</summary>
    Quest,

    /// <summary>Type 0. Asset is a creature id. Kept distinct from Unknown because the statistics endpoint sometimes carries a counter.</summary>
    Creature,

    /// <summary>Type 8. Asset is another achievement id, so the criterion recurses.</summary>
    Achievement,

    /// <summary>Type 46. Asset is a faction id, with the Warband caveat.</summary>
    Reputation,

    /// <summary>Backed by a per-character statistic.</summary>
    Statistic,

    /// <summary>A dungeon or raid encounter.</summary>
    Encounter,

    /// <summary>The catalogue had no entry, or the type is one we do not claim to understand. Not a failure; a boundary.</summary>
    Unknown,
}

/// <summary>A criterion's kind and the thing it points at. The Rust's payload enum, as a JSON <c>{"Quest": 5000}</c> or <c>"Unknown"</c>.</summary>
[JsonConverter(typeof(CriterionKindConverter))]
public readonly record struct CriterionKind(CriterionType Type, long Asset)
{
    public static CriterionKind Unknown => new(CriterionType.Unknown, 0);

    public static CriterionKind Quest(long id) => new(CriterionType.Quest, id);

    public static CriterionKind Creature(long id) => new(CriterionType.Creature, id);

    public static CriterionKind Achievement(long id) => new(CriterionType.Achievement, id);

    public static CriterionKind Reputation(long id) => new(CriterionType.Reputation, id);

    public static CriterionKind Statistic(long id) => new(CriterionType.Statistic, id);

    public static CriterionKind Encounter(long id) => new(CriterionType.Encounter, id);

    /// <summary>Read a kind from the client database's <c>(Type, Asset)</c> pair.</summary>
    public static CriterionKind FromCatalogue(long criteriaType, long asset) => criteriaType switch
    {
        0 => Creature(asset),
        8 => Achievement(asset),
        27 => Quest(asset),
        46 => Reputation(asset),
        _ => Unknown,
    };

    /// <summary>Whether a character's own data can answer this criterion.</summary>
    public bool IsObservable => Type is CriterionType.Quest or CriterionType.Statistic or CriterionType.Encounter or CriterionType.Reputation or CriterionType.Achievement;
}

public sealed class CriterionKindConverter : JsonConverter<CriterionKind>
{
    public override CriterionKind Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            return reader.GetString() == "Unknown" ? CriterionKind.Unknown : throw new JsonException("not a criterion kind");
        }
        if (reader.TokenType != JsonTokenType.StartObject)
        {
            throw new JsonException("a criterion kind is a name or an object");
        }
        reader.Read();
        var name = reader.GetString();
        reader.Read();
        var asset = reader.GetInt64();
        reader.Read();
        return name switch
        {
            "Quest" => CriterionKind.Quest(asset),
            "Creature" => CriterionKind.Creature(asset),
            "Achievement" => CriterionKind.Achievement(asset),
            "Reputation" => CriterionKind.Reputation(asset),
            "Statistic" => CriterionKind.Statistic(asset),
            "Encounter" => CriterionKind.Encounter(asset),
            _ => throw new JsonException($"not a criterion kind: {name}"),
        };
    }

    public override void Write(Utf8JsonWriter writer, CriterionKind value, JsonSerializerOptions options)
    {
        if (value.Type == CriterionType.Unknown)
        {
            writer.WriteStringValue("Unknown");
            return;
        }
        writer.WriteStartObject();
        writer.WriteNumber(value.Type.ToString(), value.Asset);
        writer.WriteEndObject();
    }
}

/// <summary>One node of an achievement's criteria tree.</summary>
public sealed record Criterion
{
    [JsonPropertyName("id")]
    public long Id { get; init; }

    [JsonPropertyName("kind")]
    public CriterionKind Kind { get; init; } = CriterionKind.Unknown;

    /// <summary>How many are needed. Zero means the criterion is a flag rather than a counter, and one of it is enough.</summary>
    [JsonPropertyName("required")]
    public long Required { get; init; }

    [JsonPropertyName("children")]
    public List<Criterion> Children { get; init; } = [];

    public static Criterion Leaf(long id, CriterionKind kind, long required) =>
        new() { Id = id, Kind = kind, Required = required };

    /// <summary>How many of this node must be satisfied, treating a flag as one.</summary>
    internal long Threshold => Math.Max(Required, 1);

    public bool Equals(Criterion? other) =>
        other is not null && Id == other.Id && Kind == other.Kind && Required == other.Required && Children.SequenceEqual(other.Children);

    public override int GetHashCode() => HashCode.Combine(Id, Kind, Required, Children.Count);
}

/// <summary>
/// The per-character data a criterion can be measured against. Every field
/// is genuinely per character; that is the entire selection criterion,
/// because account-wide is the problem being worked around.
/// </summary>
public sealed record PrimaryData
{
    /// <summary>From the completed-quests endpoint, or the addon's completed quest list.</summary>
    public HashSet<long> Quests { get; init; } = [];

    /// <summary>From the statistics endpoint.</summary>
    public Dictionary<long, double> Statistics { get; init; } = [];

    /// <summary>From the dungeon and raid encounter endpoints.</summary>
    public HashSet<long> Encounters { get; init; } = [];

    /// <summary>Standing value per faction id.</summary>
    public Dictionary<long, long> Reputations { get; init; } = [];

    /// <summary>Faction ids whose standing exceeds anything this character could have earned during the run.</summary>
    public HashSet<long> InheritedReputations { get; init; } = [];

    /// <summary>What this character has personally been observed earning, per faction. From the addon and nowhere else.</summary>
    public Dictionary<long, EarnedReputation> EarnedReputations { get; init; } = [];

    /// <summary>Achievement ids the run has done. Not the account's, which has almost everything.</summary>
    public HashSet<long> AchievementsDone { get; init; } = [];
}
