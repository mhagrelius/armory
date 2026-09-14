using System.Globalization;
using Armory.Roster;

namespace Armory.Blizzard;

/// <summary>Which picture of a character is wanted.</summary>
public enum Portrait
{
    /// <summary>The square head-and-shoulders, for a list row.</summary>
    Avatar,

    /// <summary>The waist-up crop, for a card.</summary>
    Inset,

    /// <summary>The full-body render, for a header.</summary>
    Main,
}

/// <summary>
/// Where Blizzard's art lives, and which of it can be had for nothing.
/// Constructed URLs on the render service cost no request and no quota;
/// everything else needs a call that answers with a URL rather than bytes.
/// Nothing here fetches an image.
/// </summary>
public static class Media
{
    private const SourceId DataSource = SourceId.BlizzardGameData;
    private const SourceId ProfileSource = SourceId.BlizzardProfile;

    /// <summary>The path an item's media hangs off. Public because the id is read back out of a cached URL.</summary>
    public const string ItemMedia = "/data/wow/media/item/";

    public const string AchievementMedia = "/data/wow/media/achievement/";

    private static string RenderHost(Region region) => $"https://render.worldofwarcraft.com/{region.Code()}";

    /// <summary>A creature's portrait by its display id. The one that lets a whole collection be illustrated without a request; zoom is the only size served.</summary>
    public static string CreatureRender(Region region, long displayId) =>
        string.Create(CultureInfo.InvariantCulture, $"{RenderHost(region)}/npcs/zoom/creature-display-{displayId}.jpg");

    /// <summary>An icon by the game's own texture name. 56px is the only size served.</summary>
    public static string Icon(Region region, string texture) => $"{RenderHost(region)}/icons/56/{texture}.jpg";

    /// <summary>The class crest: the class name lowercased with its spaces removed.</summary>
    public static string ClassIcon(Region region, string className) => Icon(region, "classicon_" + Squash(className));

    /// <summary>The faction crest. Neutral is not a side and has no crest.</summary>
    public static string? FactionIcon(Region region, Faction faction) => faction switch
    {
        Faction.Alliance => Icon(region, "ui_allianceicon"),
        Faction.Horde => Icon(region, "ui_hordeicon"),
        _ => null,
    };

    private static string Squash(string text) => string.Concat(text.Where(char.IsAsciiLetterOrDigit).Select(char.ToLowerInvariant));

    /// <summary>An item's icon. The only way to illustrate a toy.</summary>
    public static Request Item(Region region, long itemId) =>
        Request.Get(DataSource, Api.Url(region, Namespace.Static, string.Create(CultureInfo.InvariantCulture, $"{ItemMedia}{itemId}")));

    public static Request Achievement(Region region, long id) =>
        Request.Get(DataSource, Api.Url(region, Namespace.Static, string.Create(CultureInfo.InvariantCulture, $"{AchievementMedia}{id}")));

    /// <summary>
    /// What a media URL was built for: the inverse of <see cref="Item"/> and
    /// <see cref="Achievement"/>. A cached media body names no item, so the id
    /// is read off the front of the key.
    /// </summary>
    public static long? MediaId(string url, string path)
    {
        var at = url.IndexOf(path, StringComparison.Ordinal);
        if (at < 0)
        {
            return null;
        }
        var rest = url[(at + path.Length)..];
        var end = rest.IndexOfAny(['?', '&', '/']);
        var digits = end < 0 ? rest : rest[..end];
        return long.TryParse(digits, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id) ? id : null;
    }

    /// <summary>A creature display's render, asked for rather than constructed. The endpoint is the contract, the URL shape an observation.</summary>
    public static Request CreatureDisplay(Region region, long displayId) =>
        Request.Get(DataSource, Api.Url(region, Namespace.Static, string.Create(CultureInfo.InvariantCulture, $"/data/wow/media/creature-display/{displayId}")));

    /// <summary>A character's portraits. Profile namespace, and the only art here that needs a signed-in account.</summary>
    public static Request Character(Region region, CharacterKey key) =>
        Request.Get(ProfileSource, Api.Url(region, Namespace.Profile, $"/profile/wow/character/{key.RealmSlug}/{key.Name}/character-media"));

    /// <summary>The asset key Blizzard files a portrait under. <c>main-raw</c> has an alpha channel; <c>main</c> is composited onto a scene.</summary>
    internal static string Key(this Portrait portrait) => portrait switch
    {
        Portrait.Avatar => "avatar",
        Portrait.Inset => "inset",
        _ => "main-raw",
    };

    /// <summary>Read a URL out of a media response, by key rather than position. An entry with no art is an answer, not a fault.</summary>
    public static Outcome<string> ParseAsset(byte[] body, string key)
    {
        var parsed = Outcomes.ParseJson<string>(DataSource, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        foreach (var asset in parsed.Value.At("assets").Items())
        {
            if (asset.At("key").Str() == key && asset.At("value").Str() is { } url)
            {
                return new Outcome<string>.Found(url);
            }
        }
        return new Outcome<string>.Empty();
    }

    public static Outcome<string> ParseIcon(byte[] body) => ParseAsset(body, "icon");

    public static Outcome<string> ParsePortrait(byte[] body, Portrait want) => ParseAsset(body, want.Key());
}
