using System.Globalization;
using Armory.Chronicle;
using Armory.Roster;

namespace Armory.Tally;

/// <summary>
/// What a tally counts. A closed set: a row with a kind this version does not
/// know is a newer addon writing an older application's folder, and is
/// better reported than silently filed under something plausible.
/// </summary>
public enum Counting
{
    /// <summary>Times a recipe has been made.</summary>
    Recipe,

    /// <summary>Evenings a person has been in the party.</summary>
    Companion,

    /// <summary>Attempts at an encounter, won or lost.</summary>
    Attempt,

    /// <summary>Attempts that ended with the boss on the floor.</summary>
    Victory,

    /// <summary>Seconds spent in a zone, keyed by <c>UiMapID</c> because two zones share the name Nagrand.</summary>
    Zone,

    /// <summary>Deaths, by what did it.</summary>
    Killer,

    /// <summary>Yards travelled, keyed by how.</summary>
    Distance,

    /// <summary>Flights taken, keyed by where from.</summary>
    Flight,

    /// <summary>Delves finished, keyed by tier.</summary>
    Delve,

    /// <summary>Quests taken from or handed to a particular NPC.</summary>
    Questgiver,

    /// <summary>Named rares put down, by name. Kept apart from <see cref="Victory"/>: a world rare raises no encounter event at all.</summary>
    Rare,
}

public static class CountingExtensions
{
    public static IReadOnlyList<Counting> All { get; } =
    [
        Counting.Recipe, Counting.Companion, Counting.Attempt, Counting.Victory, Counting.Zone, Counting.Killer,
        Counting.Distance, Counting.Flight, Counting.Delve, Counting.Questgiver, Counting.Rare,
    ];

    /// <summary>The token the addon writes.</summary>
    public static string Token(this Counting kind) => kind switch
    {
        Counting.Recipe => "recipe",
        Counting.Companion => "companion",
        Counting.Attempt => "attempt",
        Counting.Victory => "victory",
        Counting.Zone => "zone",
        Counting.Killer => "killer",
        Counting.Distance => "distance",
        Counting.Flight => "flight",
        Counting.Delve => "delve",
        Counting.Questgiver => "questgiver",
        Counting.Rare => "rare",
        _ => "",
    };

    public static Counting? FromToken(string token) =>
        All.Cast<Counting?>().FirstOrDefault(kind => kind!.Value.Token() == token);

    /// <summary>What a group of these is called on a page.</summary>
    public static string Title(this Counting kind) => kind switch
    {
        Counting.Recipe => "At the workbench",
        Counting.Companion => "Alongside",
        Counting.Attempt => "Fought most",
        Counting.Victory => "Defeated most",
        Counting.Zone => "Where the time went",
        Counting.Killer => "Killed by",
        Counting.Distance => "Distance travelled",
        Counting.Flight => "Flights taken",
        Counting.Delve => "Delves finished",
        Counting.Questgiver => "Sent you out most",
        Counting.Rare => "Rares hunted down",
        _ => "",
    };

    /// <summary>The line under that title.</summary>
    public static string Description(this Counting kind) => kind switch
    {
        Counting.Recipe => "Everything this character has ever made",
        Counting.Companion => "Who has been in the party, and how often",
        Counting.Attempt => "Bosses pulled, won or lost",
        Counting.Victory => "Bosses that went down",
        Counting.Zone => "Hours spent, by zone",
        Counting.Killer => "What has killed this character, and how often",
        Counting.Distance => "Ground covered since the addon was installed",
        Counting.Flight => "Where the flight paths were taken from",
        Counting.Delve => "How many, and at what tier",
        Counting.Questgiver => "Who keeps giving this character work",
        Counting.Rare => "Named rares, and how many times each",
        _ => "",
    };
}

/// <summary>
/// One counter: how many times this character did this particular thing.
/// </summary>
/// <remarks>
/// Counters Armory keeps because nothing else does. They only exist because
/// the addon has been adding one to them since it was installed, which
/// decides everything about how they are stored: one table, not one per
/// kind; merged by taking the larger count, because a reinstalled addon
/// starts at one and there is nowhere to get a year of evenings back from;
/// never purged, because none of it came through the API.
/// </remarks>
public sealed record Tally
{
    public required Counting Kind { get; init; }

