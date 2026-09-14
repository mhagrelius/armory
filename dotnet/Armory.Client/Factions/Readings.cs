using System.Globalization;
using Armory.Blizzard;
using Armory.Provenance;

namespace Armory.Client.Factions;

/// <summary>
/// What can honestly be said about who earned one standing. Not
/// <see cref="Origin"/>, which is the same question asked of a currency and
/// has a fourth answer: a currency can be transferred between characters and
/// a standing cannot.
/// </summary>
public enum Reading
{
    /// <summary>The addon watched this character earn some of it.</summary>
    Earned,

    /// <summary>The addon was watching and this character earned none of it. An honest zero, which is a different fact from an unwatched one.</summary>
    Nothing,

    /// <summary>Warbands handed it over before the run began. It cannot move, so nothing about it counts towards the run.</summary>
    Inherited,

    /// <summary>Nobody was watching. Who did the work cannot be said.</summary>
    Unclear,
}

/// <summary>
/// The reputations page's arithmetic, out of the page so it has tests. The
/// page is one bar with two readings: pale is where the account already
/// stands, gold is what the character in front of you was watched earning.
/// Only the gold moves, and only the gold is ever claimed by the run.
/// </summary>
public static class Readings
{
    /// <summary>The classic ladder's ends, as ranks. <see cref="Standings.StandingEarned"/> answers in these.</summary>
    private const int Neutral = 4;
    private const int Exalted = 8;

    /// <summary>
    /// Whether there is a measurement here to draw. False for exactly one
    /// case, and it is the point of the enum: an unwatched standing has a
    /// floor and no measurement, and a bar drawn over a floor reads as the
    /// measurement it is not.
    /// </summary>
    public static bool IsMeasured(this Reading reading) => reading != Reading.Unclear;

    /// <summary>
    /// Which of the three claims a standing supports. Order matters: work
    /// this character was watched doing outranks the inherited flag, because
    /// an inherited faction somebody has been grinding anyway is the most
    /// interesting row on the page. The flag outranks silence, because it is
    /// a positive fact rather than an absence.
    /// </summary>
    public static Reading Of(FactionStanding standing, Earned? earned)
    {
        if (earned is not null && earned.HasTouched(standing.Faction))
        {
            return Reading.Earned;
        }
        if (standing.Inherited)
        {
            return Reading.Inherited;
        }
        return earned is null ? Reading.Unclear : Reading.Nothing;
    }

    /// <summary>
    /// How much of where the account stands this character can be shown to
    /// have earned. Renown answers in levels because that is the shape it
    /// has; the classic ladder answers in ranks, because a rank is what a
    /// player reads and the point thresholds between them are wildly uneven.
    /// </summary>
    public static double Share(FactionStanding standing, EarnedReputation mine)
    {
        if (standing.Renown > 0)
        {
            return Math.Min(mine.Renown, standing.Renown) / (double)standing.Renown;
        }
        var (rank, _) = Standings.StandingEarned(mine);
        var climbed = (double)Math.Max(rank - Neutral, 0);
        var partial = Standings.FractionEarned(mine) ?? 0.0;
        return Math.Clamp((climbed + partial) / (Exalted - Neutral), 0.0, 1.0);
    }

    /// <summary>How many of a character's standings support each of the three claims.</summary>
    public static (int Earned, int Inherited, int Unclear) Tally(IEnumerable<FactionStanding> standings, Earned? earned)
    {
        var counts = (Earned: 0, Inherited: 0, Unclear: 0);
        foreach (var standing in standings)
        {
            switch (Of(standing, earned))
            {
                case Reading.Earned:
                    counts.Earned++;
                    break;
                case Reading.Inherited:
                    counts.Inherited++;
                    break;
                case Reading.Unclear:
                    counts.Unclear++;
                    break;
                default:
                    break;
            }
        }
        return counts;
    }

    /// <summary>Where the account stands, as a word.</summary>
    public static string Tier(FactionStanding standing) => standing.Tier.Length > 0
        ? standing.Tier
        : standing.Renown > 0 ? string.Create(CultureInfo.InvariantCulture, $"Renown {standing.Renown}") : "no standing recorded";

    /// <summary>The word to the right of a faction's name: this character's own standing, what their own work would have reached from nothing.</summary>
    public static string Badge(Reading reading, EarnedReputation mine)
    {
        if (reading == Reading.Earned)
        {
            var (rank, name) = Standings.StandingEarned(mine);
            return name == "Renown" ? string.Create(CultureInfo.InvariantCulture, $"RENOWN {rank}") : name.ToUpperInvariant();
        }
        return reading switch
        {
            Reading.Inherited => "INHERITED",
            Reading.Unclear => "CANNOT TELL",
            // Watched, and nothing earned yet. Said in words rather than as a
            // gold "NEUTRAL", which would spend the accent on no work at all.
            _ => "NOTHING EARNED YET",
        };
    }

    /// <summary>What this character was watched earning, in their own numbers.</summary>
    public static string EarnedLine(FactionStanding standing, EarnedReputation mine, string who)
    {
        if (mine.Points == 0 && mine.Renown == 0)
        {
            return $"Nothing watched being earned by {who} yet";
        }
        if (standing.Renown > 0)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{mine.Renown} of {standing.Renown} renown earned by {who}");
        }
        var (_, reached) = Standings.StandingEarned(mine);
        return string.Create(CultureInfo.InvariantCulture, $"{mine.Points:N0} reputation earned by {who} — {reached} from nothing");
    }
}
