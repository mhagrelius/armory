using Armory.Blizzard;
using Armory.Roster;

namespace Armory.Collections;

/// <summary>Where a person can go to read about a collectible, and the id spaces that decide it.</summary>
public static class Links
{
    public static string Label(this Kind kind) => kind switch
    {
        Kind.Mount => "Mounts",
        Kind.Pet => "Pets",
        Kind.Toy => "Toys",
        _ => "Decor",
    };

    public static string Singular(this Kind kind) => kind switch
    {
        Kind.Mount => "mount",
        Kind.Pet => "pet",
        Kind.Toy => "toy",
        _ => "decor",
    };

    public static IReadOnlyList<Kind> AllKinds { get; } = [Kind.Mount, Kind.Pet, Kind.Toy, Kind.Decor];

    /// <summary>The Wowhead path for this kind. Linking is not scraping: a link a person clicks is a person visiting a website.</summary>
    internal static string WowheadPath(this Kind kind) => kind switch
    {
        Kind.Mount => "spell",
        Kind.Pet => "npc",
        _ => "item",
    };

    /// <summary>
    /// Whether the link id is still the collection id standing in for an
    /// item. Only meaningful for the kinds addressed by item: a mount's link
    /// legitimately equals its id when the spell is absent.
    /// </summary>
    public static bool ItemIdIsGuessed(this Collectible entry) =>
        entry.Kind is Kind.Toy or Kind.Decor && entry.LinkId == entry.Id;

    /// <summary>
    /// The Wowhead page, or null when the id space is a guess rather than a
    /// fact. A chair sent somebody to a belt: a plausible wrong page reads as
    /// correct until you look at it.
    /// </summary>
    public static string? WowheadUrl(this Collectible entry) =>
        entry.ItemIdIsGuessed() ? null : $"https://www.wowhead.com/{entry.Kind.WowheadPath()}={entry.LinkId}";

    /// <summary>The Warcraft Wiki, which is community-run and reads better for lore.</summary>
    public static string WikiUrl(this Collectible entry) =>
        $"https://warcraft.wiki.gg/wiki/Special:Search?search={Api.Encode(entry.Name)}";

    /// <summary>The item this is, for anything addressed by item; the collection id otherwise.</summary>
    public static long ItemId(this Collectible entry) => entry.Kind is Kind.Toy or Kind.Decor ? entry.LinkId : entry.Id;

    /// <summary>The item this is, only where that is known. What an icon lookup should use.</summary>
    public static long? KnownItemId(this Collectible entry) => entry.ItemIdIsGuessed() ? null : entry.ItemId();

    /// <summary>Whether this account can obtain it at all. A faction-locked mount is not missing from a collection that could never have held it.</summary>
    public static bool ObtainableBy(this Collectible entry, Faction faction) =>
        entry.Faction is { } only && only != faction ? false : entry.Source.IsRepeatable();
}
