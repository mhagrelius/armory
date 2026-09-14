using System.Globalization;
using Armory.Chronicle;
using Armory.Tally;

namespace Armory.Client.Shell;

/// <summary>One segment of the ledger bar: its share of everything that moved, and whether it is the book's first (drawn at full strength).</summary>
public readonly record struct Segment(double Share, bool First);

/// <summary>What the chronicle's cards say, worked out away from any widget so it can be tested. The pure half of the GTK chronicle, run and character pages.</summary>
public static class Cards
{
    /// <summary>How many days the momentum strip covers.</summary>
    public const int Days = 14;

    /// <summary>
    /// A card's meta line: when it started, how long, and what it was.
    /// "20:14 — 3h 41m · 12 quests · 1 death". The instance is last because
    /// on an evening that had one it is the answer to "what was this", and on
    /// an evening that did not there is nothing to say.
    /// </summary>
    /// <param name="offset">The offset to show the clock in. The evening's own by default, which is what the tests pin.</param>
    public static string MetaLine(Digest digest, TimeSpan? offset = null)
    {
        var started = offset is { } chosen ? digest.StartedAt.ToOffset(chosen) : digest.StartedAt;
        var minutes = Math.Max((long)digest.Duration.TotalMinutes, 0);
        var parts = new List<string> { string.Create(CultureInfo.InvariantCulture, $"{started:HH:mm} — {minutes / 60}h {minutes % 60}m") };
        if (digest.Quests.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Quests.Count, "quest", "quests"));
        }
        if (digest.Deaths.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Deaths.Count, "death", "deaths"));
        }
        if (digest.Instances.Count > 0)
        {
            parts.Add(digest.Instances[0].Name);
        }
        return string.Join(" · ", parts);
    }

    /// <summary>The one-line count under "What happened", falling back to the headline when there is nothing to count.</summary>
    public static string Tally(Digest digest)
    {
        var parts = new List<string>();
        if (digest.Quests.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Quests.Count, "quest", "quests"));
        }
        if (digest.Felled.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Felled.Count, "boss", "bosses"));
        }
        if (digest.Deaths.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Deaths.Count, "death", "deaths"));
        }
        return parts.Count == 0 ? digest.Headline() : string.Join(" · ", parts);
    }

    /// <summary>Whether an evening matches what is being searched for, across the prose and the log alike: somebody looking for "Nagrand" does not know or care which of the two it is written in.</summary>
    public static bool Matches(Digest digest, Entry? entry, string needle)
    {
        if (entry is not null && (Has(entry.Title, needle) || Has(entry.Body, needle)))
        {
            return true;
        }
        return Has(digest.DisplayName, needle)
            || digest.Route.Any(stop => Has(stop.Zone, needle))
            || digest.Quests.Any(quest => Has(quest.Title, needle))
            || digest.Felled.Any(name => Has(name, needle))
            || digest.Achievements.Any(achievement => Has(achievement.Name, needle));
    }

    private static bool Has(string text, string needle) => text.Contains(needle, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Each book's segments, as shares of everything that moved. The
    /// denominator is income and spending together, so the bar is the shape
    /// of the evening's money rather than of either half: a night that earned
    /// three hundred and spent ten reads as almost all green, and one that
    /// earned three hundred and spent three hundred and forty reads as the
    /// shopping trip it was. Both empty when nothing moved: an empty track
    /// says the opposite of nothing.
    /// </summary>
    public static (List<Segment> Income, List<Segment> Spending) LedgerShares(IReadOnlyList<(Purpose Purpose, long Amount)> income, IReadOnlyList<(Purpose Purpose, long Amount)> spending)
    {
        var total = income.Sum(book => book.Amount) + spending.Sum(book => book.Amount);
        if (total == 0)
        {
            return ([], []);
        }
        List<Segment> Share(IReadOnlyList<(Purpose Purpose, long Amount)> book) =>
            book.Select((line, index) => new Segment(line.Amount / (double)total, index == 0)).ToList();
        return (Share(income), Share(spending));
    }

    /// <summary>Copper as gold alone, which is all a ledger legend has room for.</summary>
    public static string Gold(long copper) => (copper / 10_000) switch
    {
        0 => "under 1g",
        var amount => string.Create(CultureInfo.InvariantCulture, $"{amount:N0}g"),
    };

    /// <summary>A counter's number where the label beside it says what it is. The two that are a measurement keep their unit.</summary>
    public static string CountedAs(Counting kind, long count) => kind switch
    {
        Counting.Zone => Counters.Spent(count),
        Counting.Distance => Counters.Far(count),
        _ => count.ToString("N0", CultureInfo.InvariantCulture),
    };

    /// <summary>The same number said in full, for a tooltip that has room for the noun.</summary>
    public static string Said(Counting kind, long count) => kind switch
    {
        Counting.Zone => Counters.Spent(count),
        Counting.Distance => Counters.Far(count),
        Counting.Recipe => Prose.Plural(count, "time", "times"),
        Counting.Companion => Prose.Plural(count, "evening", "evenings"),
        Counting.Delve => Prose.Plural(count, "delve", "delves"),
        Counting.Questgiver => Prose.Plural(count, "quest", "quests"),
        Counting.Rare => Prose.Plural(count, "kill", "kills"),
        Counting.Attempt => Prose.Plural(count, "attempt", "attempts"),
        _ => count.ToString(CultureInfo.InvariantCulture),
    };

    /// <summary>The counters the rail lists, roughly descending by how much somebody wants to know: what they do, who with, where, and then the jokes about how much of their life this is.</summary>
    public static IReadOnlyList<Counting> RailOrder { get; } =
    [
        Counting.Recipe,
        Counting.Companion,
        Counting.Victory,
        Counting.Attempt,
        Counting.Delve,
        Counting.Zone,
        Counting.Killer,
        Counting.Distance,
        Counting.Flight,
    ];

    /// <summary>
    /// The last fourteen days, oldest first, as a fraction of the longest
    /// day among them. Null is a day nobody played, and it is drawn as an
    /// absence rather than as a zero-height bar: somebody who did not play
    /// on Tuesday did not play a very little on Tuesday. A day per bar, not a
    /// session per bar: somebody who logged in twice played once.
    /// </summary>
    public static List<double?> Fortnight(IEnumerable<Session> sessions, DateOnly today)
    {
        var minutes = new long[Days];
        foreach (var session in sessions)
        {
            var day = today.DayNumber - DateOnly.FromDateTime(session.StartedAt.UtcDateTime).DayNumber;
            if (day is >= 0 and < Days)
            {
                minutes[Days - 1 - day] += Math.Max((long)session.Duration.TotalMinutes, 0);
            }
        }
        var longest = minutes.Max();
        return minutes.Select(played => played == 0 || longest == 0 ? (double?)null : played / (double)longest).ToList();
    }

    /// <summary>How long a fight took, without the reader guessing at the units: under a minute is said outright, past that m:ss means what it looks like.</summary>
    public static string FightLength(long seconds) => seconds < 60
        ? string.Create(CultureInfo.InvariantCulture, $"{seconds}s")
        : string.Create(CultureInfo.InvariantCulture, $"{seconds / 60}:{seconds % 60:00}");

    /// <summary>Where the evening was spent: the stop it stayed longest at, not the one it finished in. An evening in Nagrand that ended with a hearthstone to Dornogal was an evening in Nagrand.</summary>
    public static string? SpentIn(Digest digest) => digest.Route.Count == 0 ? null : digest.Route.MaxBy(stop => stop.Stayed)!.Zone;

    /// <summary>An evening on the run's road: its title and the line beneath.</summary>
    public static (string Title, string Detail) RoadLine(Digest digest)
    {
        var minutes = Math.Max((long)digest.Duration.TotalMinutes, 0);
        var parts = new List<string> { string.Create(CultureInfo.InvariantCulture, $"{minutes / 60}h {minutes % 60}m") };
        if (digest.Quests.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Quests.Count, "quest", "quests"));
        }
        if (digest.Deaths.Count > 0)
        {
            parts.Add(Prose.Plural(digest.Deaths.Count, "death", "deaths"));
        }
        return ($"An evening in {SpentIn(digest) ?? "somewhere unrecorded"}", $"{digest.DisplayName} · {string.Join(" · ", parts)}");
    }

    /// <summary>Seven counts, Monday first, from the evenings' local start times; which weekday is commonest; and the earliest hour anybody started.</summary>
    public static (int[] Days, int Modal, int? Earliest) Weekdays(IEnumerable<Digest> evenings)
    {
        var days = new int[7];
        int? earliest = null;
        foreach (var evening in evenings)
        {
            var local = evening.StartedAt.ToLocalTime();
            days[((int)local.DayOfWeek + 6) % 7]++;
            earliest = earliest is { } hour ? Math.Min(hour, local.Hour) : local.Hour;
        }
        var modal = 0;
        for (var index = 1; index < 7; index++)
        {
            if (days[index] > days[modal])
            {
                modal = index;
            }
        }
        return (days, modal, earliest);
    }

    private static readonly string[] Weekday = ["Mondays", "Tuesdays", "Wednesdays", "Thursdays", "Fridays", "Saturdays", "Sundays"];

    /// <summary>"Tuesdays, mostly, and never before 7pm."</summary>
    public static string WeekdaySentence(int modal, int? earliest) => earliest is { } hour
        ? $"{Weekday[modal]}, mostly, and never before {Clock(hour)}."
        : $"{Weekday[modal]}, mostly.";

    /// <summary>An hour as a person says it.</summary>
    public static string Clock(int hour) => hour switch
    {
        0 => "midnight",
        12 => "midday",
        < 12 => string.Create(CultureInfo.InvariantCulture, $"{hour}am"),
        _ => string.Create(CultureInfo.InvariantCulture, $"{hour - 12}pm"),
    };

    /// <summary>How long Armory has been watching a character, and how much of it there is. No character age exists anywhere, so this is what the header says instead.</summary>
    public static (string Span, string Detail) WatchedFor(IReadOnlyList<Digest> evenings, DateTimeOffset now)
    {
        if (evenings.Count == 0)
        {
            return ("Not yet watched", "No evening on this character has been recorded.");
        }
        var first = evenings.MinBy(evening => evening.StartedAt)!;
        var months = Math.Max((long)(now - first.StartedAt).TotalDays, 0) / 30;
        var span = months switch
        {
            0 => "less than a month recorded",
            1 => "one month recorded",
            _ => string.Create(CultureInfo.InvariantCulture, $"{months} months recorded"),
        };
        return (span, string.Create(CultureInfo.InvariantCulture, $"{Prose.Plural(evenings.Count, "evening", "evenings")} since {first.StartedAt.ToLocalTime():d MMMM yyyy}"));
    }
}