    /// <summary>What is being counted, as the addon keys it: a spell id, a zone name, a person's name.</summary>
    public required string Key { get; init; }

    /// <summary>The same thing said the way a person says it. A key must not change when Blizzard renames something.</summary>
    public string Label { get; init; } = "";

    public long Count { get; init; }
}

/// <summary>Every counter, per character.</summary>
public sealed class Tallies : Dictionary<CharacterKey, List<Tally>>
{
}

public static class Counters
{
    /// <summary>
    /// The shortest name worth matching a drop against. Three letters matches
    /// half the game by accident: Blizzard has encounters called Ick and rares
    /// called Zul.
    /// </summary>
    private const int NameFloor = 5;

    /// <summary>One character's counters of one kind, biggest first.</summary>
    public static List<Tally> Of(IEnumerable<Tally> tallies, Counting kind) =>
        tallies.Where(tally => tally.Kind == kind)
            .OrderByDescending(tally => tally.Count)
            .ThenBy(tally => tally.Label, StringComparer.Ordinal)
            .ToList();

    /// <summary>
    /// How many times this account has fought whatever drops a thing.
    /// </summary>
    /// <remarks>
    /// <b>A count of attempts and never a drop rate.</b> The join is on the
    /// sentence the in-game journal gives a collectible against the encounter
    /// and rare names the addon counted: substring, and the longest match
    /// wins so a boss whose name contains another's is not credited to the
    /// shorter one. Returns what was fought as well as the count, because a
    /// number without a referent is not an answer.
    /// </remarks>
    public static (string Fought, long Tries)? AttemptsAt(string? description, IEnumerable<Tally> tallies)
    {
        if (description is null)
        {
            return null;
        }
        var sentence = description.ToLowerInvariant();
        var best = tallies
            .Where(tally => tally.Kind is Counting.Attempt or Counting.Rare)
            .Where(tally => tally.Label.Length >= NameFloor)
            .Where(tally => sentence.Contains(tally.Label.ToLowerInvariant(), StringComparison.Ordinal))
            .OrderByDescending(tally => tally.Label.Length)
            .ThenByDescending(tally => tally.Count)
            .FirstOrDefault();
        return best is null ? null : (best.Label, best.Count);
    }

    /// <summary>
    /// The same question asked of a whole account. A mount is account-wide,
    /// so every character's pulls at the boss that drops it are pulls at that
    /// mount, summed rather than the largest taken.
    /// </summary>
    public static List<Tally> AccountAttempts(Tallies tallies)
    {
        var totals = new Dictionary<(Counting, string), Tally>();
        foreach (var tally in tallies.Values.SelectMany(list => list))
        {
            if (tally.Kind is not (Counting.Attempt or Counting.Rare))
            {
                continue;
            }
            var key = (tally.Kind, tally.Label);
            totals[key] = totals.TryGetValue(key, out var held) ? held with { Count = held.Count + tally.Count } : tally;
        }
        return totals.Values.ToList();
    }

    /// <summary>
    /// A duration in hours and minutes, for the zone tallies. Seconds below a
    /// minute, because a ten-second fight is short, not absent; hours alone
    /// past a day, because "17 hours 3 minutes" is a spreadsheet.
    /// </summary>
    public static string Spent(long seconds)
    {
        if (seconds < 60)
        {
            return Prose.Plural(seconds, "second", "seconds");
        }
        var minutes = seconds / 60;
        if (minutes < 60)
        {
            return Prose.Plural(minutes, "minute", "minutes");
        }
        var hours = minutes / 60;
        if (hours >= 24)
        {
            return Prose.Plural(hours, "hour", "hours");
        }
        var rest = minutes % 60;
        return rest == 0
            ? Prose.Plural(hours, "hour", "hours")
            : string.Create(CultureInfo.InvariantCulture, $"{hours} hr {rest} min");
    }

    /// <summary>A distance in yards, said the way a person would. Miles above a mile.</summary>
    public static string Far(long yards)
    {
        const long mile = 1_760;
        if (yards < mile)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{yards} yards");
        }
        var miles = yards / (double)mile;
        return miles < 10.0
            ? string.Create(CultureInfo.InvariantCulture, $"{miles:0.0} miles")
            : string.Create(CultureInfo.InvariantCulture, $"{Math.Round(miles)} miles");
    }
}
