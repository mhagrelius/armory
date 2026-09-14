using System.Text.Json.Serialization;

namespace Armory.Zones;

/// <summary>
/// One dungeon or raid, from Blizzard's Adventure Guide: a paragraph per
/// instance written as the premise rather than the summary, and each
/// encounter's loot table, which is how an item is joined to a boss and a
/// boss to a place.
/// </summary>
public sealed record Instance
{
    [JsonPropertyName("id")]
    public long Id { get; init; }

    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    /// <summary>The instance's own <c>UiMapID</c>, the same key a zone and a session join on.</summary>
    [JsonPropertyName("map")]
    public long? Map { get; init; }

    /// <summary>Blizzard's own account of the place.</summary>
    [JsonPropertyName("description")]
    public string Description { get; init; } = "";

    /// <summary><c>DUNGEON</c> or <c>RAID</c>, as the API spells it.</summary>
    [JsonPropertyName("expansion")]
    public string? Expansion { get; init; }

    /// <summary>Encounter ids, in the order the guide lists them. Duplicates are real and kept.</summary>
    [JsonPropertyName("encounters")]
    public List<long> Encounters { get; init; } = [];

    public bool Equals(Instance? other) =>
        other is not null && Id == other.Id && Name == other.Name && Map == other.Map && Description == other.Description && Expansion == other.Expansion && Encounters.SequenceEqual(other.Encounters);

    public override int GetHashCode() => HashCode.Combine(Id, Name);
}

/// <summary>One boss, and what it drops. Possibility, never probability.</summary>
public sealed record Encounter
{
    [JsonPropertyName("id")]
    public long Id { get; init; }

    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("description")]
    public string Description { get; init; } = "";

    [JsonPropertyName("loot")]
    public List<long> Loot { get; init; } = [];

    public bool Equals(Encounter? other) =>
        other is not null && Id == other.Id && Name == other.Name && Description == other.Description && Loot.SequenceEqual(other.Loot);

    public override int GetHashCode() => HashCode.Combine(Id, Name);
}

/// <summary>Everything the guide knows, as the application holds it.</summary>
public sealed class Guide
{
    public Dictionary<long, Instance> Instances { get; } = [];

    public Dictionary<long, Encounter> Encounters { get; } = [];

    /// <summary>The instance a map contains. An instance sits on its own map, so this answers "what is this place" for somebody standing inside it.</summary>
    public Instance? At(long map) => Instances.Values.FirstOrDefault(instance => instance.Map == map);

    /// <summary>Which boss drops an item, if the guide says so. The first that lists it.</summary>
    public (Instance Instance, Encounter Encounter)? Drops(long item)
    {
        foreach (var instance in Instances.Values)
        {
            foreach (var id in instance.Encounters)
            {
                if (Encounters.TryGetValue(id, out var encounter) && encounter.Loot.Contains(item))
                {
                    return (instance, encounter);
                }
            }
        }
        return null;
    }

    /// <summary>Every item the guide lists as dropping anywhere. What the auction snapshot is filtered against.</summary>
    public HashSet<long> Loot() => Encounters.Values.SelectMany(encounter => encounter.Loot).ToHashSet();
}
