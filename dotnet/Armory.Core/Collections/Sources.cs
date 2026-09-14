namespace Armory.Collections;

public static class Sources
{
    /// <summary>Blizzard's one-word <c>source.type</c> code from the web API.</summary>
    public static Source FromType(string code) => code switch
    {
        "DROP" => Source.Drop,
        "VENDOR" => Source.Vendor,
        "QUEST" => Source.Quest,
        "ACHIEVEMENT" => Source.Achievement,
        "PROFESSION" or "TRADESKILL" => Source.Profession,
        "PVP" => Source.Pvp,
        "PROMOTION" or "TCG" or "STORE" => Source.Promotion,
        _ => Source.Unknown,
    };

    /// <summary>
    /// Read a source out of the journals' sentence. The in-game journals give
    /// "Drop: Attumen the Huntsman, Karazhan" where the web API gives one
    /// word or nothing. Only the first clause is classified, matching on the
    /// leading word: "Vendor: sold near the Drop Zone" is a vendor, and a
    /// substring search would call it a drop.
    /// </summary>
    public static Source FromText(string text)
    {
        var end = text.IndexOfAny([':', '\n']);
        var head = (end < 0 ? text : text[..end]).Trim().ToLowerInvariant();
        return head switch
        {
            "drop" or "world drop" => Source.Drop,
            // The Trading Post is a vendor that rotates. Its stock comes back,
            // so it is not the dead end that a trading-card mount is.
            "vendor" or "trading post" => Source.Vendor,
            "quest" => Source.Quest,
            "achievement" => Source.Achievement,
            "profession" => Source.Profession,
            "pvp" or "arena" or "battleground" => Source.Pvp,
            "promotion" or "trading card game" or "collector's edition" or "in-game shop" or "blizzard store" or "recruit-a-friend" => Source.Promotion,
            _ => Source.Unknown,
        };
    }

    public static string Label(this Source source) => source switch
    {
        Source.Drop => "Drop",
        Source.Vendor => "Vendor",
        Source.Quest => "Quest",
        Source.Achievement => "Achievement",
        Source.Profession => "Profession",
        Source.Pvp => "PvP",
        Source.Promotion => "Promotion",
        _ => "Unknown",
    };

    /// <summary>Whether a run could plausibly obtain this again. A promotion is gone for good; everything else, and the unknown, might come back.</summary>
    public static bool IsRepeatable(this Source source) => source != Source.Promotion;
}
