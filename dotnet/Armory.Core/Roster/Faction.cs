using System.Text.Json.Serialization;

namespace Armory.Roster;

/// <summary>Which side a character, or a collectible, belongs to.</summary>
[JsonConverter(typeof(JsonStringEnumConverter<Faction>))]
public enum Faction
{
    Alliance,
    Horde,

    /// <summary>Pandaren before they choose, and anything Blizzard adds later.</summary>
    Neutral,
}

public static class FactionExtensions
{
    /// <summary>Blizzard's <c>faction.type</c> code, as the profile API spells it.</summary>
    public static Faction FactionFromType(string code) => code switch
    {
        "ALLIANCE" => Faction.Alliance,
        "HORDE" => Faction.Horde,
        _ => Faction.Neutral,
    };

    /// <summary>The word the store keeps and the page shows.</summary>
    public static string Label(this Faction faction) => faction switch
    {
        Faction.Alliance => "Alliance",
        Faction.Horde => "Horde",
        _ => "Neutral",
    };
}
