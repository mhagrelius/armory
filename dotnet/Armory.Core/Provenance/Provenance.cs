using System.Text.Json.Serialization;
using Armory.Roster;

namespace Armory.Provenance;

/// <summary>
/// What one character has personally earned with one faction.
/// </summary>
/// <remarks>
/// The War Within syncs most standings across the Warband to the
/// furthest-progressed character, so a character made yesterday reads
/// Exalted with factions it has never met. A person replaying the game can
/// earn the equivalent of Exalted with a faction the account maxed out in
/// 2023, and the standing cannot move to record it. One client, one
/// character at a time: a standing that rises between login and logout rose
/// because of that character, and the addon needs two snapshots and a
/// subtraction to know it.
/// </remarks>
public sealed record EarnedReputation
{
    /// <summary>Reputation points this character earned, paragon included. Cumulative; not the standing.</summary>
    [JsonPropertyName("points")]
    public long Points { get; init; }

    /// <summary>Renown levels this character earned. The number that means something to a person.</summary>
    [JsonPropertyName("renown")]
    public long Renown { get; init; }

    /// <summary>The highest renown level this character has personally seen: what the account showed them, not what they earned.</summary>
    [JsonPropertyName("renown_seen")]
    public long RenownSeen { get; init; }

    /// <summary>Whether the faction is account-wide at all. When it is not, the standing was already honest.</summary>
    [JsonPropertyName("account_wide")]
    public bool AccountWide { get; init; }
}

/// <summary>Where an amount on a character came from.</summary>
public enum Origin
{
    /// <summary>This character did the work.</summary>
    Earned,

    /// <summary>It arrived without being earned: more turned up than the game counted as earned, on something the Warband can move.</summary>
    Transferred,

    /// <summary>It was there before anybody was watching.</summary>
    Existing,

    /// <summary>It rose, and there is no way to tell which. Said plainly rather than guessed: a confident wrong attribution inflates a run.</summary>
    Unclear,
}

public static class OriginExtensions
{
    public static string Label(this Origin origin) => origin switch
    {
        Origin.Earned => "Earned",
        Origin.Transferred => "Transferred",
        Origin.Existing => "Already held",
        _ => "Unclear",
    };

    /// <summary>Whether a run may count this as progress. Only the first.</summary>
    public static bool Counts(this Origin origin) => origin == Origin.Earned;
}

/// <summary>What one character has personally gained of one currency.</summary>
public sealed record EarnedCurrency
{
    /// <summary>How much arrived while this character was logged in, by any means.</summary>
    [JsonPropertyName("gained")]
    public long Gained { get; init; }

    /// <summary>How much of that the game itself calls earned. Only meaningful when <see cref="TracksEarned"/>.</summary>
    [JsonPropertyName("earned")]
    public long Earned { get; init; }

    /// <summary>Whether the game maintains an earned total for this currency at all. Its flat zero otherwise is not "earned nothing".</summary>
    [JsonPropertyName("tracks_earned")]
    public bool TracksEarned { get; init; }

    [JsonPropertyName("account_wide")]
    public bool AccountWide { get; init; }

    /// <summary>Whether the Warband can move it between characters, which is what makes a rise ambiguous.</summary>
    [JsonPropertyName("transferable")]
    public bool Transferable { get; init; }

    /// <summary>
    /// Where this character's holding came from. Nothing arrived: already
    /// there. Cannot be transferred: earned here. The game tracks an earned
    /// total: believe it. Otherwise: unknowable, and said so.
    /// </summary>
    [JsonIgnore]
    public Origin Origin
    {
        get
        {
            if (Gained == 0 && Earned == 0)
            {
                return Origin.Existing;
            }
            if (!Transferable)
            {
                return Origin.Earned;
            }
            if (TracksEarned)
            {
                return Earned >= Gained ? Origin.Earned : Origin.Transferred;
            }
            return Origin.Unclear;
        }
    }

    /// <summary>How much of it a run may count.</summary>
    public long Creditable() => Origin switch
    {
        Origin.Earned => Math.Max(Gained, Earned),
        // The part the game vouched for, and not a copper more.
        Origin.Transferred => Earned,
        _ => 0,
    };
}

/// <summary>Everything one character has been observed earning.</summary>
public sealed record Earned
{
    /// <summary>Faction id to what this character earned with them.</summary>
    [JsonPropertyName("reputation")]
    public Dictionary<long, EarnedReputation> Reputation { get; init; } = [];

    /// <summary>Currency id to what this character gained of it.</summary>
    [JsonPropertyName("currency")]
    public Dictionary<long, EarnedCurrency> Currency { get; init; } = [];

    public EarnedReputation With(long faction) => Reputation.GetValueOrDefault(faction) ?? new EarnedReputation();

    /// <summary>Whether this character has personally done anything with a faction.</summary>
    public bool HasTouched(long faction) => With(faction).Points > 0 || With(faction).Renown > 0;
}

/// <summary>The whole account's answer to "who did this".</summary>
public sealed class Earnings : Dictionary<CharacterKey, Earned>
{
}

public static class Standings
{
    /// <summary>
    /// How much reputation the game asks for between one standing and the
    /// next: Blizzard's classic ladder, cumulative from Neutral. What a replay
    /// wants is "how much would this character have needed from nothing".
    /// </summary>
    private static readonly (int Rank, string Name, long Threshold)[] Ladder =
    [
        (5, "Friendly", 3_000),
        (6, "Honored", 9_000),
        (7, "Revered", 21_000),
        (8, "Exalted", 42_000),
        // Beyond Exalted there is only paragon, which has no standing of its own.
        (9, "Paragon", 42_000 + 10_000),
    ];

    /// <summary>
    /// The standing this character's own earned reputation would have
    /// reached, starting from Neutral. The number the soft reset is about.
    /// Renown is answered on levels, because that is the shape it has.
    /// </summary>
    public static (int Rank, string Name) StandingEarned(EarnedReputation earned)
    {
        if (earned.Renown > 0)
        {
            return ((int)Math.Min(earned.Renown, 255), "Renown");
        }
        var reached = (4, "Neutral");
        foreach (var (rank, name, threshold) in Ladder)
        {
            if (earned.Points >= threshold)
            {
                reached = (rank, name);
            }
        }
        return reached;
    }

    /// <summary>How far through the current standing this character's own work has taken them, or null at the top.</summary>
    public static double? FractionEarned(EarnedReputation earned)
    {
        if (earned.Renown > 0)
        {
            return null;
        }
        long floor = 0;
        foreach (var (_, _, threshold) in Ladder)
        {
            if (earned.Points >= threshold)
            {
                floor = threshold;
            }
            else
            {
                var span = threshold - floor;
                return span == 0 ? null : (earned.Points - floor) / (double)span;
            }
        }
        return null;
    }
}
