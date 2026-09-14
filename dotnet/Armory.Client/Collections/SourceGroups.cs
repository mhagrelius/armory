using Armory.Collections;

namespace Armory.Client.Collections;

/// <summary>
/// What the collection page knows about a source that the catalogue does not:
/// the order the page reads in, what to call a group, and whether chance is
/// involved at all. The pure half of <c>ui/collection_page.rs</c>'s source
/// handling, kept out of the widget so it can be tested.
/// </summary>
public static class SourceGroups
{
    /// <summary>
    /// The order the page reads in, and the order the rail lists.
    /// Explicit rather than alphabetical. <c>Source.Label</c> puts Achievement
    /// above Drop, and a collection whose largest group is drops should not
    /// open on its third largest.
    /// </summary>
    public static IReadOnlyList<Source> Groups { get; } =
    [
        Source.Drop,
        Source.Vendor,
        Source.Achievement,
        Source.Quest,
        Source.Profession,
        Source.Pvp,
        Source.Promotion,
        Source.Unknown,
    ];

    /// <summary>Where a source sits in the reading order; after everything for one the page has no group for.</summary>
    public static int Rank(Source source)
    {
        for (var index = 0; index < Groups.Count; index++)
        {
            if (Groups[index] == source)
            {
                return index;
            }
        }
        return Groups.Count;
    }

    /// <summary>A source group's heading in the main column.</summary>
    public static string Heading(Source source) => source switch
    {
        Source.Drop => "DROPS",
        Source.Vendor => "VENDOR",
        Source.Achievement => "ACHIEVEMENTS",
        Source.Quest => "QUESTS",
        Source.Profession => "PROFESSIONS",
        Source.Pvp => "PVP",
        Source.Promotion => "PROMOTIONS",
        _ => "UNRECORDED",
    };

    /// <summary>
    /// A source in the rail's list. <c>Source.Label</c> says "Unknown", which
    /// reads as a property of the mount. It is a gap in Blizzard's data, and
    /// the caveat at the foot of the rail is where that is explained.
    /// </summary>
    public static string RailLabel(Source source) => source == Source.Unknown ? "Unrecorded" : source.Label();

    /// <summary>
    /// How much chance stands between the account and one of these.
    /// </summary>
    /// <remarks>
    /// Armory has no drop rates of its own. What the page does know is whether
    /// chance is involved at all: a vendor mount is gold and a quest reward is
    /// time, where a raid drop is a coin flipped until it lands. Ranking on
    /// that is the strongest true statement available. Null is a source with
    /// chance in it, which sorts last and gets no gold.
    /// </remarks>
    public static byte? Certainty(Source source) => source switch
    {
        Source.Vendor => 0,
        Source.Quest => 1,
        Source.Achievement => 2,
        Source.Profession => 3,
        Source.Pvp => 4,
        _ => null,
    };

    /// <summary>What can honestly be said, in a line, about how one of these is earned.</summary>
    public static string? NoChance(Source source) => source switch
    {
        Source.Vendor => "SOLD, NOT DROPPED",
        Source.Quest => "A QUEST, NOT A ROLL",
        Source.Achievement => "EARNED, NOT ROLLED",
        Source.Profession => "MADE, NOT DROPPED",
        Source.Pvp => "WON, NOT ROLLED",
        _ => null,
    };
}
