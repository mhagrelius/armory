using System.Globalization;
using Armory.Blizzard;
using Armory.Collections;
using Armory.Roster;

namespace Armory.Client.Collections;

/// <summary>Which entries the grid is showing.</summary>
public enum Showing
{
    /// <summary>
    /// Missing, and still gettable. A faction-locked mount on the wrong side
    /// and a trading-card mount are not a gap in this account's collection,
    /// and counting them as one overstates the backlog by a few hundred.
    /// </summary>
    Missing,
    Collected,

    /// <summary>Everything in the catalogue, obtainable or not.</summary>
    All,
}

/// <summary>The rail's standing: what is had, out of what this account could ever hold, and what was left out of the count.</summary>
public readonly record struct Standing(long Collected, long Countable, long Unobtainable)
{
    public double Fraction => Countable == 0 ? 0.0 : (double)Collected / Countable;
}

/// <summary>
/// What the collection page's cards and cells say, worked out away from any
/// widget so it can be tested. The pure half of <c>ui/collection_page.rs</c>.
/// </summary>
public static class CollectionCards
{
    /// <summary>How many of the closest to earning to show.</summary>
    public const int Closest = 3;

    /// <summary>The rail's toggle, left to right.</summary>
    public static IReadOnlyList<Showing> Toggle { get; } = [Showing.Missing, Showing.Collected, Showing.All];

    public static string Label(this Showing showing) => showing switch
    {
        Showing.All => "All",
        Showing.Collected => "Collected",
        _ => "Missing",
    };

    /// <summary>
    /// What a count of this view is a count <em>of</em>. "318 missing" and
    /// "318 collected" are opposite facts and a group heading that said only
    /// "318" would be either, depending on a toggle elsewhere.
    /// </summary>
    public static string Counted(this Showing showing) => showing switch
    {
        Showing.All => "in all",
        Showing.Collected => "collected",
        _ => "missing",
    };

    /// <summary>
    /// The day the weekly lockout turns over, which is the clock a collector
    /// plans a raid week around. Blizzard resets the Americas on Tuesday,
    /// Europe on Wednesday and the Asian regions on Thursday.
    /// </summary>
    public static string ResetDay(Region region) => region switch
    {
        Region.Us => "TUESDAY",
        Region.Eu => "WEDNESDAY",
        _ => "THURSDAY",
    };

    /// <summary>
    /// What hovering a cell says. The cell shows a name over one word. The
    /// journal's sentence, "Drop: Lord Aurius Rivendare, Stratholme", is the
    /// thing worth reading and does not fit, so it is a tooltip rather than a
    /// truncation.
    /// </summary>
    public static string Tooltip(Collectible collectible)
    {
        var lines = new List<string> { collectible.Name };
        if (collectible.Description is { Length: > 0 } text)
        {
            lines.AddRange(Lines(text));
        }
        else if (collectible.Source != Source.Unknown)
        {
            lines.Add(collectible.Source.Label());
        }
        else
        {
            lines.Add("No source recorded");
        }
        return string.Join("\n", lines);
    }

    /// <summary>
    /// The second line of one of the three cards: where the thing comes from,
    /// in the journal's own words where there are any. The addon records
    /// "Vendor: Unger Statforth / Zone: Wetlands" and the web API records the
    /// word VENDOR, so this takes the sentence when there is one and falls
    /// back to the word.
    /// </summary>
    public static string Whence(Collectible collectible)
    {
        var sentence = string.Join(" · ", Lines(collectible.Description ?? "").Select(line => line.Trim()).Where(line => line.Length > 0));
        return sentence.Length == 0 ? collectible.Source.Label() : sentence;
    }

    /// <summary>Whether the toggle keeps an entry.</summary>
    public static bool Keeps(Showing showing, bool owned, bool obtainable) => showing switch
    {
        Showing.All => true,
        Showing.Collected => owned,
        _ => !owned && obtainable,
    };

