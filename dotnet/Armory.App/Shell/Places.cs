using Armory.Collections;

namespace Armory.App.Shell;

/// <summary>One destination in the navigation pane.</summary>
public sealed record Place(string Name, string Title, string Glyph, Kind? Collection = null);

/// <summary>
/// The pane's destinations, in the order the GTK window lists them: two on
/// their own, four under Collection, three under Account. Settings sits in
/// the pane footer. The glyphs are Segoe Fluent Icons.
/// </summary>
public static class Places
{
    public static IReadOnlyList<Place> First { get; } =
    [
        new("run", "Run", ""),
        new("chronicle", "Chronicle", ""),
    ];

    public static IReadOnlyList<Place> Collection { get; } =
    [
        new("zones", "Zones", ""),
        new("mounts", "Mounts", "", Kind.Mount),
        new("pets", "Pets", "", Kind.Pet),
        new("toys", "Toys", "", Kind.Toy),
        new("decor", "Decor", "", Kind.Decor),
    ];

    public static IReadOnlyList<Place> Account { get; } =
    [
        new("roster", "Roster", ""),
        new("reputations", "Reputations", ""),
        new("market", "Market", ""),
    ];

    public static IEnumerable<Place> All => First.Concat(Collection).Concat(Account);

    public static Place? Named(string name) => All.FirstOrDefault(place => place.Name == name);

    public static string PlaceOf(Kind kind) => kind switch
    {
        Kind.Mount => "mounts",
        Kind.Pet => "pets",
        Kind.Toy => "toys",
        _ => "decor",
    };
}
