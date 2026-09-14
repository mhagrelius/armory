using Armory.Blizzard;

namespace Armory.Client.Roster;

/// <summary>A colour as bytes, with no window type behind it so the table can be tested.</summary>
public readonly record struct Rgb(byte R, byte G, byte B);

/// <summary>
/// One class's ring: the style slug the GTK stylesheet keyed on, and Blizzard's
/// class colour. The colour is fixed game data, not a theme value, and never
/// swapped for the accent. Priest and Rogue fail contrast on a light card and
/// are darkened there rather than reassigned.
/// </summary>
public sealed record ClassRing(string Style, Rgb Dark, Rgb Light)
{
    public ClassRing(string style, Rgb colour)
        : this(style, colour, colour)
    {
    }
}

/// <summary>
/// The thirteen classes the game ships, by display name. Both the ring and the
/// crest are derived from the same display string and neither is checked by
/// the compiler; the test is what says they agree.
/// </summary>
public static class Classes
{
    /// <summary>A class Blizzard has not shipped yet, or a locale this build does not speak. No ring beats a wrong one.</summary>
    public static ClassRing Unknown { get; } = new("class-unknown", new Rgb(0x80, 0x80, 0x80));

    private static readonly Dictionary<string, ClassRing> Rings = new(StringComparer.Ordinal)
    {
        ["Death Knight"] = new("class-death-knight", new Rgb(0xC4, 0x1E, 0x3A)),
        ["Demon Hunter"] = new("class-demon-hunter", new Rgb(0xA3, 0x30, 0xC9)),
        ["Druid"] = new("class-druid", new Rgb(0xFF, 0x7C, 0x0A)),
        ["Evoker"] = new("class-evoker", new Rgb(0x33, 0x93, 0x7F)),
        ["Hunter"] = new("class-hunter", new Rgb(0xAA, 0xD3, 0x72)),
        ["Mage"] = new("class-mage", new Rgb(0x3F, 0xC7, 0xEB)),
        ["Monk"] = new("class-monk", new Rgb(0x00, 0xFF, 0x98)),
        ["Paladin"] = new("class-paladin", new Rgb(0xF4, 0x8C, 0xBA)),
        ["Priest"] = new("class-priest", new Rgb(0xFF, 0xFF, 0xFF), new Rgb(0x8A, 0x8A, 0x8A)),
        ["Rogue"] = new("class-rogue", new Rgb(0xFF, 0xF4, 0x68), new Rgb(0x8C, 0x82, 0x1A)),
        ["Shaman"] = new("class-shaman", new Rgb(0x00, 0x70, 0xDD)),
        ["Warlock"] = new("class-warlock", new Rgb(0x87, 0x88, 0xEE)),
        ["Warrior"] = new("class-warrior", new Rgb(0xC6, 0x9B, 0x6D)),
    };

    /// <summary>The ring for a class, or <see cref="Unknown"/>.</summary>
    public static ClassRing Ring(string className) => Rings.GetValueOrDefault(className, Unknown);

    /// <summary>The class crest on the render service, where there is no portrait to ring.</summary>
    public static string Crest(Region region, string className) => Media.ClassIcon(region, className);
}