    /// <summary>Whether the search finds an entry: its name, its source's word and the journal's sentence, all at once.</summary>
    public static bool Matches(Collectible collectible, string needle)
    {
        if (needle.Length == 0)
        {
            return true;
        }
        var haystack = $"{collectible.Name} {collectible.Source.Label()} {collectible.Description ?? ""}";
        return haystack.Contains(needle, StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// What the grid shows, in the order somebody sees it: source group
    /// first, then name. Fixed rather than offered as a choice. The page
    /// <em>is</em> the grouping, so a second ordering would be a grid whose
    /// headings no longer bracket their own entries, and <see cref="ArtWanted"/>
    /// reads this order as the order somebody sees.
    /// </summary>
    public static List<Collectible> Shown(IEnumerable<Collectible> catalogue, IReadOnlySet<long> owned, Faction faction, Showing showing, string needle) =>
        catalogue
            .Where(entry => Keeps(showing, owned.Contains(entry.Id), entry.ObtainableBy(faction)) && Matches(entry, needle))
            .OrderBy(entry => SourceGroups.Rank(entry.Source))
            .ThenBy(entry => entry.Name.ToLowerInvariant(), StringComparer.Ordinal)
            .ThenBy(entry => entry.Id)
            .ToList();

    /// <summary>
    /// The three the page opens by recommending: missing, obtainable, and
    /// ordered by how little chance stands in the way. See
    /// <see cref="SourceGroups.Certainty"/>: with no rarity source there is no
    /// "closest" that is a measurement, so this is the nearest honest thing,
    /// the entries whose cost is time or gold rather than a roll.
    /// </summary>
    public static List<Collectible> PickClosest(IEnumerable<Collectible> catalogue, IReadOnlySet<long> owned, Faction faction) =>
        catalogue
            .Where(entry => !owned.Contains(entry.Id) && entry.ObtainableBy(faction))
            .OrderBy(entry => SourceGroups.Certainty(entry.Source) ?? byte.MaxValue)
            .ThenBy(entry => entry.Name.ToLowerInvariant(), StringComparer.Ordinal)
            .ThenBy(entry => entry.Id)
            .Take(Closest)
            .ToList();

    /// <summary>
    /// The gold line on one of the three cards, or null when there is nothing
    /// gold to say. Two different lines, and neither is a drop rate: the first
    /// says chance is not in the way at all; the second says the odds Rarity
    /// estimates and how many times this account has already fought the thing
    /// that drops it, either half alone when only one is known.
    /// </summary>
    public static string? Claim(Source source, long? oneIn, (string Fought, long Tries)? fought)
    {
        if (SourceGroups.NoChance(source) is { } line)
        {
            return line;
        }
        return (oneIn, fought) switch
        {
            ({ } odds, { } pulls) => string.Create(CultureInfo.InvariantCulture, $"1 IN {odds}\n{Tries(pulls.Tries)}"),
            ({ } odds, null) => string.Create(CultureInfo.InvariantCulture, $"1 IN {odds}"),
            (null, { } pulls) => Tries(pulls.Tries),
            _ => null,
        };
    }

    /// <summary>
    /// What the gold line on a card actually means, said in full. The two
    /// halves come from different places and are worth different amounts, so
    /// the tooltip names both rather than letting a single line read as one
    /// measurement.
    /// </summary>
    public static string? ClaimTooltip(long? oneIn, (string Fought, long Tries)? fought) => (oneIn, fought) switch
    {
        ({ } odds, { } pulls) => string.Create(CultureInfo.InvariantCulture, $"Roughly a one in {odds} chance, estimated by the Rarity addon — Blizzard publishes no drop rates. This account has pulled {pulls.Fought} {pulls.Tries} times, which is Armory's own count."),
        ({ } odds, null) => string.Create(CultureInfo.InvariantCulture, $"Roughly a one in {odds} chance, estimated by the Rarity addon — Blizzard publishes no drop rates."),
        (null, { } pulls) => string.Create(CultureInfo.InvariantCulture, $"This account has pulled {pulls.Fought} {pulls.Tries} times. No drop rate for this one: install the Rarity addon and Armory will read its estimate from your own copy."),
        _ => null,
    };

    /// <summary>
    /// The standing. The denominator is what this account could ever hold,
    /// which is the same rule the run applies to its own ring: counting the
    /// impossible produces a bar that can never fill. What was left out is
    /// returned so the page can say so rather than quietly applying it.
    /// </summary>
    public static Standing Counts(IEnumerable<Collectible> catalogue, IReadOnlySet<long> owned, Faction faction)
    {
        long collected = 0, obtainable = 0, total = 0;
        foreach (var entry in catalogue)
        {
            total++;
            if (owned.Contains(entry.Id))
            {
                collected++;
            }
            else if (entry.ObtainableBy(faction))
            {
                obtainable++;
            }
        }
        return new Standing(collected, collected + obtainable, total - collected - obtainable);
    }

    /// <summary>
    /// Which item icons to spend the media budget on: the entries being
    /// shown, in the order they are shown, and then everything held, so
    /// "Fetch Missing Artwork" means every picture that is missing rather
    /// than every picture on the tab somebody happens to be looking at.
    /// Nothing is asked for twice, and an entry whose item is a guess is
    /// skipped, because the icon of item 5 when 5 is a decor id is a real
    /// icon for the wrong thing.
    /// </summary>
    public static List<long> ArtWanted(IEnumerable<Collectible> shown, IEnumerable<Collectible> held, Func<Collectible, bool> hasArt, int limit)
    {
        var wanted = new List<long>();
        var seen = new HashSet<long>();
        foreach (var entry in shown.Concat(held))
        {
            if (wanted.Count >= limit)
            {
                break;
            }
            if (hasArt(entry) || entry.KnownItemId() is not { } item || item <= 0 || !seen.Add(item))
            {
                continue;
            }
            wanted.Add(item);
        }
        return wanted;
    }

    private static string Tries(long tries) => string.Create(CultureInfo.InvariantCulture, $"{tries:N0} {(tries == 1 ? "TRY" : "TRIES")}");

    private static IEnumerable<string> Lines(string text) => text.ReplaceLineEndings("\n").TrimEnd('\n').Split('\n');
}
