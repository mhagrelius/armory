using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Roster;

namespace Armory.Collections;

/// <summary>Which collection something belongs to.</summary>
[JsonConverter(typeof(JsonStringEnumConverter<Kind>))]
public enum Kind
{
    Mount,
    Pet,
    Toy,

    /// <summary>
    /// Housing decor. Owned in quantities in the game; Armory records owned
    /// or not, the same as the other three, because it tracks a collection
    /// rather than furnishes a house.
    /// </summary>
    Decor,
}

/// <summary>How coarse Blizzard's answer to "where is this from" is.</summary>
[JsonConverter(typeof(JsonStringEnumConverter<Source>))]
public enum Source
{
    Drop,
    Vendor,
    Quest,
    Achievement,
    Profession,
    Pvp,
    Promotion,

    /// <summary>The field was absent, which is common for anything old.</summary>
    Unknown,
}

/// <summary>
/// One mount, pet, toy or piece of decor, as the catalogue knows it. The JSON
/// shape is the Rust <c>Collectible</c>'s, field for field, because the
/// <c>collectible.json</c> column travels between machines and both
/// implementations read it.
/// </summary>
public sealed record Collectible
{
    [JsonPropertyName("kind")]
    public required Kind Kind { get; init; }

    [JsonPropertyName("id")]
    public required long Id { get; init; }

    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("source")]
    public Source Source { get; init; } = Source.Unknown;

    /// <summary>The sentence the in-game journal gives, markup stripped. Null from the web API, which has no such text.</summary>
    [JsonPropertyName("description")]
    public string? Description { get; init; }

    /// <summary>Flavour text: what the thing says about itself. Journals only.</summary>
    [JsonPropertyName("flavour")]
    public string? Flavour { get; init; }

    /// <summary>The texture the game draws for this, as a FileDataID. The key any icon service is looked up by.</summary>
    [JsonPropertyName("icon")]
    public long? Icon { get; init; }

    /// <summary>The creature display, for the 3D render Blizzard's media endpoint serves. Mounts and pets only.</summary>
    [JsonPropertyName("display")]
    public long? Display { get; init; }

    /// <summary>Which faction can use it at all, when only one can.</summary>
    [JsonPropertyName("faction")]
    public Faction? Faction { get; init; }

    /// <summary>The id Wowhead indexes this under: a mount by its spell, a pet by its creature, a toy by its item.</summary>
    [JsonPropertyName("link_id")]
    public required long LinkId { get; init; }

    /// <summary>
    /// Whether this can be caged and traded. Pets only, and the journal is the
    /// only source. Null means nobody has told us, which is not the same as
    /// false.
    /// </summary>
    [JsonPropertyName("tradeable")]
    public bool? Tradeable { get; init; }

    /// <summary>The options the column is written and read with.</summary>
    public static JsonSerializerOptions Json { get; } = new(JsonSerializerDefaults.General)
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    /// <summary>
    /// This, filled in from <paramref name="other"/> wherever this is silent.
    /// The addon has the source prose, the art and the faction lock; the web
    /// API has the name and the expansion. Whichever lands second must not
    /// flatten the other.
    /// </summary>
    public Collectible Merge(Collectible other) => this with
    {
        Name = Name.Length == 0 ? other.Name : Name,
        Source = Source == Source.Unknown ? other.Source : Source,
        Description = string.IsNullOrEmpty(Description) ? other.Description : Description,
        Flavour = string.IsNullOrEmpty(Flavour) ? other.Flavour : Flavour,
        Icon = Icon ?? other.Icon,
        Display = Display ?? other.Display,
        Faction = Faction ?? other.Faction,
        // The journal is the only source for this, so a web-API row landing
        // second must not write its null over an answer.
        Tradeable = Tradeable ?? other.Tradeable,
        // A link to the collection id is the fallback the index sets when it
        // has nothing better. A real one is never equal to it.
        LinkId = LinkId == Id && other.LinkId != other.Id ? other.LinkId : LinkId,
    };

    /// <summary>
    /// Two JSON halves of one collectible, merged. An arriving value that will
    /// not parse is taken as it is; a held one that will not is ignored.
    /// </summary>
    public static string MergeJson(string arriving, string held)
    {
        Collectible? merged;
        try
        {
            merged = JsonSerializer.Deserialize<Collectible>(arriving, Json);
        }
        catch (JsonException)
        {
            return arriving;
        }
        if (merged is null)
        {
            return arriving;
        }
        try
        {
            if (JsonSerializer.Deserialize<Collectible>(held, Json) is { } mine)
            {
                merged = merged.Merge(mine);
            }
        }
        catch (JsonException)
        {
            // What is held is unreadable; the arriving half stands alone.
        }
        return JsonSerializer.Serialize(merged, Json);
    }
}
